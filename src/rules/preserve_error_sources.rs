//! Keep error sources typed (`#[from]`), not stringified or mapped to an unrelated variant
//!
//! Docs and examples: `rules/preserve-error-sources/`.
//!
//! Flags conversions into a crate's *internal* error enum that keep only the
//! source's text: `.map_err(|e| Error::X(e.to_string()))`, `format!("{e}")`
//! into a variant, and `impl From<E> for Error` bodies that do the same; and a
//! `#[from] rusqlite::Error`/`std::io::Error` on a variant named for one
//! operation. Errors exported through UniFFI carry strings by design and are
//! left alone.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::uniffi::{self, ApiKind};

pub static RULE: Rule = Rule {
    id: "preserve-error-sources",
    area: Area::Errors,
    title:
        "Keep error sources typed (`#[from]`), not stringified or mapped to an unrelated variant",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Source types that any operation of a component can raise.
const BROAD_SOURCES: &[&str] = &["rusqlite::Error", "std::io::Error", "io::Error"];

/// Words that name a kind of failure rather than one operation.
const GENERIC_WORDS: &[&str] = &[
    "Sql", "Sqlite", "Database", "Db", "Io", "IO", "Storage", "Rusqlite", "Error", "File",
];

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        // Error enums the apps see: they must carry strings.
        let exported: BTreeSet<String> = uniffi::package_api(cx.ws, package)
            .into_iter()
            .filter(|i| i.kind == ApiKind::Error)
            .map(|i| i.name)
            .collect();
        for file in cx.ws.package_files(package) {
            let Some(rust) = cx.ws.rust(&file.path).filter(|r| errors::is_shipped(&r.path))
            else {
                continue;
            };
            let support = TestSupport::of(rust);
            let mut v = Conversions {
                exported: &exported,
                hits: Vec::new(),
            };
            v.visit_file(&rust.ast);
            for (line, col, msg) in v.hits {
                if !support.contains(line) {
                    out.add_at(&rust.path, line, col, msg);
                }
            }
        }
    }
}

struct Conversions<'a> {
    exported: &'a BTreeSet<String>,
    hits: Vec<(usize, usize, String)>,
}

impl Conversions<'_> {
    /// `Error::Variant(<stringified e>)` or `Error::Variant { reason: <stringified e> }`
    /// where `Error` is internal: the variant path, if so.
    fn stringifying_variant(&self, expr: &syn::Expr, var: &str) -> Option<String> {
        let (path, args): (&syn::Path, Vec<&syn::Expr>) = match expr {
            syn::Expr::Call(c) => match &*c.func {
                syn::Expr::Path(p) => (&p.path, c.args.iter().collect()),
                _ => return None,
            },
            syn::Expr::Struct(s) => (&s.path, s.fields.iter().map(|f| &f.expr).collect()),
            _ => return None,
        };
        if path.segments.len() < 2 {
            return None;
        }
        let enum_name = path.segments[path.segments.len() - 2].ident.to_string();
        let internal = !self.exported.contains(&enum_name)
            && !enum_name.ends_with("ApiError")
            && enum_name != "Self";
        (internal && args.iter().any(|a| is_stringified(a, var)))
            .then(|| syntax::path_string(path))
    }
}

/// `e.to_string()`, `format!("{e}")`, `format!("{}", e)`, `format!("{e:?}")`:
/// nothing but the error's text.
fn is_stringified(expr: &syn::Expr, var: &str) -> bool {
    match expr {
        syn::Expr::MethodCall(m) => {
            m.method == "to_string" && m.args.is_empty() && text(&m.receiver) == var
        }
        syn::Expr::Macro(m) if syntax::macro_name(&m.mac) == "format" => {
            let Some(args) = errors::macro_args(&m.mac) else {
                return false;
            };
            let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(fmt),
                ..
            })) = args.first()
            else {
                return false;
            };
            let fmt = fmt.value();
            let inline = [format!("{{{var}}}"), format!("{{{var}:?}}")];
            let positional = args.len() == 2
                && text(&args[1]) == var
                && (fmt == "{}" || fmt == "{:?}");
            positional || inline.contains(&fmt)
        }
        syn::Expr::Paren(p) => is_stringified(&p.expr, var),
        _ => false,
    }
}

