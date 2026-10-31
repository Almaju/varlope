//! Don't hand-edit generated files or commit unrelated churn
//!
//! Docs and examples: `rules/generated-files-untouched/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::{Diff, FileChange, Status};
use crate::helpers::docs_deps::lock_packages;
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "generated-files-untouched",
    area: Area::Deps,
    title: "Don't hand-edit generated files or commit unrelated churn",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: true,
    check,
};

/// A blanket `cargo update` moves many packages; `cargo update -p x` for a
/// security fix moves one or two.
const STRAY_UPDATE_MIN: usize = 5;

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    hand_edited_summaries(diff, out);
    for change in &diff.files {
        if change.status == Status::Added && change.path.split('/').any(|s| s == "Generated") {
            out.add(
                &change.path,
                0,
                "generated bindings are build output (gitignored): don't commit them",
            );
        }
        let name = change.path.rsplit('/').next().unwrap_or("");
        let empty = change.status == Status::Added && change.new_text.as_deref() == Some("");
        if empty && !matches!(name, "__init__.py" | ".gitkeep" | "py.typed") {
            out.add(
                &change.path,
                0,
                "empty file: remove it, or give it the content it was meant to have",
            );
        }
    }
    stray_lockfile_updates(diff, out);
    ios_churn(diff, out);
}

fn is_summary(path: &str) -> bool {
    path.ends_with("DEPENDENCIES.md") || path.ends_with("dependency-licenses.xml")
}

/// The dependency summaries are generated from Cargo metadata: a change to
/// them with no dependency change is a hand edit the next regeneration undoes.
fn hand_edited_summaries(diff: &Diff, out: &mut Findings) {
    let deps_changed = diff
        .files
        .iter()
        .any(|f| f.path.ends_with("Cargo.lock") || f.path.ends_with("Cargo.toml"));
    if deps_changed {
        return;
    }
    for change in diff.changed(is_summary) {
        let line = change.added.first().map(|(l, _)| *l).unwrap_or(0);
        out.add(
            &change.path,
            line,
            "generated file changed with no Cargo.toml/Cargo.lock change: don't edit it by hand, run tools/regenerate_dependency_summaries.sh",
        );
    }
}

/// Lockfile version changes that none of the PR's Cargo.toml edits reach: a
/// stray `cargo update` riding along with an unrelated change.
fn stray_lockfile_updates(diff: &Diff, out: &mut Findings) {
    let Some(lock) = diff.file("Cargo.lock") else {
        return;
    };
    let (Some(old), Some(new)) = (&lock.old_text, &lock.new_text) else {
        return;
    };
    // A PR that only touches the lockfile (and the files generated from it)
    // is an update on purpose.
    let lock_only = diff
        .files
        .iter()
        .all(|f| f.path.ends_with("Cargo.lock") || is_summary(&f.path) || f.path == "CHANGELOG.md");
    if lock_only {
        return;
    }
    let old = lock_packages(old);
    let new = lock_packages(new);
    let versions = |packages: &[crate::helpers::docs_deps::LockPackage]| {
        let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for p in packages {
            map.entry(p.name.clone())
                .or_default()
                .insert(p.version.clone());
        }
        map
    };
    let (old_versions, new_versions) = (versions(&old), versions(&new));
    let names: BTreeSet<&String> = old_versions.keys().chain(new_versions.keys()).collect();
    let changed: Vec<&String> = names
        .into_iter()
        .filter(|n| old_versions.get(*n) != new_versions.get(*n))
        .collect();
    // Dependencies named on changed Cargo.toml lines, and everything they pull in.
    let mut todo: Vec<String> = diff
        .changed(|p| p.ends_with("Cargo.toml"))
        .flat_map(|c| c.added_text().chain(c.removed_text()).collect::<Vec<_>>())
        .filter_map(|l| {
            let key = l.trim().split(['=', ' ']).next()?.trim().trim_matches('"');
            (!key.is_empty() && !key.starts_with('[') && !key.starts_with('#'))
                .then(|| key.to_string())
        })
        .collect();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    while let Some(name) = todo.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        for p in old.iter().chain(&new).filter(|p| p.name == name) {
            todo.extend(p.dependencies.iter().cloned());
        }
    }
    let stray: Vec<&String> = changed
        .into_iter()
        .filter(|n| !reached.contains(*n) && !reached.contains(&n.replace('-', "_")))
        .collect();
    if stray.len() < STRAY_UPDATE_MIN {
        return;
    }
    let line = new
        .iter()
        .find(|p| p.name == *stray[0])
        .map(|p| p.line)
        .unwrap_or(0);
    let shown: Vec<&str> = stray.iter().take(5).map(|s| s.as_str()).collect();
    out.add(
        "Cargo.lock",
        line,
        format!(
            "{} packages change version that this PR's Cargo.toml edits don't need ({}…), likely a stray `cargo update`: revert them and use `cargo update -p <crate>`",
            stray.len(),
            shown.join(", ")
        ),
    );
}

