//! Replace boolean flags/positional tuples with enums or named structs
//!
//! Docs and examples: `rules/no-bool-params/`.

use syn::visit::Visit;

use crate::helpers::api_mobile::is_production_rust;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "no-bool-params",
    area: Area::Api,
    title: "Replace boolean flags/positional tuples with enums or named structs",
    kind: Kind::Clippy,
    // `fn_params_excessive_bools` is not listed: with clippy's default
    // `max-fn-params-bools = 3` it only fires at four `bool`s, and the native
    // check below already reports two or more.
    clippy: &["clippy::struct_excessive_bools"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if !is_production_rust(&file.path) {
            continue;
        }
        let mut v = BoolParams { hits: Vec::new() };
        v.visit_file(&file.ast);
        for (line, name, count) in v.hits {
            out.add(
                &file.path,
                line,
                format!(
                    "`{name}` takes {count} `bool` parameters, so call sites read like `{name}(true, false)`; take an enum or a struct with named fields"
                ),
            );
        }
    }
}

struct BoolParams {
    hits: Vec<(usize, String, usize)>,
}

impl BoolParams {
    fn sig(&mut self, sig: &syn::Signature, attrs: &[syn::Attribute]) {
        let count = sig
            .inputs
            .iter()
            .filter(|a| matches!(a, syn::FnArg::Typed(t) if syntax::type_string(&t.ty) == "bool"))
            .count();
        let allowed = attrs.iter().any(|a| {
            let name = syntax::attr_name(a);
            (name == "allow" || name == "expect")
                && syntax::attr_args(a).contains("fn_params_excessive_bools")
        });
        if count >= 2 && !allowed && sig.abi.is_none() {
            self.hits
                .push((sig.ident.span().start().line, sig.ident.to_string(), count));
        }
    }
}

impl<'ast> Visit<'ast> for BoolParams {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.sig(&f.sig, &f.attrs);
        syn::visit::visit_item_fn(self, f);
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if i.trait_.is_some() {
            return; // the trait fixes the signature
        }
        for item in &i.items {
            if let syn::ImplItem::Fn(f) = item {
                self.sig(&f.sig, &f.attrs);
                syn::visit::visit_block(self, &f.block);
            }
        }
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.sig(&f.sig, &f.attrs);
        syn::visit::visit_trait_item_fn(self, f);
    }
    fn visit_item_foreign_mod(&mut self, _: &'ast syn::ItemForeignMod) {}
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/stateful/nimbus_client.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/no-bool-params/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/no-bool-params/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_trait_impls_ffi_one_bool_and_allowed() {
        let src = r#"
impl Engine for E { fn apply(&self, a: bool, b: bool) {} }
extern "C" fn cb(a: bool, b: bool) {}
fn one(a: bool, n: Option<bool>) {}
#[expect(clippy::fn_params_excessive_bools, reason = "mirrors the server API")]
fn allowed(a: bool, b: bool) {}
#[cfg(test)]
fn helper(a: bool, b: bool) {}
"#;
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }

    #[test]
    fn flags_trait_definitions() {
        let src = "pub trait Engine {\n    fn sync(&self, full: bool, force: bool);\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }
}
