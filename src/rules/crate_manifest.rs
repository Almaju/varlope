//! Crate manifests: `license = "MPL-2.0"`, `exclude` platform dirs, no deps on `examples/`
//!
//! Docs and examples: `rules/crate-manifest/`.

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
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
        if vendored {
            match package.package_field("license").and_then(|l| l.as_str()) {
                None => out.add(
                    &package.manifest,
                    header,
                    "no `license`: add `license = \"MPL-2.0\"` (mozilla-central can't vendor a crate without one)",
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
            out.add(
                &package.manifest,
                header,
                "no `edition`: without it the crate builds as Rust 2015; add `edition = \"2021\"` like the other crates",
            );
        }
        if package.dir.starts_with("components/") {
            platform_excludes(cx, package, header, out);
        }
        if !package.dir.starts_with("examples/") && !package.dir.starts_with("testing/") {
            examples_deps(cx, package, out);
        }
    }
}

/// `android/` and `ios/` dirs must be left out of the published crate, or
/// mozilla-central vendors them into `third_party/rust`.
fn platform_excludes(cx: &Context, package: &Package, header: usize, out: &mut Findings) {
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
        out.add(
            &package.manifest,
            header,
            format!(
                "the crate has {} code but no `exclude = [{}]`, so mozilla-central vendors it into third_party/rust",
                missing.join(" and "),
                list.join(", ")
            ),
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
