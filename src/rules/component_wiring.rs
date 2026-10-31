//! New/renamed components are registered everywhere (megazords, uniffi.toml, Swift, Gradle, CI)
//!
//! Docs and examples: `rules/component-wiring/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::docs_deps::{comment_lines, hash_comment_start};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;
use crate::workspace::{DepKind, Package, Workspace};

pub static RULE: Rule = Rule {
    id: "component-wiring",
    area: Area::Deps,
    title: "New/renamed components are registered everywhere (megazords, uniffi.toml, Gradle, CI)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const ANDROID_MEGAZORD: &str = "megazords/full";
const BUILDCONFIG: &str = ".buildconfig-android.yml";

fn check(cx: &Context, out: &mut Findings) {
    // component -> (needed by an iOS megazord, needed by the Android one)
    let mut platforms: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for megazord in cx
        .ws
        .packages()
        .iter()
        .filter(|p| p.dir.starts_with("megazords/"))
    {
        let Some(lib) = lib_file(cx.ws, megazord) else {
            continue;
        };
        let exported = reexports(lib);
        deps_and_reexports(cx, megazord, lib, &exported, out);
        for (_, name) in &exported {
            let entry = platforms.entry(name.clone()).or_default();
            entry.0 |= megazord.dir.contains("ios");
            entry.1 |= megazord.dir == ANDROID_MEGAZORD;
        }
    }
    for (name, (ios, android)) in platforms {
        let Some(component) = cx.ws.package(&name) else {
            continue;
        };
        if has_uniffi(cx.ws, component) {
            bindings_config(cx, component, ios, android, out);
        }
    }
    android_modules(cx, out);
}

/// The megazord's `lib.rs`.
fn lib_file<'w>(ws: &'w Workspace, package: &Package) -> Option<&'w TextFile> {
    let custom = package
        .toml
        .get("lib")
        .and_then(|l| l.get("path"))
        .and_then(|p| p.as_str())
        .map(|p| format!("{}/{}", package.dir, p.trim_start_matches("./")));
    let path = custom.unwrap_or_else(|| format!("{}/src/lib.rs", package.dir));
    ws.file(&path)
}

/// `(line, crate)` of each `pub use crate;` (or `pub use crate as alias;`).
fn reexports(lib: &TextFile) -> Vec<(usize, String)> {
    lib.lines()
        .filter_map(|(n, l)| {
            let used = l
                .trim()
                .strip_prefix("pub use ")?
                .trim_end_matches(';')
                .trim();
            let name = used.split(" as ").next().unwrap_or(used).trim();
            let simple = !name.contains("::") && !name.contains('{') && !name.contains(' ');
            simple.then(|| (n, name.to_string()))
        })
        .collect()
}

/// Packages linked into a megazord: the re-exported crates and everything
/// they depend on (in-repo crates only), by lib name.
fn linked(ws: &Workspace, exported: &[(usize, String)]) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut todo: Vec<String> = exported.iter().map(|(_, n)| n.clone()).collect();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(package) = ws.package(&name) else {
            continue;
        };
        for dep in package.dependencies() {
            if dep.kind == DepKind::Normal && dep.path.is_some() {
                if let Some(p) = ws.package(&dep.name) {
                    todo.push(p.lib_name());
                }
            }
        }
    }
    seen
}

/// A crate with UniFFI bindings: a `uniffi.toml`, a UDL file, or scaffolding
/// set up in its sources.
fn has_uniffi(ws: &Workspace, package: &Package) -> bool {
    ws.file(&format!("{}/uniffi.toml", package.dir)).is_some()
        || ws.package_files(package).any(|f| {
            f.extension() == "udl"
                || (f.extension() == "rs" && f.text.contains("uniffi::setup_scaffolding!"))
        })
}

/// A megazord links only the crates its `lib.rs` re-exports: a dependency
/// without `pub use` ships without its FFI, and a `pub use` without a
/// dependency doesn't build.
fn deps_and_reexports(
    cx: &Context,
    megazord: &Package,
    lib: &TextFile,
    exported: &[(usize, String)],
    out: &mut Findings,
) {
    let linked = linked(cx.ws, exported);
    let mut dep_libs = BTreeSet::new();
    for dep in megazord.dependencies() {
        if dep.kind != DepKind::Normal {
            continue;
        }
        let package = dep.path.as_ref().and_then(|_| {
            cx.ws
                .package(&dep.name)
                .or_else(|| cx.ws.package(&dep.name.replace('-', "_")))
        });
        let lib_name = package
            .map(|p| p.lib_name())
            .unwrap_or_else(|| dep.name.replace('-', "_"));
        dep_libs.insert(lib_name.clone());
        let Some(package) = package else { continue };
        if has_uniffi(cx.ws, package) && !linked.contains(&lib_name) {
            let line = megazord.line_in_manifest(cx.ws, &format!("{} ", dep.name));
            out.add(
                &megazord.manifest,
                line,
                format!("`{}` is a dependency but {} has no `pub use {lib_name};` and nothing re-exported uses it, so it is not linked in: re-export it or drop the dependency", dep.name, lib.path),
            );
        }
    }
    for (line, name) in exported {
        if !dep_libs.contains(name) {
            out.add(
                &lib.path,
                *line,
                format!(
                    "`{name}` is re-exported but is not a dependency in {}",
                    megazord.manifest
                ),
            );
        }
    }
}

