//! Keep public error enums small and actionable; one reported catch-all with a reason
//!
//! Docs and examples: `rules/uniffi-public-errors-actionable/`.

use std::collections::BTreeSet;

use quote::ToTokens;
use syn::visit::Visit;

use crate::helpers::api_mobile::{is_production_rust, is_uniffi_crate_file};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};

pub static RULE: Rule = Rule {
    id: "uniffi-public-errors-actionable",
    area: Area::Uniffi,
    title: "Keep public error enums small and actionable; one reported catch-all with a reason",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// `Other`, `Unexpected*`, `Internal*`, `Unknown`: the variant everything
/// unexpected ends up in.
fn is_catch_all(variant: &str) -> bool {
    ["Unexpected", "Other", "Internal"]
        .iter()
        .any(|p| variant.starts_with(p))
        || matches!(
            variant,
            "Unknown" | "UnknownError" | "UnknownException" | "Generic" | "GenericError"
        )
}

fn check(cx: &Context, out: &mut Findings) {
    let files: Vec<&RustFile> = cx
        .ws
        .rust_files()
        .filter(|f| is_production_rust(&f.path) && is_uniffi_crate_file(cx.ws, &f.path))
        .collect();
    let udl_errors = udl_error_names(cx);
    let constructed = constructed_variants(cx);
    let foreign_crates = crates_with_foreign_traits(cx);
    for file in &files {
        let krate = cx
            .ws
            .package_of(&file.path)
            .map(|p| p.dir.clone())
            .unwrap_or_default();
        for item in syntax::all_items(&file.ast) {
            let syn::Item::Enum(e) = item else { continue };
            let name = e.ident.to_string();
            let line = file.line_of_node(&e.ident);
            let exported = syntax::has_derive(&e.attrs, "uniffi::Error")
                || udl_errors.contains(&(krate.clone(), name.clone()));
            if !exported || file.in_test(line) {
                continue;
            }
            let mut dead: Vec<(String, usize)> = Vec::new();
            for v in &e.variants {
                let variant = v.ident.to_string();
                let vline = file.line_of_node(&v.ident);
                if is_catch_all(&variant) && v.fields.is_empty() {
                    out.add(
                        &file.path,
                        vline,
                        format!(
                            "catch-all `{name}::{variant}` carries no reason, so callers and crash reports only see its name; give it a `reason: String`"
                        ),
                    );
                    continue;
                }
                // thiserror's `#[from]` writes the constructor for us.
                let from = v
                    .fields
                    .iter()
                    .any(|f| f.attrs.iter().any(|a| a.path().is_ident("from")));
                let used = from
                    || constructed.contains(&(name.clone(), variant.clone()))
                    || constructed.contains(&(name.clone(), "*".to_string()));
                if !used && !foreign_crates.contains(&krate) {
                    dead.push((variant, vline));
                }
            }
            if let Some((first, line)) = dead.first() {
                let message = match dead.len() {
                    1 => format!(
                        "`{name}::{first}` is never constructed in Rust; remove it, or document who throws it"
                    ),
                    n => format!(
                        "{n} variants of `{name}` are never constructed in Rust ({}); remove them, or document who throws them",
                        dead.iter().map(|(v, _)| format!("`{v}`")).collect::<Vec<_>>().join(", ")
                    ),
                };
                out.add(&file.path, *line, message);
            }
        }
        for imp in syntax::all_items(&file.ast) {
            if let syn::Item::Impl(i) = imp {
                error_handling(file, &krate, i, out);
            }
        }
    }
}

/// `(crate dir, name)` of `[Error]` enums and interfaces declared in UDL.
fn udl_error_names(cx: &Context) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for file in cx.ws.files_with_ext(&["udl"]) {
        let Some(package) = cx.ws.package_of(&file.path) else {
            continue;
        };
        for item in udl::parse(&file.text) {
            if matches!(item.kind, UdlKind::Enum | UdlKind::Interface) && item.has_attr("Error") {
                out.insert((package.dir.clone(), item.name));
            }
        }
    }
    out
}

/// Crates (by dir) that export a trait foreign code implements: its errors
/// may be thrown from Kotlin/Swift, where this rule can't see.
fn crates_with_foreign_traits(cx: &Context) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in cx.ws.files() {
        let Some(package) = cx.ws.package_of(&file.path) else {
            continue;
        };
        let foreign = if file.extension() == "udl" {
            udl::parse(&file.text).iter().any(|i| {
                i.kind == UdlKind::CallbackInterface
                    || (i.kind == UdlKind::Interface && i.attrs.contains("WithForeign"))
            })
        } else if let Some(rust) = cx.ws.rust(&file.path) {
            syntax::all_items(&rust.ast).iter().any(|item| {
                matches!(item, syn::Item::Trait(t) if syntax::find_attr(&t.attrs, "uniffi::export")
                    .map(syntax::attr_args)
                    .is_some_and(|a| a.contains("callback_interface") || a.contains("with_foreign")))
            })
        } else {
            false
        };
        if foreign {
            out.insert(package.dir.clone());
        }
    }
    out
}