/// The name of a closure's single parameter.
fn closure_param(c: &syn::ExprClosure) -> Option<String> {
    match c.inputs.first()? {
        syn::Pat::Ident(i) if c.inputs.len() == 1 => Some(i.ident.to_string()),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for Conversions<'_> {
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if m.method == "map_err" && m.args.len() == 1 {
            if let syn::Expr::Closure(c) = &m.args[0] {
                if let Some(var) = closure_param(c) {
                    if let Some(variant) = self.stringifying_variant(&c.body, &var) {
                        let start = m.method.span().start();
                        self.hits.push((
                            start.line,
                            start.column + 1,
                            format!("`{variant}` keeps only the text of `{var}`; hold the typed source (`#[from]`/`#[source]`) and let `?` convert it"),
                        ));
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        // `impl From<SomeError> for OurError`
        let source = i.trait_.as_ref().and_then(|(_, path, _)| {
            let last = path.segments.last()?;
            if last.ident != "From" {
                return None;
            }
            let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
                return None;
            };
            match args.args.first()? {
                syn::GenericArgument::Type(t) => syntax::type_ident(t),
                _ => None,
            }
        });
        let target = syntax::type_ident(&i.self_ty).unwrap_or_default();
        let errors_on_both_sides =
            source.is_some_and(|s| s.contains("Error")) && target.contains("Error");
        if errors_on_both_sides
            && !self.exported.contains(&target)
            && !target.ends_with("ApiError")
        {
            for item in &i.items {
                let syn::ImplItem::Fn(f) = item else { continue };
                let Some(syn::FnArg::Typed(arg)) = f.sig.inputs.first() else {
                    continue;
                };
                let var = text(&arg.pat);
                let body = match f.block.stmts.as_slice() {
                    [syn::Stmt::Expr(e, None)] => e,
                    _ => continue,
                };
                // `Self::X(e.to_string())` or `Error::X(..)`
                let named = match body {
                    syn::Expr::Call(c) => matches!(&*c.func, syn::Expr::Path(_)),
                    syn::Expr::Struct(_) => true,
                    _ => false,
                };
                let stringified = named
                    && match body {
                        syn::Expr::Call(c) => c.args.iter().any(|a| is_stringified(a, &var)),
                        syn::Expr::Struct(s) => s.fields.iter().any(|f| is_stringified(&f.expr, &var)),
                        _ => false,
                    };
                if stringified {
                    let start = syn::spanned::Spanned::span(body).start();
                    self.hits.push((
                        start.line,
                        start.column + 1,
                        format!("`From` conversion into `{target}` keeps only the text of `{var}`; store the typed source instead"),
                    ));
                }
            }
        }
        syn::visit::visit_item_impl(self, i);
    }

    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        if !self.exported.contains(&e.ident.to_string()) {
            for variant in &e.variants {
                for field in &variant.fields {
                    let source = syntax::type_string(&field.ty);
                    let from = syntax::has_attr(&field.attrs, "from");
                    let name = variant.ident.to_string();
                    let generic = GENERIC_WORDS.iter().any(|w| name.contains(w));
                    if from && BROAD_SOURCES.contains(&source.as_str()) && !generic {
                        let start = variant.ident.span().start();
                        self.hits.push((
                            start.line,
                            start.column + 1,
                            format!("`#[from] {source}` on `{name}` labels every unrelated {source} as a `{name}` failure; use a generic variant (e.g. `Database`/`Io`) or add context with `map_err`"),
                        ));
                    }
                }
            }
        }
        syn::visit::visit_item_enum(self, e);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/http_cache/store.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[("components/ads-client/Cargo.toml", "[package]\nname = \"ads-client\"\n"), (PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/preserve-error-sources/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 9);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/preserve-error-sources/do.rs")).is_empty());
    }

    #[test]
    fn flags_from_impls_and_operation_named_from() {
        let src = "pub enum Error {\n    AttachmentWrite(#[from] std::io::Error),\n    Io2(#[from] std::io::Error),\n}\nimpl From<serde_json::Error> for Error {\n    fn from(e: serde_json::Error) -> Self {\n        Self::Json(format!(\"{e}\"))\n    }\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 7]);
    }

    #[test]
    fn ignores_added_context_and_public_errors() {
        let src = "#[derive(Debug, thiserror::Error, uniffi::Error)]\npub enum ApiError { Other { reason: String } }\nfn f() -> Result<()> {\n    a().map_err(|e| ApiError::Other { reason: e.to_string() })?;\n    b().map_err(|e| Error::Encrypt(format!(\"{e} (encrypting {id})\")))?;\n    Ok(())\n}\nimpl From<&str> for RequestHash { fn from(s: &str) -> Self { RequestHash(s.to_string()) } }\n";
        assert!(run(src).is_empty());
    }
}
