//! No wildcard arms on domain enums; destructure fully in merge/hash/eq impls
//!
//! Docs and examples: `rules/exhaustive-matches/`.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "exhaustive-matches",
    area: Area::Idioms,
    title: "No wildcard arms on domain enums; destructure fully in merge/hash/eq impls",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::wildcard_enum_match_arm",
        "clippy::match_wildcard_for_single_variants",
    ],
    in_tests: false,
    check,
};

/// Clippy's lints cover wildcard arms (in modules that opt in). varlope adds
/// the field-by-field half: a `merge` that never reads one of the fields of
/// the value it merges in, which is how new fields get silently dropped.
/// (Manual `Hash`/`PartialEq` impls often skip fields on purpose, so they
/// are left to review.)
fn check(cx: &Context, out: &mut Findings) {
    // Named-field structs per crate directory: name -> fields.
    let mut structs: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for file in idioms::rust_files(cx.ws) {
        let krate = crate_dir(cx, &file.path);
        for item in syntax::all_items(&file.ast) {
            if let syn::Item::Struct(s) = item {
                if let syn::Fields::Named(named) = &s.fields {
                    let fields = named
                        .named
                        .iter()
                        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
                        .collect();
                    structs.insert((krate.clone(), s.ident.to_string()), fields);
                }
            }
        }
    }
    for file in idioms::rust_files(cx.ws) {
        let krate = crate_dir(cx, &file.path);
        for f in functions(&file.ast) {
            if !f.name.starts_with("merge") || f.trait_name.is_some() {
                continue;
            }
            for input in &f.sig.inputs {
                let syn::FnArg::Typed(arg) = input else { continue };
                let syn::Pat::Ident(param) = &*arg.pat else { continue };
                let Some(mut ty) = syntax::type_ident(&arg.ty) else { continue };
                if ty == "Self" {
                    match &f.self_ty {
                        Some(s) => ty = s.clone(),
                        None => continue,
                    }
                }
                let Some(fields) = structs.get(&(krate.clone(), ty.clone())) else {
                    continue;
                };
                let param = param.ident.to_string();
                let mut reads = Reads {
                    param: &param,
                    fields: BTreeSet::new(),
                    whole: false,
                };
                reads.visit_block(f.block);
                if reads.whole {
                    continue;
                }
                let missing: Vec<&String> =
                    fields.iter().filter(|f| !reads.fields.contains(*f)).collect();
                // Only partial merges: one that reads nothing is not field-by-field.
                if missing.is_empty() || reads.fields.is_empty() {
                    continue;
                }
                let (line, column) = file.pos(&f.sig.ident);
                let list = missing
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("`{}` reads `{param}: {ty}` field by field and never reads {list}; destructure it without `..` (`{}: _` for fields skipped on purpose) so a new field fails to compile here", f.name, missing[0]),
                );
            }
        }
    }
}

fn crate_dir(cx: &Context, path: &str) -> String {
    cx.ws
        .package_of(path)
        .map(|p| p.dir.clone())
        .unwrap_or_default()
}

/// Fields of `param` the body reads (`param.field`), and whether it uses
/// `param` as a whole in a way that may read every field (destructuring
/// without `..`, passing it on, cloning it).
struct Reads<'p> {
    param: &'p str,
    fields: BTreeSet<String>,
    whole: bool,
}

impl<'a> Visit<'a> for Reads<'_> {
    fn visit_expr(&mut self, e: &'a syn::Expr) {
        match e {
            syn::Expr::Field(field) if is_param(&field.base, self.param) => {
                if let syn::Member::Named(name) = &field.member {
                    self.fields.insert(name.to_string());
                }
                return;
            }
            syn::Expr::Path(p) if p.path.is_ident(self.param) => {
                // Used other than through a field: passed on, matched, cloned.
                self.whole = true;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, e);
    }

    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        if idioms::count_ident(&mac.tokens, self.param) > 0 {
            self.whole = true;
        }
    }

    fn visit_local(&mut self, local: &'a syn::Local) {
        // `let T { a, b, .. } = param;`
        if let (syn::Pat::Struct(ps), Some(init)) = (&local.pat, &local.init) {
            if is_param(&init.expr, self.param) {
                for field in &ps.fields {
                    if let syn::Member::Named(name) = &field.member {
                        self.fields.insert(name.to_string());
                    }
                }
                if ps.rest.is_none() {
                    self.whole = true;
                }
                if let Some((_, diverge)) = &init.diverge {
                    self.visit_expr(diverge);
                }
                return;
            }
        }
        syn::visit::visit_local(self, local);
    }
}

/// `param`, `&param`, `*param`.
fn is_param(expr: &syn::Expr, param: &str) -> bool {
    match expr {
        syn::Expr::Path(p) => p.path.is_ident(param),
        syn::Expr::Reference(r) => is_param(&r.expr, param),
        syn::Expr::Unary(u) => is_param(&u.expr, param),
        syn::Expr::Paren(p) => is_param(&p.expr, param),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/search/src/filter.rs";
    const TYPES: (&str, &str) = (
        "components/search/src/configuration_types.rs",
        "pub struct JSONEngineUrl {\n    pub base: Option<String>,\n    pub params: Option<Vec<SearchUrlParam>>,\n    pub is_new_until: Option<String>,\n}\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[TYPES, (PATH, include_str!("../../rules/exhaustive-matches/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 2);
        assert!(found[0].message.contains("never reads `is_new_until`"), "{found:#?}");
        assert!(found[0].message.contains("`is_new_until: _`"), "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[TYPES, (PATH, include_str!("../../rules/exhaustive-matches/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn accepts_merges_that_use_the_whole_value() {
        let src = "impl SearchEngineUrl {\n\
            fn merge(&mut self, preferred: &JSONEngineUrl) { self.base = preferred.base.clone(); self.log(preferred); }\n\
            fn merge_all(&mut self, preferred: &JSONEngineUrl) { let JSONEngineUrl { base, .. } = preferred; let _ = base; }\n\
        }\n";
        let found = check(&RULE, &[TYPES, (PATH, src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.starts_with("`merge_all`"));
    }
}
