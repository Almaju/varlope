//! Consumer-visible component changes need a CHANGELOG.md entry.
//!
//! Docs and examples: `rules/changelog-required/`.

use crate::diff::FileChange;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::workspace::{is_component_src, is_test_path};

pub static RULE: Rule = Rule {
    id: "changelog-required",
    area: Area::Docs,
    title: "Consumer-visible component changes need a CHANGELOG.md entry",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    if diff.touches("CHANGELOG.md") {
        return;
    }
    let Some(first) = diff.files.iter().find(|f| consumer_visible(f)) else {
        return;
    };
    let line = first.added.first().map(|(l, _)| *l).unwrap_or(0);
    let count = diff.files.iter().filter(|f| consumer_visible(f)).count();
    out.add(
        &first.path,
        line,
        format!("{count} consumer-visible file(s) changed but CHANGELOG.md was not touched"),
    );
}

/// Code consumers see: component sources, UDL, and the Kotlin/Swift wrappers.
fn consumer_visible(change: &FileChange) -> bool {
    let p = change.path.as_str();
    if is_test_path(p) {
        return false;
    }
    let real_change = change
        .added_text()
        .chain(change.removed_text())
        .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"));
    real_change
        && (is_component_src(p)
            || (p.starts_with("components/") && p.ends_with(".udl"))
            || (p.starts_with("components/") && p.contains("/android/src/main/"))
            || (p.starts_with("components/") && p.contains("/ios/") && p.ends_with(".swift")))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check_unified;

    #[test]
    fn flags_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/changelog-required/dont.diff"),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "components/places/src/places.udl");
    }

    #[test]
    fn accepts_do() {
        assert!(check_unified(
            &RULE,
            &[],
            include_str!("../../rules/changelog-required/do.diff")
        )
        .is_empty());
    }

    #[test]
    fn ignores_test_only_changes() {
        let diff = "--- a/components/places/src/tests.rs\n+++ b/components/places/src/tests.rs\n+fn t() {}\n";
        assert!(check_unified(&RULE, &[], diff).is_empty());
    }
}
