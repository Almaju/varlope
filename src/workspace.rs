//! The checkout being linted: its files and its Cargo packages, loaded once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result};

use crate::source::{RustFile, TextFile};

/// Extensions and file names varlope reads. Everything else is skipped.
const EXTENSIONS: &[&str] = &[
    "rs", "udl", "toml", "yaml", "yml", "md", "sh", "py", "kt", "kts", "swift", "gradle", "sql",
    "txt", "xml", "json",
];
const MAX_BYTES: u64 = 1_000_000;

#[derive(Debug, Default)]
pub struct Workspace {
    pub root: PathBuf,
    files: BTreeMap<String, TextFile>,
    rust: BTreeMap<String, RustFile>,
    /// Rust files that do not parse, with the error.
    pub parse_errors: Vec<(String, String)>,
    packages: Vec<Package>,
}

impl Workspace {
    /// Load every tracked text file under `root` (via `git ls-files` when it is
    /// a git checkout, otherwise a directory walk that skips `target/`).
    pub fn load(root: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for rel in list_files(root)? {
            if !wanted(&rel) {
                continue;
            }
            let path = root.join(&rel);
            let Ok(meta) = path.metadata() else { continue };
            if !meta.is_file() || meta.len() > MAX_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue; // not UTF-8
            };
            files.push((rel, text));
        }
        Ok(Self::build(root.to_path_buf(), files))
    }

    /// A workspace made of in-memory files, for tests.
    pub fn from_files(files: &[(&str, &str)]) -> Self {
        Self::build(
            PathBuf::from("."),
            files
                .iter()
                .map(|(p, t)| (p.to_string(), t.to_string()))
                .collect(),
        )
    }

    fn build(root: PathBuf, files: Vec<(String, String)>) -> Self {
        let mut ws = Workspace {
            root,
            ..Default::default()
        };
        for (path, text) in files {
            if path.ends_with(".rs") {
                match RustFile::parse(path.clone(), text.clone()) {
                    Ok(file) => {
                        ws.rust.insert(path.clone(), file);
                    }
                    Err(error) => ws.parse_errors.push((path.clone(), error.to_string())),
                }
            }
            if path.ends_with("Cargo.toml") {
                if let Some(package) = Package::parse(&path, &text) {
                    ws.packages.push(package);
                }
            }
            ws.files.insert(path.clone(), TextFile::new(path, text));
        }
        ws
    }

    /// Every loaded text file (Rust files included), sorted by path.
    pub fn files(&self) -> impl Iterator<Item = &TextFile> {
        self.files.values()
    }

    pub fn file(&self, path: &str) -> Option<&TextFile> {
        self.files.get(path)
    }

    /// Files whose extension is one of `exts` (without the dot).
    pub fn files_with_ext<'a>(&'a self, exts: &'a [&'a str]) -> impl Iterator<Item = &'a TextFile> {
        self.files
            .values()
            .filter(move |f| exts.contains(&f.extension()))
    }

    /// Every Rust file that parses.
    pub fn rust_files(&self) -> impl Iterator<Item = &RustFile> {
        self.rust.values()
    }

    pub fn rust(&self, path: &str) -> Option<&RustFile> {
        self.rust.get(path)
    }

    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages
            .iter()
            .find(|p| p.name == name || p.lib_name() == name)
    }

    /// The package that owns `path`: the one with the longest matching dir.
    pub fn package_of(&self, path: &str) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|p| p.dir.is_empty() || path.starts_with(&format!("{}/", p.dir)))
            .max_by_key(|p| p.dir.len())
    }

    /// Files of one package (excluding nested packages).
    pub fn package_files<'a>(&'a self, package: &'a Package) -> impl Iterator<Item = &'a TextFile> {
        self.files.values().filter(move |f| {
            self.package_of(&f.path)
                .is_some_and(|p| p.manifest == package.manifest)
        })
    }

    /// Crate names (with `_`) the iOS megazord re-exports: these all end up in
    /// one Swift module.
    pub fn ios_megazord_crates(&self) -> Vec<String> {
        self.megazord_crates("megazords/ios-rust/src/lib.rs")
    }

    /// Crate names the Android megazord re-exports.
    pub fn android_megazord_crates(&self) -> Vec<String> {
        self.megazord_crates("megazords/full/src/lib.rs")
    }

    fn megazord_crates(&self, path: &str) -> Vec<String> {
        let Some(file) = self.file(path) else {
            return Vec::new();
        };
        file.lines()
            .filter_map(|(_, l)| {
                let l = l.trim();
                let name = l.strip_prefix("pub use ")?.trim_end_matches(';');
                (!name.contains("::") && !name.contains('{')).then(|| name.to_string())
            })
            .collect()
    }
}