/// `Package.resolved` and Xcode project churn in PRs that change nothing
/// about the iOS build (Xcode rewrites them when switching branches).
fn ios_churn(diff: &Diff, out: &mut Findings) {
    let is_churn_file = |p: &str| p.ends_with("Package.resolved") || p.ends_with(".pbxproj");
    let ios_config = diff.files.iter().any(|f| {
        let p = f.path.as_str();
        p.ends_with("Package.swift")
            || p.ends_with("libs.versions.toml")
            || p.ends_with(".xcconfig")
            || (p.contains(".xcodeproj/") && !p.ends_with(".pbxproj"))
            || glean_bump(f)
    });
    if ios_config {
        return;
    }
    for change in diff.changed(is_churn_file) {
        let line = change.added.first().map(|(l, _)| *l).unwrap_or(0);
        out.add(
            &change.path,
            line,
            "changed in a PR that doesn't touch the iOS build config (Xcode rewrites it when switching branches): revert it unless the change is intended",
        );
    }
}

/// A Glean version bump in Cargo.lock moves Package.resolved too.
fn glean_bump(change: &FileChange) -> bool {
    change.path.ends_with("Cargo.lock")
        && change
            .new_text
            .as_deref()
            .zip(change.old_text.as_deref())
            .is_some_and(|(new, old)| {
                let glean = |t: &str| {
                    lock_packages(t)
                        .into_iter()
                        .find(|p| p.name == "glean-sym")
                        .map(|p| p.version)
                };
                glean(new) != glean(old)
            })
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check_diff, check_unified};

    #[test]
    fn flags_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/generated-files-untouched/dont.diff"),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(
            (found[0].path.as_str(), found[0].line),
            ("DEPENDENCIES.md", 1204)
        );
    }

    #[test]
    fn accepts_do() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/generated-files-untouched/do.diff"),
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    fn lock(packages: &[(&str, &str, &[&str])]) -> String {
        packages
            .iter()
            .map(|(n, v, deps)| {
                let deps: String = deps.iter().map(|d| format!(" \"{d}\",\n")).collect();
                format!(
                    "[[package]]\nname = \"{n}\"\nversion = \"{v}\"\ndependencies = [\n{deps}]\n\n"
                )
            })
            .collect()
    }

    #[test]
    fn stray_cargo_update() {
        let old = lock(&[
            ("places", "0.1.0", &["mockall", "url"]),
            ("mockall", "0.12.0", &[]),
            ("url", "2.5.0", &[]),
            ("tokio", "1.38.0", &[]),
            ("serde", "1.0.200", &[]),
            ("regex", "1.10.0", &[]),
            ("libc", "0.2.150", &[]),
            ("bytes", "1.5.0", &[]),
        ]);
        let targeted = old.replace(
            "mockall\"\nversion = \"0.12.0",
            "mockall\"\nversion = \"0.13.0",
        );
        let blanket = targeted
            .replace("1.38.0", "1.40.0")
            .replace("1.0.200", "1.0.210")
            .replace("1.10.0", "1.11.0")
            .replace("0.2.150", "0.2.160")
            .replace("1.5.0", "1.6.0");
        let toml_old = "[dev-dependencies]\nmockall = \"0.12\"\n";
        let toml_new = "[dev-dependencies]\nmockall = \"0.13\"\n";
        let src = ("components/places/src/lib.rs", "fn a() {}\n");
        let src_new = ("components/places/src/lib.rs", "fn a() {}\nfn b() {}\n");
        let run = |new_lock: &str| {
            check_diff(
                &RULE,
                &[
                    ("Cargo.lock", &old),
                    ("components/places/Cargo.toml", toml_old),
                    src,
                ],
                &[
                    ("Cargo.lock", new_lock),
                    ("components/places/Cargo.toml", toml_new),
                    src_new,
                ],
            )
        };
        assert!(run(&targeted).is_empty());
        let found = run(&blanket);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.starts_with("5 packages"));
    }

    #[test]
    fn lock_only_updates_are_deliberate() {
        let old = lock(&[
            ("a", "1.0.0", &[]),
            ("b", "1.0.0", &[]),
            ("c", "1.0.0", &[]),
            ("d", "1.0.0", &[]),
            ("e", "1.0.0", &[]),
        ]);
        let new = old.replace("1.0.0", "1.1.0");
        assert!(check_diff(&RULE, &[("Cargo.lock", &old)], &[("Cargo.lock", &new)]).is_empty());
    }

    #[test]
    fn generated_bindings_empty_files_and_xcode_churn() {
        let diff = "diff --git a/components/places/ios/Generated/places.swift b/components/places/ios/Generated/places.swift\nnew file mode 100644\n--- /dev/null\n+++ b/components/places/ios/Generated/places.swift\n@@ -0,0 +1 @@\n+// generated\ndiff --git a/megazords/ios-rust/Package.resolved b/megazords/ios-rust/Package.resolved\n--- a/megazords/ios-rust/Package.resolved\n+++ b/megazords/ios-rust/Package.resolved\n@@ -9 +9 @@\n-        \"version\" : \"70.0.0\"\n+        \"version\" : \"70.1.0\"\n";
        let found = check_unified(&RULE, &[], diff);
        assert_eq!(found.len(), 2, "{found:#?}");
        let found = check_diff(
            &RULE,
            &[],
            &[("components/x/src/empty.rs", ""), ("tools/__init__.py", "")],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
    }
}
