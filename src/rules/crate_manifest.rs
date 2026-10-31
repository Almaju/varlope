//! Crate manifests: `license = "MPL-2.0"`, `exclude` platform dirs, no deps on `examples/`
//!
//! Docs and examples: `rules/crate-manifest/`.

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::workspace::Package;

pub static RULE: Rule = Rule {
    id: "crate-manifest",
    area: Area::Deps,
    title:
        "Crate manifests: `license = \"MPL-2.0\"`, `exclude` platform dirs, no deps on `examples/`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        if is_out_of_scope(&package.manifest) {
            continue;
        }
        // What mozilla-central vendors: components and megazords, not tools,
        // examples or test harnesses.
        let vendored = ["components/", "megazords/"]
            .iter()
            .any(|p| package.dir.starts_with(p));
        let header = package.line_in_manifest(cx.ws, "[package]");
        let text = cx
            .ws
            .file(&package.manifest)
            .map(|f| f.text.as_str())
            .unwrap_or("");
        if vendored {
            match package.package_field("license").and_then(|l| l.as_str()) {
                None => add(
                    out,
                    &package.manifest,
                    header,
                    "no `license`: add `license = \"MPL-2.0\"` (mozilla-central can't vendor a crate without one)",
                    add_key(text, "license = \"MPL-2.0\"", Some("exclude")),
                ),
                Some("MPL-2.0") => {}
                Some(other) => out.add(
                    &package.manifest,
                    package.line_in_manifest(cx.ws, "license"),
                    format!("`license = \"{other}\"`: crates here are `MPL-2.0`"),
                ),
            }
        }
        if package.package_field("edition").is_none() {
            add(
                out,
                &package.manifest,
                header,
                "no `edition`: without it the crate builds as Rust 2015; add `edition = \"2021\"` like the other crates",
                add_key(text, "edition = \"2021\"", None),
            );
        }
        if package.dir.starts_with("components/") {
            platform_excludes(cx, package, header, text, out);
        }
        if !package.dir.starts_with("examples/") && !package.dir.starts_with("testing/") {
            examples_deps(cx, package, out);
        }
    }
}

fn add(out: &mut Findings, path: &str, line: usize, message: &str, fix: Option<Fix>) {
    match fix {
        Some(fix) => out.add_fix(path, line, message, fix),
        None => out.add(path, line, message),
    }
}

/// The `[package]` table of a manifest: the line of each key, and the last
/// line holding a key (1-based).
fn package_table(text: &str) -> Option<(Vec<(String, usize)>, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with("[package]"))?;
    let mut keys = Vec::new();
    let mut last = start;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        let t = line.trim();
        if t.starts_with('[') && !line.starts_with(char::is_whitespace) {
            break;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        last = i;
        if !line.starts_with(char::is_whitespace) {
            if let Some((key, _)) = t.split_once('=') {
                keys.push((key.trim().to_string(), i + 1));
            }
        }
    }
    Some((keys, last + 1))
}

/// Add `line` to `[package]`: before the `before` key when there is one,
/// else after the table's last key.
fn add_key(text: &str, line: &str, before: Option<&str>) -> Option<Fix> {
    let (keys, last) = package_table(text)?;
    let at = before
        .and_then(|b| keys.iter().find(|(k, _)| k == b))
        .map(|(_, n)| *n)
        .unwrap_or(last + 1);
    Some(Fix::insert_before(at, vec![line.to_string()]))
}

/// Add `items` to `exclude`: a new key, or the end of a one-line array.
fn add_excludes(text: &str, items: &[String]) -> Option<Fix> {
    let (keys, _) = package_table(text)?;
    let Some((_, line)) = keys.iter().find(|(k, _)| k == "exclude") else {
        return add_key(text, &format!("exclude = [{}]", items.join(", ")), None);
    };
    let current = text.lines().nth(line - 1)?;
    let close = current.rfind(']')?;
    let open = current.find('[')?;
    if open > close || current[close + 1..].trim_start().starts_with(|c| c != '#') {
        return None;
    }
    let inner = current[open + 1..close].trim_end();
    let separator = if inner.trim().is_empty() {
        ""
    } else if inner.ends_with(',') {
        " "
    } else {
        ", "
    };
    let fixed = format!(
        "{}{separator}{}{}",
        &current[..open + 1 + inner.len()],
        items.join(", "),
        &current[close..]
    );
    Some(Fix::replace_line(*line, fixed))
}

