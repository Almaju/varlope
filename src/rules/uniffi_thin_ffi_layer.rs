//! Keep the FFI layer thin: `Arc`, builders and serde stay at the boundary
//!
//! Docs and examples: `rules/uniffi-thin-ffi-layer/`.

use std::collections::BTreeSet;

use quote::ToTokens;

use crate::helpers::api_mobile::{has_word, is_production_rust, words};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::workspace::Package;

pub static RULE: Rule = Rule {
    id: "uniffi-thin-ffi-layer",
    area: Area::Uniffi,
    title: "Keep the FFI layer thin: `Arc`, builders and serde stay at the boundary",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// Field types or names that mean the object runs or records something.
const OWNED_MACHINERY: &[&str] = &["worker", "telemetry", "thread", "join", "scheduler"];

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        if !package.dependencies().iter().any(|d| d.name == "uniffi") {
            continue;
        }
        let files: Vec<&RustFile> = cx
            .ws
            .package_files(package)
            .filter(|f| is_production_rust(&f.path))
            .filter_map(|f| cx.ws.rust(&f.path))
            .collect();
        for file in &files {
            machinery_in_ffi_objects(file, out);
        }
        if files.iter().any(|f| is_ffi_module(package, &f.path)) {
            // In a UDL crate, `#[handle_error]` sits on functions the UDL exports.
            let has_udl = cx.ws.package_files(package).any(|f| f.extension() == "udl");
            core_depends_on_ffi(package, &files, !has_udl, out);
        }
    }
}

/// `src/ffi.rs`, `src/ffi/**`: the crate says where its FFI layer is.
fn is_ffi_module(package: &Package, path: &str) -> bool {
    let src = format!("{}/src/", package.dir);
    path.strip_prefix(&src)
        .is_some_and(|rest| rest == "ffi.rs" || rest.starts_with("ffi/"))
}

/// A `uniffi::Object` that delegates to a core `inner` but also owns a
/// worker, a thread or telemetry itself.
fn machinery_in_ffi_objects(file: &RustFile, out: &mut Findings) {
    for item in syntax::all_items(&file.ast) {
        let syn::Item::Struct(s) = item else { continue };
        if !syntax::has_derive(&s.attrs, "uniffi::Object") || file.in_test(file.line_of_node(s)) {
            continue;
        }
        let names: Vec<String> = s
            .fields
            .iter()
            .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
            .collect();
        let delegates = names.iter().any(|n| n == "inner" || n == "internal");
        if !delegates {
            continue; // no core/FFI split to keep
        }
        for f in &s.fields {
            let Some(ident) = &f.ident else { continue };
            let text = format!("{ident} {}", syntax::type_string(&f.ty));
            let w = words(&text.replace(['<', '>', ':', ',', '(', ')'], " "));
            if let Some(what) = OWNED_MACHINERY.iter().find(|m| w.iter().any(|x| x == *m)) {
                out.add(
                    &file.path,
                    file.line_of_node(ident),
                    format!(
                        "FFI object `{}` owns `{ident}` ({what}); move it into the core type and keep the object to FFI plumbing",
                        s.ident
                    ),
                );
            }
        }
    }
}