/// `uniffi.toml` settings every megazord crate needs, per platform.
fn bindings_config(
    cx: &Context,
    component: &Package,
    ios: bool,
    android: bool,
    out: &mut Findings,
) {
    let path = format!("{}/uniffi.toml", component.dir);
    let Some(file) = cx.ws.file(&path) else {
        out.add(
            &component.manifest,
            0,
            format!("`{}` is in a megazord but has no uniffi.toml: copy one from tools/start-bindings/templates", component.name),
        );
        return;
    };
    let Ok(toml) = file.text.parse::<toml::Table>() else {
        return;
    };
    let section = |lang: &str| toml.get("bindings").and_then(|b| b.get(lang));
    let mut missing = Vec::new();
    if android {
        match section("kotlin") {
            None => {
                missing.push("`[bindings.kotlin]` with `package_name` and `omit_checksums = true`")
            }
            Some(k) => {
                if k.get("package_name").is_none() {
                    missing.push("`[bindings.kotlin] package_name`");
                }
                if k.get("omit_checksums").and_then(|v| v.as_bool()) != Some(true) {
                    missing.push("`[bindings.kotlin] omit_checksums = true`");
                }
            }
        }
    }
    if ios {
        match section("swift") {
            None => missing.push(
                "`[bindings.swift]` with `ffi_module_name = \"MozillaRustComponents\"` and `ffi_module_filename`",
            ),
            Some(s) => {
                if s.get("ffi_module_name").and_then(|v| v.as_str()) != Some("MozillaRustComponents") {
                    missing.push("`[bindings.swift] ffi_module_name = \"MozillaRustComponents\"`");
                }
                if s.get("ffi_module_filename").is_none() {
                    missing.push("`[bindings.swift] ffi_module_filename`");
                }
            }
        }
    }
    for what in missing {
        out.add(
            &path,
            1,
            format!("no {what}: every megazord crate sets it (see tools/start-bindings/templates)"),
        );
    }
}