/// `android/` and `ios/` dirs must be left out of the published crate, or
/// mozilla-central vendors them into `third_party/rust`.
fn platform_excludes(
    cx: &Context,
    package: &Package,
    header: usize,
    text: &str,
    out: &mut Findings,
) {
    let excluded: Vec<String> = package
        .package_field("exclude")
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    s.trim_start_matches('/')
                        .trim_end_matches(['/', '*'])
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    let missing: Vec<&str> = ["android", "ios"]
        .into_iter()
        .filter(|platform| {
            let prefix = format!("{}/{platform}/", package.dir);
            cx.ws.files().any(|f| f.path.starts_with(&prefix))
                && !excluded.iter().any(|e| e == platform)
        })
        .collect();
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().map(|p| format!("\"/{p}\"")).collect();
        add(
            out,
            &package.manifest,
            header,
            &format!(
                "the crate has {} code but no `exclude = [{}]`, so mozilla-central vendors it into third_party/rust",
                missing.join(" and "),
                list.join(", ")
            ),
            add_excludes(text, &list),
        );
    }
}

/// Path dependencies into `examples/` pull example-only code (and its NSS
/// linking) into real crates.
fn examples_deps(cx: &Context, package: &Package, out: &mut Findings) {
    for dep in package.dependencies() {
        let Some(path) = dep.path.as_deref() else {
            continue;
        };
        let target = normalize(&format!("{}/{path}", package.dir));
        if target.starts_with("examples/") {
            out.add(
                &package.manifest,
                package.line_in_manifest(cx.ws, &format!("{} ", dep.name)).max(
                    package.line_in_manifest(cx.ws, &format!("{}=", dep.name)),
                ),
                format!("depends on `{target}`: example crates are not for reuse; move the shared code into components/support/"),
            );
        }
    }
}

/// `a/b/../c` → `a/c`.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: &str = "components/notes/Cargo.toml";
    const ANDROID: (&str, &str) = (
        "components/notes/android/build.gradle",
        "apply from: \"$appServicesRootDir/build-scripts/component-common.gradle\"\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                ANDROID,
                (
                    MANIFEST,
                    include_str!("../../rules/crate-manifest/dont.toml"),
                ),
            ],
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
        assert_eq!((found[0].line, found[1].line, found[2].line), (1, 1, 8));
    }

    /// `(line, remove, insert)` of each edit of a finding's fix.
    fn edits(f: &crate::rule::Finding) -> Vec<(usize, usize, Vec<String>)> {
        let fix = f.fix.as_ref().expect("a fix");
        fix.edits
            .iter()
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    #[test]
    fn fixes_dont() {
        let found = check(
            &RULE,
            &[
                ANDROID,
                (
                    MANIFEST,
                    include_str!("../../rules/crate-manifest/dont.toml"),
                ),
            ],
        );
        // Both keys go after the last key of [package].
        assert_eq!(
            edits(&found[0]),
            vec![(5, 0, vec!["license = \"MPL-2.0\"".to_string()])]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(5, 0, vec!["exclude = [\"/android\"]".to_string()])]
        );
        // Where the shared code should live is for a human to decide.
        assert!(found[2].fix.is_none());
    }

    #[test]
    fn fix_extends_exclude_and_adds_edition() {
        let manifest = "[package]\nname = \"x\"\nexclude = [\"/android\"] # platforms\nlicense = \"MPL-2.0\"\n";
        let found = check(
            &RULE,
            &[
                ("components/x/Cargo.toml", manifest),
                ("components/x/ios/X.swift", "class X {}\n"),
            ],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(
            edits(&found[0]),
            vec![(5, 0, vec!["edition = \"2021\"".to_string()])]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(
                3,
                1,
                vec!["exclude = [\"/android\", \"/ios\"] # platforms".to_string()]
            )]
        );
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                ANDROID,
                (MANIFEST, include_str!("../../rules/crate-manifest/do.toml")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn examples_and_tools_may_skip_the_license_and_use_examples() {
        let example = "[package]\nname = \"sync-pass\"\nedition = \"2021\"\n\n[dependencies]\ncli-support = { path = \"../cli-support\" }\n";
        let tool = "[package]\nname = \"start-bindings\"\nedition = \"2021\"\n";
        let found = check(
            &RULE,
            &[
                ("examples/sync-pass/Cargo.toml", example),
                ("tools/start-bindings/Cargo.toml", tool),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn license_must_be_mpl_and_edition_set() {
        let manifest = "[package]\nname = \"x\"\nlicense = \"MIT\"\n";
        let found = check(&RULE, &[("components/x/Cargo.toml", manifest)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!((found[0].line, found[1].line), (1, 3));
    }
}
