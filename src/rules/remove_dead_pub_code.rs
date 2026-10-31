//! Remove unused pub items, error variants and columns that rustc can't see
//!
//! Docs and examples: `rules/remove-dead-pub-code/`.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::uniffi;

pub static RULE: Rule = Rule {
    id: "remove-dead-pub-code",
    area: Area::DeadCode,
    title: "Remove unused pub items, error variants and columns that rustc can't see",
    kind: Kind::Code,
    clippy: &[
        "clippy::used_underscore_binding",
        "clippy::used_underscore_items",
    ],
    in_tests: false,
    check,
};

/// rustc's `dead_code` lint does not warn about variants of a `pub` enum, so
/// error variants nobody raises survive refactors. Report the variants of
/// error enums that no expression anywhere in the workspace constructs.
fn check(cx: &Context, out: &mut Findings) {
    let exported = exported_names(cx);
    let mut constructed = BTreeSet::new();
    for file in idioms::rust_files(cx.ws) {
        let mut v = Constructions(&mut constructed);
        v.visit_file(&file.ast);
    }
    for file in idioms::rust_files(cx.ws) {
        if file.is_test_file() {
            continue;
        }
        for item in syntax::all_items(&file.ast) {
            let syn::Item::Enum(e) = item else { continue };
            if !is_error_enum(e) || exported.contains(&e.ident.to_string()) {
                continue;
            }
            for variant in &e.variants {
                let name = variant.ident.to_string();
                if constructed.contains(&name) || converted_from(variant) {
                    continue;
                }
                let (line, column) = file.pos(&variant.ident);
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!(
                        "`{}::{name}` is never constructed anywhere in the workspace; remove the variant and any arms that match it",
                        e.ident
                    ),
                );
            }
        }
    }
}

/// An enum used as an error: named `*Error` or deriving `thiserror::Error`.
fn is_error_enum(e: &syn::ItemEnum) -> bool {
    !syntax::is_test_gated(&e.attrs)
        && (e.ident.to_string().ends_with("Error") || syntax::has_derive(&e.attrs, "Error"))
        && !e.attrs.iter().any(|a| a.path().is_ident("non_exhaustive"))
}

/// A variant built by `?` through a `#[from]` conversion.
fn converted_from(variant: &syn::Variant) -> bool {
    variant
        .fields
        .iter()
        .any(|f| syntax::has_attr(&f.attrs, "from"))
}

/// Names of types exported over UniFFI: removing their variants is an API
/// change consumers must hear about, handled by other rules.
fn exported_names(cx: &Context) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for package in cx.ws.packages() {
        for item in uniffi::package_api(cx.ws, package) {
            out.insert(item.name);
        }
    }
    out
}

/// Last segments of every path used as a value (`Error::Io(..)`,
/// `Self::Network { .. }`, `Interrupted`), plus every identifier inside a
/// macro invocation, where expressions and patterns cannot be told apart.
struct Constructions<'s>(&'s mut BTreeSet<String>);

impl<'a> Visit<'a> for Constructions<'_> {
    fn visit_expr_path(&mut self, e: &'a syn::ExprPath) {
        if let Some(last) = e.path.segments.last() {
            self.0.insert(last.ident.to_string());
        }
        syn::visit::visit_expr_path(self, e);
    }

    fn visit_expr_struct(&mut self, e: &'a syn::ExprStruct) {
        if let Some(last) = e.path.segments.last() {
            self.0.insert(last.ident.to_string());
        }
        syn::visit::visit_expr_struct(self, e);
    }

    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        self.0.extend(idioms::idents(&mac.tokens));
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/error.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/remove-dead-pub-code/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 6);
        assert!(found[0].message.contains("`Error::HttpError`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/remove-dead-pub-code/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn counts_constructions_anywhere_and_in_macros() {
        let error = "#[derive(Debug, thiserror::Error)]\npub enum Error {\n\
            #[error(\"a\")] A(String),\n\
            #[error(\"b\")] B { code: u16 },\n\
            #[error(\"c\")] C,\n\
            #[error(\"d\")] D(#[from] std::io::Error),\n\
            }\n";
        let other = "fn f() -> Result<()> { Err(Error::A(\"x\".into())) }\n\
            fn g() -> Error { crate::Error::B { code: 1 } }\n\
            fn h() -> Result<()> { bail!(Error::C) }\n";
        let found = check(
            &RULE,
            &[(PATH, error), ("components/ads-client/src/other.rs", other)],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_exported_and_non_error_enums() {
        let src = "#[derive(Debug, thiserror::Error, uniffi::Error)]\npub enum ApiError {\n    #[error(\"x\")]\n    Network { reason: String },\n}\n\
            pub enum Status { Unused }\n";
        let found = check(
            &RULE,
            &[
                (PATH, src),
                (
                    "components/ads-client/Cargo.toml",
                    "[package]\nname = \"ads-client\"\n",
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
