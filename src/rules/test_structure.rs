//! Focused, descriptively named tests; shared fixtures via helpers
//!
//! Docs and examples: `rules/test-structure/`.

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "test-structure",
    area: Area::Tests,
    title: "Focused, descriptively named tests; shared fixtures via helpers",
    kind: Kind::Review,
    clippy: &[],
    in_tests: true,
    check,
};

/// A test with this many assertions checks several behaviours at once (a
/// single scenario checked step by step rarely needs half as many).
const MAX_ASSERTS: usize = 50;

/// Names that say a test exists, not what it checks.
const VAGUE: &[&str] = &[
    "", "basic", "basics", "simple", "smoke", "works", "it_works", "misc", "all", "stuff",
    "everything", "sanity", "general", "example", "things", "various", "other", "tmp",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        if !file.path.starts_with("components/") {
            continue;
        }
        for f in functions(&file.ast) {
            if !is_test_fn(f.attrs) {
                continue;
            }
            let (line, column) = file.pos(&f.sig.ident);
            if vague(&f.name) {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("test name `{}` does not say what it checks; name it after the behaviour under test", f.name),
                );
            }
            let mut asserts = 0;
            idioms::macros_in(f.block, |mac| {
                if syntax::macro_name(mac).starts_with("assert") {
                    asserts += 1;
                }
            });
            if asserts >= MAX_ASSERTS {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("`{}` makes {asserts} assertions, so it tests many behaviours at once; split new cases into focused tests that share setup through a helper", f.name),
                );
            }
        }
    }
}

/// `#[test]`, `#[tokio::test]`, `#[rstest]`: a test, not test-only code.
fn is_test_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "test" || s.ident == "rstest")
    })
}

/// `test`, `test_basic`, `smoke_test`, `test2`.
fn vague(name: &str) -> bool {
    let mut core = name.strip_prefix("test").unwrap_or(name);
    core = core.trim_start_matches('_');
    core = core.strip_suffix("test").unwrap_or(core).trim_end_matches('_');
    let core = core.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
    VAGUE.contains(&core)
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/tests.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/test-structure/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 5);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/test-structure/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_catch_all_tests() {
        let asserts = "assert_eq!(store.fetch(q), vec![]);\n".repeat(50);
        let src = format!("#[test]\nfn query_amp_and_wikipedia() {{\n{asserts}}}\n#[cfg(test)]\nfn new() {{}}\n");
        let found = check(&RULE, &[(PATH, &src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("50 assertions"));
    }

    #[test]
    fn names() {
        for name in ["test", "test_basic", "smoke_test", "test2", "test_simple_1", "basic"] {
            assert!(super::vague(name), "{name}");
        }
        for name in ["test_basic_query_matches_prefix", "new_tab_is_synced", "testing_mode_off"] {
            assert!(!super::vague(name), "{name}");
        }
    }
}
