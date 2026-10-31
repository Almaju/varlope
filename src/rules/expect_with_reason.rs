//! No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`
//!
//! Docs and examples: `rules/expect-with-reason/`.

use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "expect-with-reason",
    area: Area::DeadCode,
    title: "No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`",
    kind: Kind::Code,
    clippy: &["clippy::allow_attributes_without_reason"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws).filter(|f| !idioms::is_tooling(&f.path)) {
        let mut visitor = Attrs { hits: Vec::new() };
        visitor.visit_file(&file.ast);
        for (attr, message) in visitor.hits {
            let (line, column) = file.pos(attr);
            out.add_at(&file.path, line, column, message);
        }
    }
}

struct Attrs<'a> {
    hits: Vec<(&'a syn::Attribute, String)>,
}

impl<'a> Visit<'a> for Attrs<'a> {
    fn visit_attribute(&mut self, attr: &'a syn::Attribute) {
        let name = if attr.path().is_ident("allow") {
            "allow"
        } else if attr.path().is_ident("expect") {
            "expect"
        } else {
            return;
        };
        let mut lints = Vec::new();
        let mut reason = false;
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("reason") {
                reason = true;
                let _ = meta.value()?.parse::<syn::Expr>()?;
            } else {
                lints.push(crate::syntax::path_string(&meta.path));
            }
            Ok(())
        });
        if parsed.is_err() || reason {
            return;
        }
        let bang = if matches!(attr.style, syn::AttrStyle::Inner(_)) {
            "!"
        } else {
            ""
        };
        if name == "expect" {
            self.hits.push((
                attr,
                format!(
                    "`#{bang}[expect({})]` has no `reason = \"…\"`; say why the lint is expected here",
                    lints.join(", ")
                ),
            ));
            return;
        }
        let Some(lint) = lints.iter().find(|l| silences_unused(l)) else {
            return;
        };
        self.hits.push((
            attr,
            format!(
                "`#{bang}[allow({lint})]` hides unused code for good; delete it, gate it with `#[cfg(..)]`, or write `#[expect({lint}, reason = \"…\")]`"
            ),
        ));
    }
}

/// `dead_code`, `unused` and the `unused_*` lints.
fn silences_unused(lint: &str) -> bool {
    lint == "dead_code" || lint == "unused" || lint.starts_with("unused_")
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/http_cache/request_hash.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/expect-with-reason/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/expect-with-reason/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_inner_allows_and_expect_without_reason() {
        let src = "#![allow(unused_imports)]\n#[expect(clippy::too_many_arguments)]\nfn f() {}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("#![allow(unused_imports)]"));
        assert!(found[1].message.contains("no `reason"));
    }

    #[test]
    fn ignores_other_lints_reasons_conditional_allows_and_tests() {
        let src = "#[allow(clippy::large_enum_variant)]\nenum E { A }\n\
                   #[allow(dead_code, reason = \"read by the FFI\")]\nstruct S;\n\
                   #[cfg_attr(not(feature = \"x\"), allow(dead_code))]\nfn g() {}\n\
                   #[cfg(test)]\nmod tests {\n#[allow(dead_code)]\nfn h() {}\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }
}
