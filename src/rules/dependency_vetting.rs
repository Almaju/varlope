//! New/bumped crates must be vetted and compatible with mozilla-central
//!
//! Docs and examples: `rules/dependency-vetting/`.

use crate::diff::{FileChange, Status};
use crate::helpers::docs_deps::{hash_comment_start, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "dependency-vetting",
    area: Area::Deps,
    title: "Dependencies must vendor cleanly into mozilla-central: no inherited deps, no test-only crates in shipping code, explained audit ignores",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

/// Crates for tests, benchmarks and command-line tools, which mozilla-central
/// would otherwise have to vendor and vet as shipping code.
const DEV_ONLY: &[&str] = &[
    "clap",
    "cli-support",
    "criterion",
    "dotenvy",
    "env_logger",
    "expect-test",
    "httpmock",
    "indicatif",
    "insta",
    "mockall",
    "mockito",
    "pretty_assertions",
    "proptest",
    "rstest",
    "serial_test",
    "structopt",
    "tempfile",
    "tokio-test",
    "viaduct-dev",
    "wiremock",
];

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    for change in &diff.files {
        if change.status == Status::Deleted {
            continue;
        }
        if change.path.ends_with("Cargo.toml") && vendored(&change.path) {
            let text = change
                .new_text
                .clone()
                .or_else(|| cx.ws.file(&change.path).map(|f| f.text.clone()));
            manifest(change, text.as_deref(), out);
        }
        let ext = change.path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if matches!(ext, "yml" | "yaml" | "sh" | "toml" | "py") && !is_out_of_scope(&change.path) {
            audit_ignores(change, out);
        }
    }
}

/// Crates mozilla-central vendors: components and megazords, not tools,
/// examples, benches or test harnesses.
fn vendored(manifest: &str) -> bool {
    (manifest.starts_with("components/") || manifest.starts_with("megazords/"))
        && !manifest.split('/').any(|s| {
            s.contains("bench")
                || s == "cli"
                || s.ends_with("-cli")
                || s.ends_with("_cli")
                || s.contains("integration-tests")
                || s == "tests"
        })
}

/// The `[section]` a 1-based line of a manifest sits in.
fn section_at(text: &str, line: usize) -> String {
    text.lines()
        .take(line)
        .filter_map(|l| {
            let t = l.trim();
            (t.starts_with('[') && !t.starts_with("[["))
                .then(|| t.trim_matches(['[', ']']).to_string())
        })
        .last()
        .unwrap_or_default()
}

fn manifest(change: &FileChange, text: Option<&str>, out: &mut Findings) {
    for (line, added) in &change.added {
        let code = match hash_comment_start(added) {
            Some(at) => &added[..at],
            None => added.as_str(),
        };
        let Some((name, value)) = code.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"');
        let compact: String = value.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains("workspace=true") {
            out.add(
                &change.path,
                *line,
                format!("`{name}` inherits from the workspace, which mozilla-central can't resolve when it vendors this crate: give the version here"),
            );
            continue;
        }
        let section = text.map(|t| section_at(t, *line)).unwrap_or_default();
        let shipping = section == "dependencies"
            || section.ends_with(".dependencies") && !section.contains("dev-");
        if shipping && DEV_ONLY.contains(&name) && !compact.contains("optional=true") {
            out.add(
                &change.path,
                *line,
                format!("`{name}` is a test/CLI crate in shipping `[dependencies]`, so mozilla-central vendors and vets it: move it to `[dev-dependencies]` or behind an optional feature"),
            );
        }
    }
}

/// `cargo audit --ignore RUSTSEC-…` or an `audit.toml` ignore with no
/// comment saying why it is safe and when it goes away.
fn audit_ignores(change: &FileChange, out: &mut Findings) {
    let lines: Vec<&str> = match &change.new_text {
        Some(t) => t.lines().collect(),
        None => Vec::new(),
    };
    for (line, added) in &change.added {
        let Some(at) = added.find("RUSTSEC-") else {
            continue;
        };
        let is_ignore = added.contains("--ignore") || change.path.ends_with("audit.toml");
        if !is_ignore || hash_comment_start(added).is_some_and(|c| c < at) {
            continue; // not an ignore, or the id is inside a comment
        }
        let explained_inline = hash_comment_start(added).is_some_and(|c| c > at);
        // A comment block right above (or a few lines above) mentioning the id.
        let id: String = added[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if id.len() < "RUSTSEC-2000-0000".len() {
            continue;
        }
        let explained_above = (line.saturating_sub(15)..*line)
            .filter_map(|l| l.checked_sub(1).and_then(|i| lines.get(i)))
            .any(|l| l.trim_start().starts_with('#') && l.contains(&id));
        let explained_in_diff = change
            .added
            .iter()
            .any(|(l, t)| l < line && t.trim_start().starts_with('#') && t.contains(&id));
        if !(explained_inline || explained_above || explained_in_diff) {
            out.add(
                &change.path,
                *line,
                format!("ignores `{id}` without saying why: add a comment with the reason it is safe here and the bug that removes the ignore"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::docs_deps::old_and_new_from_diff;
    use crate::rule::Finding;
    use crate::testing::{check_diff, check_unified};

    fn run(diff: &str) -> Vec<Finding> {
        let (old, new) = old_and_new_from_diff(diff);
        let old: Vec<(&str, &str)> = old.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let new: Vec<(&str, &str)> = new.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        check_diff(&RULE, &old, &new)
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/dependency-vetting/dont.diff"));
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![6, 7], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = run(include_str!("../../rules/dependency-vetting/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn tools_and_optional_deps_are_fine() {
        let toml = "[package]\nname = \"x\"\n\n[dependencies]\nclap = { version = \"4\", optional = true }\n";
        let old = toml.replace("clap = { version = \"4\", optional = true }\n", "");
        assert!(check_diff(
            &RULE,
            &[("components/filter_adult/Cargo.toml", &old)],
            &[("components/filter_adult/Cargo.toml", toml)]
        )
        .is_empty());
        let tool = "[package]\nname = \"nimbus-cli\"\n\n[dependencies]\nclap = \"4\"\n";
        let old = tool.replace("clap = \"4\"\n", "");
        assert!(check_diff(
            &RULE,
            &[("components/support/nimbus-cli/Cargo.toml", &old)],
            &[("components/support/nimbus-cli/Cargo.toml", tool)]
        )
        .is_empty());
    }

    #[test]
    fn audit_ignores() {
        let diff = "--- a/.github/workflows/dependency-check.yaml\n+++ b/.github/workflows/dependency-check.yaml\n@@ -60 +60,4 @@\n-            cargo audit\n+            # RUSTSEC-2026-0258: h2 in viaduct-hyper, only talks to Mozilla servers; bug 2067851 removes it.\n+            cargo audit --ignore RUSTSEC-2026-0258 \\\n+              --ignore RUSTSEC-2018-0006\n";
        let found = check_unified(&RULE, &[], diff);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("RUSTSEC-2018-0006"));
    }
}