/// Core modules that name the FFI object, the `*ApiResult`/`*ApiError` the
/// FFI layer returns, or use `#[handle_error]`.
fn core_depends_on_ffi(
    package: &Package,
    files: &[&RustFile],
    check_handle_error: bool,
    out: &mut Findings,
) {
    let mut ffi_names: BTreeSet<String> = BTreeSet::new();
    for file in files {
        for item in syntax::all_items(&file.ast) {
            match item {
                syn::Item::Struct(s) if syntax::has_derive(&s.attrs, "uniffi::Object") => {
                    ffi_names.insert(s.ident.to_string());
                }
                syn::Item::Enum(e) if syntax::has_derive(&e.attrs, "uniffi::Error") => {
                    ffi_names.insert(e.ident.to_string());
                }
                syn::Item::Type(t) => {
                    let name = t.ident.to_string();
                    let target = t.ty.to_token_stream().to_string();
                    if name.ends_with("ApiResult") || target.contains("ApiError") {
                        ffi_names.insert(name);
                    }
                }
                _ => {}
            }
        }
    }
    // UDL interfaces are left out: the UDL only declares them, and the Rust
    // type behind one is usually the core type itself.
    for file in files {
        let rest = file
            .path
            .strip_prefix(&format!("{}/src/", package.dir))
            .unwrap_or("");
        // `error.rs` is where internal errors convert to the public ones.
        let boundary = rest == "lib.rs" || rest == "error.rs" || rest.ends_with("/error.rs");
        if boundary || is_ffi_module(package, &file.path) || exports(file) {
            continue;
        }
        let defined = defined_names(file);
        for (line, text) in file.lines() {
            if file.in_test(line) {
                continue;
            }
            let code = text.split("//").next().unwrap_or("");
            let named = ffi_names
                .iter()
                .find(|n| !defined.contains(*n) && has_word(code, n));
            let message = if check_handle_error && code.contains("#[handle_error") {
                Some("core module uses `#[handle_error]`; return the internal `Result` and convert in the FFI layer".to_string())
            } else {
                named.map(|n| {
                    format!("core module depends on the FFI layer's `{n}`; take and return core types and let the FFI layer convert")
                })
            };
            if let Some(message) = message {
                out.add(&file.path, line, message);
                break; // one finding per file is enough to start the conversation
            }
        }
    }
}

/// Types and aliases a file declares.
fn defined_names(file: &RustFile) -> BTreeSet<String> {
    syntax::all_items(&file.ast)
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(s) => Some(s.ident.to_string()),
            syn::Item::Enum(e) => Some(e.ident.to_string()),
            syn::Item::Type(t) => Some(t.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// Whether a file declares exports itself (it is part of the FFI layer).
fn exports(file: &RustFile) -> bool {
    syntax::all_items(&file.ast).iter().any(|item| match item {
        syn::Item::Fn(f) => syntax::has_attr(&f.attrs, "uniffi::export"),
        syn::Item::Impl(i) => syntax::has_attr(&i.attrs, "uniffi::export"),
        syn::Item::Struct(s) => syntax::has_derive(&s.attrs, "uniffi::Object"),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/ads-client/Cargo.toml",
        "[package]\nname = \"ads-client\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const FFI: (&str, &str) = (
        "components/ads-client/src/ffi.rs",
        "pub type AdsClientApiResult<T> = std::result::Result<T, MozAdsClientApiError>;\n#[derive(Debug, thiserror::Error, uniffi::Error)]\npub enum MozAdsClientApiError { #[error(\"x\")] Other { reason: String } }\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                FFI,
                (
                    "components/ads-client/src/lib.rs",
                    include_str!("../../rules/uniffi-thin-ffi-layer/dont.rs"),
                ),
                (
                    "components/ads-client/src/worker.rs",
                    "use crate::{AdsClientApiResult, MozAdsClient};\n\npub struct BackgroundWorker;\n",
                ),
            ],
        );
        let at: Vec<(&str, usize)> = found.iter().map(|f| (f.path.as_str(), f.line)).collect();
        assert_eq!(
            at,
            vec![
                ("components/ads-client/src/lib.rs", 4),
                ("components/ads-client/src/lib.rs", 5),
                ("components/ads-client/src/worker.rs", 1)
            ],
            "{found:#?}"
        );
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                FFI,
                (
                    "components/ads-client/src/lib.rs",
                    include_str!("../../rules/uniffi-thin-ffi-layer/do.rs"),
                ),
                (
                    "components/ads-client/src/worker.rs",
                    "use crate::error::Result;\n\npub struct BackgroundWorker;\n",
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn needs_an_explicit_ffi_module_and_a_core_type() {
        let lib = "#[derive(uniffi::Object)]\npub struct Store { db: Db, worker: Worker }\n";
        let core = "use crate::StoreApiResult;\n";
        let found = check(
            &RULE,
            &[
                (
                    "components/store/Cargo.toml",
                    "[package]\nname = \"store\"\n[dependencies]\nuniffi = \"0.29\"\n",
                ),
                ("components/store/src/lib.rs", lib),
                ("components/store/src/db.rs", core),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
