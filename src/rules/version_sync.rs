//! One UniFFI/Glean version everywhere; toolchain versions in sync; no accidental submodule moves
//!
//! Docs and examples: `rules/version-sync/`.

use std::collections::BTreeMap;

use crate::helpers::docs_deps::{lock_packages, text_of, toml_string, LockPackage};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "version-sync",
    area: Area::Deps,
    title: "One UniFFI version, one Glean version everywhere, Android SDK/NDK versions in sync",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const LOCK: &str = "Cargo.lock";
const GRADLE_VERSIONS: &str = "gradle/libs.versions.toml";
const PACKAGE_RESOLVED: &str = "megazords/ios-rust/Package.resolved";
const SETTINGS_GRADLE: &str = "settings.gradle";
const DOCKERFILE: &str = "taskcluster/docker/linux/Dockerfile";

fn check(cx: &Context, out: &mut Findings) {
    if let Some(lock) = cx.ws.file(LOCK) {
        let packages = lock_packages(&lock.text);
        duplicate_uniffi(&packages, out);
        glean(cx, &packages, out);
    }
    android(cx, out);
}

/// Two versions of a UniFFI crate: the bindings and the scaffolding disagree
/// on the FFI contract.
fn duplicate_uniffi(packages: &[LockPackage], out: &mut Findings) {
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for p in packages {
        if !(p.name == "uniffi" || p.name.starts_with("uniffi_")) {
            continue;
        }
        match seen.get(p.name.as_str()) {
            Some(first) if *first != p.version => out.add(
                LOCK,
                p.line,
                format!(
                    "`{}` is in Cargo.lock at both {first} and {}: every crate must use one UniFFI version",
                    p.name, p.version
                ),
            ),
            _ => {
                seen.insert(&p.name, &p.version);
            }
        }
    }
}

/// `glean-sym` (Rust), `glean` (Kotlin) and `glean-swift` (iOS) ship together.
fn glean(cx: &Context, packages: &[LockPackage], out: &mut Findings) {
    let Some(sym) = packages.iter().find(|p| p.name == "glean-sym") else {
        return;
    };
    let mut others: Vec<(String, String)> = Vec::new();
    if let Some(text) = text_of(cx.ws, GRADLE_VERSIONS) {
        if let Some(v) = text.lines().find_map(|l| toml_string(l, "glean")) {
            others.push((format!("`glean` in {GRADLE_VERSIONS}"), v));
        }
    }
    if let Some(v) =
        text_of(cx.ws, PACKAGE_RESOLVED).and_then(|t| resolved_version(&t, "glean-swift"))
    {
        others.push((format!("`glean-swift` in {PACKAGE_RESOLVED}"), v));
    }
    // The optional `glean` crate (Firefox-on-Glean builds) follows mozilla-central:
    // only its major version has to match.
    if let Some(fog) = packages.iter().find(|p| p.name == "glean") {
        if major(&fog.version) != major(&sym.version) {
            others.push((
                "the `glean` crate's major version in Cargo.lock".into(),
                fog.version.clone(),
            ));
        }
    }
    let differing: Vec<String> = others
        .iter()
        .filter(|(_, v)| *v != sym.version)
        .map(|(what, v)| format!("{v} for {what}"))
        .collect();
    if !differing.is_empty() {
        out.add(
            LOCK,
            sym.line + 1,
            format!(
                "`glean-sym` is {} but it is {}: bump every Glean version in the same PR",
                sym.version,
                differing.join(", ")
            ),
        );
    }
}

/// The `"version"` of a pin in a `Package.resolved`.
fn resolved_version(text: &str, identity: &str) -> Option<String> {
    let at = text.find(&format!("\"identity\" : \"{identity}\""))?;
    let rest = &text[at..];
    let end = rest.find('}').unwrap_or(rest.len());
    let version_at = rest[..end].find("\"version\"")?;
    let value = rest[version_at + "\"version\"".len()..]
        .trim_start_matches([' ', ':'])
        .strip_prefix('"')?;
    Some(value[..value.find('"')?].to_string())
}

fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// Android SDK and NDK versions: `build.gradle`, `settings.gradle`, the CI
/// Docker image and the build docs must agree.
fn android(cx: &Context, out: &mut Findings) {
    let settings = cx
        .ws
        .file(SETTINGS_GRADLE)
        .map(|f| commented_args(&f.text))
        .unwrap_or_default();
    let build = cx
        .ws
        .file("build.gradle")
        .map(|f| keyed_values(&f.text))
        .unwrap_or_default();
    for key in [
        "compileSdkMajorVersion",
        "compileSdkMinorVersion",
        "targetSdkVersion",
        "minSdkVersion",
    ] {
        if let (Some((_, a)), Some((line, b))) = (build.get(key), settings.get(key)) {
            if a != b {
                out.add(
                    SETTINGS_GRADLE,
                    *line,
                    format!("{key} is {b} here but {a} in build.gradle: keep them in sync"),
                );
            }
        }
    }
    let docker = text_of(cx.ws, DOCKERFILE).unwrap_or_default();
    let docker_env = |name: &str| {
        docker.lines().enumerate().find_map(|(i, l)| {
            let rest = l.trim().strip_prefix("ENV ")?.trim().strip_prefix(name)?;
            Some((i + 1, rest.trim().trim_matches(['=', ' ', '"']).to_string()))
        })
    };
    let Some((ndk_line, ndk)) = settings.get("ndkVersion").cloned() else {
        return;
    };
    let ndk = ndk.trim_matches('"').to_string();
    if let Some((line, docker_ndk)) = docker_env("ANDROID_NDK_VERSION") {
        if docker_ndk != ndk {
            out.add(
                DOCKERFILE,
                line,
                format!("the CI image installs NDK {docker_ndk} but settings.gradle builds with {ndk} (line {ndk_line}): keep them in sync"),
            );
        }
    }
    if let (Some((line, platform)), Some((_, major)), Some((_, minor))) = (
        docker_env("ANDROID_PLATFORM_VERSION"),
        settings.get("compileSdkMajorVersion"),
        settings.get("compileSdkMinorVersion"),
    ) {
        let want = if minor == "0" {
            major.clone()
        } else {
            format!("{major}.{minor}")
        };
        if platform != want {
            out.add(
                DOCKERFILE,
                line,
                format!("the CI image installs Android platform {platform} but the build compiles against {want}: keep them in sync"),
            );
        }
    }
    for file in cx.ws.files_with_ext(&["md"]) {
        if !file.path.starts_with("docs/") {
            continue;
        }
        for (line, text) in file.lines() {
            if !text.contains("NDK") || text.trim_start().starts_with("export ") {
                continue;
            }
            for word in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
                if is_ndk_version(word) && word != ndk {
                    out.add(
                        &file.path,
                        line,
                        format!("says NDK {word} but the build uses {ndk}: update the docs"),
                    );
                }
            }
        }
    }
}

/// `29.0.14206865`.
fn is_ndk_version(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 3 && parts[2].len() >= 7 && parts.iter().all(|p| !p.is_empty())
}

/// `36, // compileSdkMajorVersion,` → `compileSdkMajorVersion -> (line, "36")`.
fn commented_args(text: &str) -> BTreeMap<String, (usize, String)> {
    let mut out = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let Some((value, comment)) = line.split_once("//") else {
            continue;
        };
        let value = value.trim().trim_end_matches(',').trim();
        let key = comment.trim().split([',', ' ']).next().unwrap_or("");
        if !value.is_empty() && !key.is_empty() && !value.contains(' ') {
            out.insert(key.to_string(), (i + 1, value.to_string()));
        }
    }
    out
}

/// `compileSdkMajorVersion: 36,` → `compileSdkMajorVersion -> (line, "36")`.
fn keyed_values(text: &str) -> BTreeMap<String, (usize, String)> {
    let mut out = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim().trim_end_matches(',').trim();
        if key.chars().all(|c| c.is_alphanumeric())
            && value.chars().all(|c| c.is_ascii_digit())
            && !value.is_empty()
        {
            out.insert(key.to_string(), (i + 1, value.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::docs_deps::files_from_diff;
    use crate::rule::Finding;
    use crate::testing::check;

    const GRADLE: (&str, &str) = (
        "gradle/libs.versions.toml",
        "[versions]\nglean = \"70.0.0\"\n",
    );
    const RESOLVED: (&str, &str) = (
        "megazords/ios-rust/Package.resolved",
        "{\n  \"pins\" : [\n    {\n      \"identity\" : \"glean-swift\",\n      \"state\" : {\n        \"version\" : \"70.0.0\"\n      }\n    }\n  ]\n}\n",
    );

    fn run(diff: &str) -> Vec<Finding> {
        let changed = files_from_diff(diff);
        let mut files: Vec<(&str, &str)> = changed
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        for context in [GRADLE, RESOLVED] {
            if !files.iter().any(|(p, _)| *p == context.0) {
                files.push(context);
            }
        }
        check(&RULE, &files)
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/version-sync/dont.diff"));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.lock", 3));
        assert!(found[0]
            .message
            .contains("70.0.0 for `glean` in gradle/libs.versions.toml"));
    }

    #[test]
    fn accepts_do() {
        let found = run(include_str!("../../rules/version-sync/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn duplicate_uniffi_versions() {
        let lock = "[[package]]\nname = \"uniffi\"\nversion = \"0.29.4\"\n\n[[package]]\nname = \"uniffi\"\nversion = \"0.32.2\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.2\"\n";
        let found = check(&RULE, &[("Cargo.lock", lock)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 6);
    }

    #[test]
    fn android_versions() {
        let build = "ext.build = [\n    compileSdkMajorVersion: 36,\n    compileSdkMinorVersion: 1,\n    targetSdkVersion: 36,\n]\n";
        let settings = "new Config(\n    36, // compileSdkMajorVersion,\n    1,  // compileSdkMinorVersion,\n    35, // targetSdkVersion,\n    \"29.0.14206865\", // ndkVersion - Keep it in sync in TC Dockerfile.\n)\n";
        let docker =
            "ENV ANDROID_PLATFORM_VERSION \"36.1\"\nENV ANDROID_NDK_VERSION \"28.2.13676358\"\n";
        let docs = "1. NDK (Side by side) version 29.0.14206865\n2. NDK 28.2.13676358 also works\n    export NDK_ROOT=\"$HOME/sdk/ndk/27.0.12077973\"\n";
        let found = check(
            &RULE,
            &[
                ("build.gradle", build),
                ("settings.gradle", settings),
                ("taskcluster/docker/linux/Dockerfile", docker),
                ("docs/building.md", docs),
            ],
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
    }
}