/// Every `(Enum, Variant)` built in an expression anywhere in production
/// Rust (`E::V`, `E::V(..)`, `E::V { .. }`, `Self::V` in `impl E`), plus
/// `(Enum, "*")` for `use E::*`. Macro bodies count conservatively.
fn constructed_variants(cx: &Context) -> BTreeSet<(String, String)> {
    let mut v = Constructed {
        out: BTreeSet::new(),
        self_ty: Vec::new(),
        file: None,
    };
    for file in cx.ws.rust_files() {
        if !is_production_rust(&file.path) {
            continue;
        }
        v.file = Some(file);
        v.visit_file(&file.ast);
    }
    v.out
}

struct Constructed<'a> {
    out: BTreeSet<(String, String)>,
    self_ty: Vec<String>,
    file: Option<&'a RustFile>,
}

impl Constructed<'_> {
    fn path(&mut self, path: &syn::Path, line: usize) {
        if self.file.is_some_and(|f| f.in_test(line)) {
            return;
        }
        let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.len() < 2 {
            return;
        }
        let (ty, variant) = (&segs[segs.len() - 2], &segs[segs.len() - 1]);
        let ty = if ty == "Self" {
            self.self_ty.last().cloned().unwrap_or_default()
        } else {
            ty.clone()
        };
        self.out.insert((ty, variant.clone()));
    }
}

impl<'ast> Visit<'ast> for Constructed<'_> {
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        self.self_ty
            .push(syntax::type_ident(&i.self_ty).unwrap_or_default());
        syn::visit::visit_item_impl(self, i);
        self.self_ty.pop();
    }
    fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
        let line = e
            .path
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        self.path(&e.path, line);
        syn::visit::visit_expr_path(self, e);
    }
    fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
        let line = e
            .path
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        self.path(&e.path, line);
        syn::visit::visit_expr_struct(self, e);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let tokens: Vec<proc_macro2::TokenTree> = flatten(m.tokens.clone());
        for w in tokens.windows(4) {
            if let (
                proc_macro2::TokenTree::Ident(ty),
                proc_macro2::TokenTree::Punct(a),
                proc_macro2::TokenTree::Punct(b),
                proc_macro2::TokenTree::Ident(variant),
            ) = (&w[0], &w[1], &w[2], &w[3])
            {
                if a.as_char() == ':' && b.as_char() == ':' {
                    let ty = if ty == "Self" {
                        self.self_ty.last().cloned().unwrap_or_default()
                    } else {
                        ty.to_string()
                    };
                    self.out.insert((ty, variant.to_string()));
                }
            }
        }
        syn::visit::visit_macro(self, m);
    }
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let text = syntax::compact(&u.tree.to_token_stream().to_string());
        // `use Error::*` / `use crate::error::Error::{A, B}`.
        if let Some(prefix) = text.strip_suffix("::*") {
            let ty = prefix.rsplit("::").next().unwrap_or(prefix);
            self.out.insert((ty.to_string(), "*".to_string()));
        }
        if let Some((prefix, list)) = text.split_once("::{") {
            let ty = prefix.rsplit("::").next().unwrap_or(prefix);
            for v in list.trim_end_matches('}').split(',') {
                self.out.insert((ty.to_string(), v.trim().to_string()));
            }
        }
    }
}