/// A `Cargo.toml` with a `[package]`.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    /// Directory relative to the root (`""` for the root package).
    pub dir: String,
    /// Path of the manifest.
    pub manifest: String,
    pub toml: toml::Table,
}

/// One dependency entry of a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The name used in code (`package = ".."` renames are resolved to the key).
    pub name: String,
    pub kind: DepKind,
    pub optional: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

impl Package {
    fn parse(manifest: &str, text: &str) -> Option<Self> {
        let toml: toml::Table = text.parse().ok()?;
        let name = toml.get("package")?.get("name")?.as_str()?.to_string();
        let dir = manifest
            .strip_suffix("Cargo.toml")
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();
        Some(Self {
            name,
            dir,
            manifest: manifest.to_string(),
            toml,
        })
    }

    /// The crate name as used in Rust code.
    pub fn lib_name(&self) -> String {
        self.toml
            .get("lib")
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.name.replace('-', "_"))
    }

    /// A string field of `[package]`, if set.
    pub fn package_field(&self, key: &str) -> Option<&toml::Value> {
        self.toml.get("package")?.get(key)
    }

    pub fn dependencies(&self) -> Vec<Dependency> {
        let mut out = Vec::new();
        for (table, kind) in [
            ("dependencies", DepKind::Normal),
            ("dev-dependencies", DepKind::Dev),
            ("build-dependencies", DepKind::Build),
        ] {
            let Some(deps) = self.toml.get(table).and_then(|t| t.as_table()) else {
                continue;
            };
            for (key, value) in deps {
                out.push(Dependency {
                    name: key.clone(),
                    kind,
                    optional: value
                        .get("optional")
                        .and_then(|o| o.as_bool())
                        .unwrap_or(false),
                    path: value
                        .get("path")
                        .and_then(|p| p.as_str())
                        .map(str::to_string),
                });
            }
        }
        out
    }

    /// Feature names declared in `[features]`.
    pub fn features(&self) -> Vec<String> {
        self.toml
            .get("features")
            .and_then(|f| f.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// The 1-based line of `needle` in the manifest text, or 0.
    pub fn line_in_manifest(&self, ws: &Workspace, needle: &str) -> usize {
        ws.file(&self.manifest)
            .and_then(|f| f.lines().find(|(_, l)| l.contains(needle)).map(|(n, _)| n))
            .unwrap_or(0)
    }
}

/// Test, bench and example code: under `tests/`, `benches/`, `examples/`,
/// `testing/`, or a `tests.rs` / `*_tests.rs` / `test_*.rs` file.
pub fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.split('/').any(|part| {
        matches!(
            part,
            "tests" | "benches" | "examples" | "testing" | "fixtures" | "integration-tests"
        )
    }) || name == "tests.rs"
        || name == "test.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with("_test.rs")
        || name.starts_with("test_")
}

/// Production Rust code of a component: `components/**/src/**.rs`, not tests.
pub fn is_component_src(path: &str) -> bool {
    path.starts_with("components/")
        && path.contains("/src/")
        && path.ends_with(".rs")
        && !is_test_path(path)
}

fn wanted(rel: &str) -> bool {
    if rel.starts_with("target/") || rel.contains("/target/") || rel.contains("node_modules/") {
        return false;
    }
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rsplit_once('.') {
        Some((_, ext)) => EXTENSIONS.contains(&ext),
        None => false,
    }
}

fn list_files(root: &Path) -> Result<Vec<String>> {
    let git = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output();
    if let Ok(out) = git {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect());
        }
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
        let n = e.file_name().to_string_lossy();
        n != "target" && n != ".git" && n != "node_modules"
    }) {
        let entry = entry.context("walking the workspace")?;
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_and_ownership() {
        let ws = Workspace::from_files(&[
            (
                "components/a/Cargo.toml",
                "[package]\nname = \"a-b\"\n[dependencies]\nserde = \"1\"\n",
            ),
            ("components/a/src/lib.rs", "pub fn f() {}"),
            (
                "megazords/ios-rust/src/lib.rs",
                "pub use a_b;\npub use x::y;\n",
            ),
        ]);
        let p = ws.package_of("components/a/src/lib.rs").unwrap();
        assert_eq!(p.name, "a-b");
        assert_eq!(p.lib_name(), "a_b");
        assert_eq!(p.dependencies()[0].name, "serde");
        assert_eq!(ws.ios_megazord_crates(), vec!["a_b"]);
        assert_eq!(ws.rust_files().count(), 2);
    }

    #[test]
    fn test_paths() {
        assert!(is_test_path("components/a/tests/x.rs"));
        assert!(is_test_path("components/a/src/sync_tests.rs"));
        assert!(!is_test_path("components/a/src/lib.rs"));
        assert!(is_component_src("components/a/src/lib.rs"));
        assert!(!is_component_src("components/a/src/tests.rs"));
    }
}
