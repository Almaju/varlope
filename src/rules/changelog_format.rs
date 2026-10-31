//! CHANGELOG edits go in the in-progress section, under one `### <Component>` heading
//!
//! Docs and examples: `rules/changelog-format/`.

use std::collections::BTreeSet;

use crate::diff::{Diff, FileChange};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "changelog-format",
    area: Area::Docs,
    title: "CHANGELOG edits go in the in-progress section, under one `### <Component>` heading",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

const CHANGELOG: &str = "CHANGELOG.md";

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    check_version_txt(cx, diff, out);
    let Some(change) = diff.file(CHANGELOG) else {
        return;
    };
    let Some(new_text) = change
        .new_text
        .clone()
        .or_else(|| cx.ws.file(CHANGELOG).map(|f| f.text.clone()))
    else {
        return;
    };
    let new = Outline::parse(&new_text);
    // Versions already released before this change: their sections are history.
    let released: BTreeSet<String> = match &change.old_text {
        Some(old) => Outline::parse(old).released(),
        None => new.released(),
    };
    history_edits(change, &new, &released, out);
    structure(change, &new, &released, out);
}

/// Added or removed lines in sections that were already released, reported
/// once per change. Moving a line out of a released section into the
/// in-progress one is a fix, not an edit.
fn history_edits(
    change: &FileChange,
    new: &Outline,
    released: &BTreeSet<String>,
    out: &mut Findings,
) {
    let old = change.old_text.as_deref().map(Outline::parse);
    let moved_in: BTreeSet<&str> = change
        .added
        .iter()
        .filter(|(l, _)| new.version_at(*l).is_some_and(|v| !released.contains(v)))
        .map(|(_, t)| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let mut versions: Vec<String> = Vec::new();
    let mut count = 0;
    let mut first_line: Option<usize> = None;
    let mut touch = |v: &str, line: Option<usize>| {
        if !versions.iter().any(|x| x == v) {
            versions.push(v.to_string());
        }
        count += 1;
        if first_line.is_none() {
            first_line = line;
        }
    };
    for (line, text) in &change.added {
        if text.trim().is_empty() {
            continue;
        }
        if let Some(v) = new.version_at(*line).filter(|v| released.contains(*v)) {
            touch(v, Some(*line));
        }
    }
    if let Some(old) = &old {
        for (line, text) in &change.removed {
            let text = text.trim();
            let moved = moved_in
                .iter()
                .any(|added| added.starts_with(text) || text.starts_with(added));
            if text.is_empty() || moved {
                continue;
            }
            if let Some(v) = old.version_at(*line).filter(|v| released.contains(*v)) {
                let header = new.versions.iter().find(|x| x.name == v).map(|x| x.line);
                touch(v, header);
            }
        }
    }
    if versions.is_empty() {
        return;
    }
    let current = new
        .versions
        .iter()
        .find(|v| !v.released)
        .map(|v| format!(" under `v{} (In progress)`", v.name))
        .unwrap_or_else(|| " in the in-progress section".to_string());
    let mut names: Vec<String> = versions.iter().take(3).map(|v| format!("`v{v}`")).collect();
    if versions.len() > 3 {
        names.push(format!("{} more", versions.len() - 3));
    }
    out.add(
        CHANGELOG,
        first_line.unwrap_or(0),
        format!(
            "changes {count} line(s) of released section(s) {}: released sections are history; new entries go{current}",
            names.join(", ")
        ),
    );
}

/// Headings and entries added to unreleased sections.
fn structure(change: &FileChange, new: &Outline, released: &BTreeSet<String>, out: &mut Findings) {
    let added: BTreeSet<usize> = change.added.iter().map(|(l, _)| *l).collect();
    let lines: Vec<&str> = new.lines.iter().map(|l| l.text.as_str()).collect();
    for &line in &added {
        let Some(info) = new.lines.get(line - 1) else {
            continue;
        };
        if info
            .version
            .is_none_or(|v| released.contains(&new.versions[v].name))
        {
            continue;
        }
        let text = info.text.as_str();
        if is_entry(text) && (info.level == 1 || info.level == 2 && is_section(&info.heading)) {
            out.add(
                CHANGELOG,
                line,
                "entry is not under a `### <Component>` heading: add it below its component's heading",
            );
        } else if let Some(name) = text.strip_prefix("### ") {
            let dupe = new.lines.iter().enumerate().any(|(i, other)| {
                i + 1 != line
                    && other.version == info.version
                    && other.section == info.section
                    && other.text.strip_prefix("### ").map(normalize) == Some(normalize(name))
            });
            if dupe {
                out.add(
                    CHANGELOG,
                    line,
                    format!("`### {}` already exists in this section: add the entry under the existing heading", name.trim()),
                );
            }
        } else if let Some(name) = text.strip_prefix("## ") {
            let dupe = new.lines.iter().enumerate().any(|(i, other)| {
                i + 1 != line
                    && other.version == info.version
                    && other.text.strip_prefix("## ").map(normalize) == Some(normalize(name))
            });
            if !is_section(name) {
                out.add(
                    CHANGELOG,
                    line,
                    format!("`## {}` looks like a component: components are `###` headings under a `## What's New` / `## What's Changed` section", name.trim()),
                );
            } else if dupe {
                out.add(
                    CHANGELOG,
                    line,
                    format!(
                        "`## {}` appears twice in this version: merge the two sections",
                        name.trim()
                    ),
                );
            }
        } else if text.trim().is_empty() {
            let prev_blank = line >= 2 && lines[line - 2].trim().is_empty();
            let prev_added = added.contains(&(line - 1));
            if prev_blank && !(prev_added && line >= 3 && lines[line - 3].trim().is_empty()) {
                out.add(
                    CHANGELOG,
                    line,
                    "extra blank line: keep one blank line between blocks",
                );
            }
        }
    }
}

/// `version.txt` and the newest CHANGELOG header move together.
fn check_version_txt(cx: &Context, diff: &Diff, out: &mut Findings) {
    let changelog = diff.file(CHANGELOG);
    let version_change = diff.file("version.txt");
    let opened = changelog.and_then(|c| {
        let new = Outline::parse(c.new_text.as_deref()?);
        let old = Outline::parse(c.old_text.as_deref()?);
        let first = new.versions.first()?;
        let existed = old.versions.iter().any(|v| v.name == first.name);
        (!existed && !first.released).then(|| first.clone())
    });
    match (version_change, opened) {
        (Some(v), None) => {
            let Some(new_major) = v.new_text.as_deref().and_then(major) else {
                return;
            };
            if v.old_text.as_deref().and_then(major) == Some(new_major) {
                return;
            }
            let newest = changelog
                .and_then(|c| c.new_text.clone())
                .or_else(|| cx.ws.file(CHANGELOG).map(|f| f.text.clone()))
                .and_then(|t| Outline::parse(&t).versions.first().map(|v| v.name.clone()));
            if newest.as_deref().and_then(major) != Some(new_major) {
                out.add(
                    "version.txt",
                    1,
                    format!("version.txt moves to {new_major} but CHANGELOG.md has no `# v{new_major}.0 (In progress)` section: open it in the same PR"),
                );
            }
        }
        (None, Some(header)) => out.add(
            CHANGELOG,
            header.line,
            format!(
                "opens `v{}` but version.txt is unchanged: bump it in the same PR",
                header.name
            ),
        ),
        _ => {}
    }
}

/// The major version of `161.0a1` or `161.0`.
fn major(text: &str) -> Option<u32> {
    let t = text.trim().trim_start_matches('v');
    t.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

fn is_entry(text: &str) -> bool {
    text.starts_with("- ") || text.starts_with("* ")
}

/// Section headings (`## ✨ What's New ✨`), as opposed to component names.
fn is_section(name: &str) -> bool {
    let n = normalize(name);
    [
        "breaking",
        "whats",
        "new",
        "changed",
        "fixed",
        "deprecat",
        "removed",
        "security",
        "fullchangelog",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// Lowercase letters and digits only: `## ⚠️ Breaking Changes ⚠️` → `breakingchanges`.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, Debug)]
struct Version {
    name: String,
    line: usize,
    released: bool,
}

#[derive(Debug)]
struct LineInfo {
    text: String,
    /// Index into `Outline::versions`.
    version: Option<usize>,
    /// Line of the `##` heading above, in this version.
    section: Option<usize>,
    /// Level of the nearest heading above (1 for `#`).
    level: usize,
    /// Text of the nearest heading above, without the `#`s.
    heading: String,
}

/// Where each line of a CHANGELOG sits: version, section, heading level.
struct Outline {
    versions: Vec<Version>,
    lines: Vec<LineInfo>,
}

impl Outline {
    fn parse(text: &str) -> Self {
        let mut versions = Vec::new();
        let mut lines = Vec::new();
        let (mut version, mut section, mut level) = (None, None, 0);
        let mut heading_text = String::new();
        let mut in_code = false;
        for (index, text) in text.lines().enumerate() {
            if text.trim_start().starts_with("```") {
                in_code = !in_code;
            }
            let heading = if in_code {
                0
            } else {
                text.chars().take_while(|c| *c == '#').count()
            };
            if heading > 0 && text[heading..].starts_with(' ') {
                level = heading;
                heading_text = text[heading..].trim().to_string();
                if heading == 1 {
                    section = None;
                    version = text[1..].trim().strip_prefix('v').map(|rest| {
                        let name = rest.split_whitespace().next().unwrap_or("").to_string();
                        let paren = rest.split_once('(').map(|(_, p)| p).unwrap_or("");
                        versions.push(Version {
                            name,
                            line: index + 1,
                            released: paren.chars().filter(char::is_ascii_digit).count() >= 8,
                        });
                        versions.len() - 1
                    });
                } else if heading == 2 {
                    section = Some(index + 1);
                }
            }
            lines.push(LineInfo {
                text: text.to_string(),
                version,
                section,
                level,
                heading: heading_text.clone(),
            });
        }
        Self { versions, lines }
    }

    fn released(&self) -> BTreeSet<String> {
        self.versions
            .iter()
            .filter(|v| v.released)
            .map(|v| v.name.clone())
            .collect()
    }

    fn version_at(&self, line: usize) -> Option<&str> {
        let info = self.lines.get(line.checked_sub(1)?)?;
        info.version.map(|v| self.versions[v].name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::docs_deps::old_and_new_from_diff;
    use crate::rule::Finding;
    use crate::testing::check_diff;

    fn run(diff: &str) -> Vec<Finding> {
        let (old, new) = old_and_new_from_diff(diff);
        let old: Vec<(&str, &str)> = old.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let new: Vec<(&str, &str)> = new.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        check_diff(&RULE, &old, &new)
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/changelog-format/dont.diff"));
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("`### <Component>`"));
        assert!(found[1].message.contains("released section(s) `v160.0`"));
    }

    #[test]
    fn accepts_do() {
        let found = run(include_str!("../../rules/changelog-format/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    const BASE: &str = "# v161.0 (In progress)\n\n## What's Changed\n\n### Autofill\n\n- a ([#1](x))\n\n# v160.0 (_2026-10-22_)\n\n## What's Changed\n\n### Search\n\n- b\n\n[Full Changelog](In progress)\n";

    #[test]
    fn release_prs_may_date_the_in_progress_section() {
        let old = "# v160.0 (In progress)\n\n## What's Changed\n\n### Search\n\n- b\n\n[Full Changelog](In progress)\n\n# v159.0 (_2026-10-08_)\n\n- old\n";
        let new = "# v161.0 (In progress)\n\n[Full Changelog](In progress)\n\n# v160.0 (_2026-10-22_)\n\n## What's Changed\n\n### Search\n\n- b\n\n[Full Changelog](https://x/compare/v159.0...v160.0)\n\n# v159.0 (_2026-10-08_)\n\n- old\n";
        let found = check_diff(
            &RULE,
            &[("CHANGELOG.md", old), ("version.txt", "160.0a1\n")],
            &[("CHANGELOG.md", new), ("version.txt", "161.0a1\n")],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn duplicate_headings_and_component_sections() {
        let new = BASE.replace(
            "- a ([#1](x))\n",
            "- a ([#1](x))\n\n### Autofill\n\n- c\n\n## Nimbus\n\n### Nimbus\n\n- d\n",
        );
        let found = check_diff(&RULE, &[("CHANGELOG.md", BASE)], &[("CHANGELOG.md", &new)]);
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 2, "{messages:#?}");
        assert!(messages[0].contains("already exists"));
        assert!(messages[1].contains("looks like a component"));
    }

    #[test]
    fn same_component_under_two_sections_is_fine() {
        let new = BASE.replace(
            "## What's Changed\n\n### Autofill",
            "## Breaking Changes\n\n### Autofill\n\n- e\n\n## What's Changed\n\n### Autofill",
        );
        let found = check_diff(&RULE, &[("CHANGELOG.md", BASE)], &[("CHANGELOG.md", &new)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn extra_blank_lines() {
        let new = BASE.replace("- a ([#1](x))\n", "- a ([#1](x))\n\n");
        let found = check_diff(&RULE, &[("CHANGELOG.md", BASE)], &[("CHANGELOG.md", &new)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("blank line"));
    }

    #[test]
    fn version_txt_without_a_new_section() {
        let found = check_diff(
            &RULE,
            &[("CHANGELOG.md", BASE), ("version.txt", "161.0a1\n")],
            &[("CHANGELOG.md", BASE), ("version.txt", "162.0a1\n")],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, "version.txt");
    }
}