/// Every Android module is published (`.buildconfig-android.yml`), wired up
/// like the template, and its crate is in the Android megazord.
fn android_modules(cx: &Context, out: &mut Findings) {
    let buildconfig: BTreeSet<String> = cx
        .ws
        .file(BUILDCONFIG)
        .map(|f| {
            f.lines()
                .filter_map(|(_, l)| {
                    let code = match hash_comment_start(l) {
                        Some(at) => &l[..at],
                        None => l,
                    };
                    code.trim()
                        .strip_prefix("path:")
                        .map(|p| p.trim().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let android_linked: BTreeSet<String> = cx
        .ws
        .packages()
        .iter()
        .find(|p| p.dir == ANDROID_MEGAZORD)
        .and_then(|m| lib_file(cx.ws, m))
        .map(|lib| linked(cx.ws, &reexports(lib)))
        .unwrap_or_default();
    for file in cx.ws.files() {
        let Some(dir) = file.path.strip_suffix("/android/build.gradle") else {
            continue;
        };
        if !dir.starts_with("components/") || !has_code(file) {
            continue;
        }
        let module = format!("{dir}/android");
        if cx.ws.file(BUILDCONFIG).is_some() && !buildconfig.contains(&module) {
            out.add(
                &file.path,
                1,
                format!("`{module}` is not a project in {BUILDCONFIG}, so it is never built or published"),
            );
        }
        let calls = [
            "configureUniFFIBindgen",
            "dependsOnTheMegazord",
            "configurePublish",
        ];
        let missing: Vec<&str> = calls
            .into_iter()
            .filter(|c| !file.text.contains(c))
            .collect();
        if !missing.is_empty() {
            out.add(
                &file.path,
                1,
                format!(
                    "no `{}` call: start from tools/start-bindings/templates/build.gradle",
                    missing.join("`, `")
                ),
            );
        }
        if let Some(package) = cx.ws.package_of(&file.path) {
            let lib = package.lib_name();
            if !android_linked.is_empty() && !android_linked.contains(&lib) {
                out.add(
                    &file.path,
                    1,
                    format!("`{lib}` has an Android module but is not linked into {ANDROID_MEGAZORD}: add `pub use {lib};` to its lib.rs"),
                );
            }
        }
    }
}

/// A build file with something besides comments in it.
fn has_code(file: &TextFile) -> bool {
    let comments = comment_lines(&file.path, &file.text);
    file.lines().any(|(n, l)| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with("//") && !(comments.contains_key(&n) && t.starts_with('*'))
    })
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::docs_deps::files_from_diff;
    use crate::rule::Finding;
    use crate::testing::check;

    const CONTEXT: &[(&str, &str)] = &[
        ("megazords/full/Cargo.toml", "[package]\nname = \"full-megazord\"\n\n[dependencies]\nlogins = { path = \"../../components/logins\" }\n"),
        ("megazords/full/src/lib.rs", "pub use logins;\n"),
        ("megazords/ios-rust/Cargo.toml", "[package]\nname = \"megazord_ios\"\n\n[dependencies]\nlogins = { path = \"../../components/logins\" }\n"),
        ("megazords/ios-rust/src/lib.rs", "pub use logins;\n"),
        ("megazords/ios-rust/focus/Cargo.toml", "[package]\nname = \"megazord_focus\"\n\n[dependencies]\nlogins = { path = \"../../../components/logins\" }\n"),
        ("megazords/ios-rust/focus/src/lib.rs", "pub use logins;\n"),
        ("components/logins/Cargo.toml", "[package]\nname = \"logins\"\n"),
        ("components/logins/uniffi.toml", "[bindings.kotlin]\npackage_name = \"mozilla.appservices.logins\"\nomit_checksums = true\n[bindings.swift]\nffi_module_name = \"MozillaRustComponents\"\nffi_module_filename = \"loginsFFI\"\n"),
        ("components/notes/Cargo.toml", "[package]\nname = \"notes\"\n"),
    ];

    /// The context files, with the diff's new lines appended (and its new
    /// files added).
    fn run(diff: &str) -> Vec<Finding> {
        let changed = files_from_diff(diff);
        let mut files: Vec<(String, String)> = CONTEXT
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect();
        for (path, text) in changed {
            match files.iter_mut().find(|(p, _)| *p == path) {
                Some((_, existing)) => {
                    for line in text.lines() {
                        if !existing.lines().any(|l| l == line) {
                            existing.push_str(line);
                            existing.push('\n');
                        }
                    }
                }
                None => files.push((path, text)),
            }
        }
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        check(&RULE, &files)
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/component-wiring/dont.diff"));
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 1, "{messages:#?}");
        assert_eq!(
            (found[0].path.as_str(), found[0].line),
            ("megazords/ios-rust/focus/Cargo.toml", 6)
        );
    }

    #[test]
    fn accepts_do() {
        let found = run(include_str!("../../rules/component-wiring/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn android_modules() {
        let gradle = "apply from: \"$appServicesRootDir/build-scripts/component-common.gradle\"\next.configureUniFFIBindgen(\"notes\")\n";
        let placeholder = "// TODO: add a build.gradle once this ships on Android\n";
        let mut files: Vec<(&str, &str)> = CONTEXT.to_vec();
        files.extend([
            (".buildconfig-android.yml", "projects:\n  logins:\n    path: components/logins/android\n  # notes:\n  #   path: components/notes/android\n"),
            ("components/notes/android/build.gradle", gradle),
            ("components/webext-storage/android/build.gradle", placeholder),
            ("components/webext-storage/Cargo.toml", "[package]\nname = \"webext-storage\"\n"),
        ]);
        let found = check(&RULE, &files);
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
        assert!(messages[0].contains("not a project"));
        assert!(messages[1].contains("dependsOnTheMegazord"));
        assert!(messages[2].contains("not linked into"));
    }

    #[test]
    fn swift_settings_for_ios_megazords() {
        let mut files: Vec<(&str, &str)> = CONTEXT
            .iter()
            .filter(|(p, _)| !p.starts_with("megazords/ios-rust/"))
            .copied()
            .collect();
        files.extend([
            ("megazords/ios-rust/Cargo.toml", "[package]\nname = \"megazord_ios\"\n\n[dependencies]\nlogins = { path = \"../../components/logins\" }\nnotes = { path = \"../../components/notes\" }\n"),
            ("megazords/ios-rust/src/lib.rs", "pub use logins;\npub use notes;\n"),
            ("components/notes/uniffi.toml", "[bindings.kotlin]\npackage_name = \"mozilla.appservices.notes\"\n"),
        ]);
        let found = check(&RULE, &files);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("[bindings.swift]"));
    }

    #[test]
    fn reexport_without_dependency() {
        let mut files: Vec<(&str, &str)> = CONTEXT
            .iter()
            .filter(|(p, _)| *p != "megazords/ios-rust/src/lib.rs")
            .copied()
            .collect();
        files.push((
            "megazords/ios-rust/src/lib.rs",
            "pub use logins;\npub use tabs;\n",
        ));
        let found = check(&RULE, &files);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(
            (found[0].path.as_str(), found[0].line),
            ("megazords/ios-rust/src/lib.rs", 2)
        );
    }
}