fn flatten(stream: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenTree> {
    let mut out = Vec::new();
    for tt in stream {
        match tt {
            proc_macro2::TokenTree::Group(g) => out.extend(flatten(g.stream())),
            other => out.push(other),
        }
    }
    out
}

/// The fallback of `get_error_handling`: the `_ =>` arm of its top-level
/// `match`, or the whole body when it converts unconditionally.
fn error_handling(file: &RustFile, krate: &str, imp: &syn::ItemImpl, out: &mut Findings) {
    let is_handling = imp
        .trait_
        .as_ref()
        .is_some_and(|(_, p, _)| syntax::path_ends_with(p, "GetErrorHandling"));
    if !is_handling {
        return;
    }
    for it in &imp.items {
        let syn::ImplItem::Fn(f) = it else { continue };
        if f.sig.ident != "get_error_handling" {
            continue;
        }
        let (fallback, wildcard): (&syn::Expr, bool) = match tail_expr(&f.block) {
            Some(syn::Expr::Match(m)) => {
                match m.arms.iter().find(|a| matches!(a.pat, syn::Pat::Wild(_))) {
                    Some(arm) => (&arm.body, true),
                    None => continue,
                }
            }
            Some(other) => (other, false),
            None => continue,
        };
        let Some((ty, variant, line)) = converted_variant(fallback) else {
            continue;
        };
        let text = syntax::tokens(fallback);
        let reported = text.contains("report_error") || text.contains(". report (");
        let component = krate.rsplit('/').next().unwrap_or(krate).replace('_', "-");
        if is_catch_all(&variant) && !reported {
            out.add(
                &file.path,
                line,
                format!(
                    "unexpected errors become `{ty}::{variant}` but are never reported; add `.report_error(\"{component}-unexpected\")`"
                ),
            );
        } else if !is_catch_all(&variant) && wildcard {
            out.add(
                &file.path,
                line,
                format!(
                    "the `_` arm turns every unlisted error into `{ty}::{variant}`; list the errors that mean that, and send the rest to the catch-all"
                ),
            );
        }
    }
}

fn tail_expr(block: &syn::Block) -> Option<&syn::Expr> {
    match block.stmts.last()? {
        syn::Stmt::Expr(e, None) => Some(e),
        _ => None,
    }
}

/// `(Enum, Variant, line)` of the `ErrorHandling::convert(Enum::Variant ..)` in `expr`.
fn converted_variant(expr: &syn::Expr) -> Option<(String, String, usize)> {
    struct Find(Option<(String, String, usize)>);
    impl<'ast> Visit<'ast> for Find {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if self.0.is_some() {
                return;
            }
            let is_convert = matches!(&*call.func, syn::Expr::Path(p) if syntax::path_ends_with(&p.path, "convert"));
            if is_convert {
                let path = match call.args.first() {
                    Some(syn::Expr::Struct(s)) => Some(&s.path),
                    Some(syn::Expr::Path(p)) => Some(&p.path),
                    Some(syn::Expr::Call(c)) => match &*c.func {
                        syn::Expr::Path(p) => Some(&p.path),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(path) = path.filter(|p| p.segments.len() >= 2) {
                    let n = path.segments.len();
                    self.0 = Some((
                        path.segments[n - 2].ident.to_string(),
                        path.segments[n - 1].ident.to_string(),
                        call.func
                            .to_token_stream()
                            .into_iter()
                            .next()
                            .map_or(0, |t| t.span().start().line),
                    ));
                    return;
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let mut find = Find(None);
    find.visit_expr(expr);
    find.0
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/store/Cargo.toml",
        "[package]\nname = \"store\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const PATH: &str = "components/store/src/error.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-public-errors-actionable/dont.rs"),
                ),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 6, 13], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-public-errors-actionable/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_dead_variants_but_not_foreign_thrown_ones() {
        let src = r#"
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StoreApiError {
    #[error("busy")]
    Busy,
    #[error("never")]
    Never,
    #[error("Other: {reason}")]
    Other { reason: String },
}
fn f() -> StoreApiError { if x() { StoreApiError::Busy } else { error!("{}", StoreApiError::Other { reason: "a".into() }) } }
"#;
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0]
            .message
            .contains("`StoreApiError::Never` is never constructed"));
        let foreign = "#[uniffi::export(with_foreign)]\npub trait KeyManager { fn get_key(&self) -> Result<Vec<u8>, StoreApiError>; }\n";
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (PATH, src),
                ("components/store/src/keys.rs", foreign),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn reads_udl_errors_and_wildcards() {
        let udl = "[Error]\ninterface StoreApiError {\n    NoSuchRecord(string reason);\n    Other(string reason);\n};\n";
        let src = r#"
pub enum StoreApiError {
    NoSuchRecord { reason: String },
    Other { reason: String },
}
impl GetErrorHandling for Error {
    type ExternalError = StoreApiError;
    fn get_error_handling(&self) -> ErrorHandling<Self::ExternalError> {
        match self {
            Self::Other(_) => ErrorHandling::convert(StoreApiError::Other { reason: self.to_string() })
                .report_error("store-unexpected"),
            _ => ErrorHandling::convert(StoreApiError::NoSuchRecord { reason: self.to_string() }),
        }
    }
}
"#;
        let found = check(
            &RULE,
            &[
                MANIFEST,
                ("components/store/src/store.udl", udl),
                (PATH, src),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("the `_` arm"), "{found:#?}");
        assert_eq!(found[0].line, 12);
    }

    #[test]
    fn counts_from_conversions_as_constructors() {
        let src = "#[derive(Debug, thiserror::Error, uniffi::Error)]\n#[uniffi(flat_error)]\npub enum SyncError {\n    #[error(\"{0}\")]\n    Sync15(#[from] sync15::Error),\n}\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, src)]).is_empty());
    }

    #[test]
    fn ignores_internal_errors() {
        let src = "pub enum Error {\n    #[error(\"json\")]\n    Json(serde_json::Error),\n    Other,\n}\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, src)]).is_empty());
    }
}
