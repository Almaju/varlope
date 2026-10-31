//! Exported fallible functions return `ApiResult` via `#[handle_error]`; internal code uses the crate `Error`
//!
//! Docs and examples: `rules/uniffi-handle-error/`.
//!
//! Only crates that already convert errors with `error_support` (they
//! `impl GetErrorHandling`) are checked. In those it flags:
//! - an exported function that builds the public error by hand instead of
//!   returning the internal error through `#[handle_error(..)]` (functions
//!   that only forward another exported function's `ApiResult` are fine);
//! - a non-exported function returning the public error type, unless a Rust
//!   implementation of a foreign trait (which returns it by design) calls it;
//! - `impl From<X> for <ExternalError>`, except UniFFI's callback error and
//!   the `anyhow::Error` adapter bridged sync engines need.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::uniffi::{self as api, effective_attrs, is_exported};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::workspace::is_component_src;

pub static RULE: Rule = Rule {
    id: "uniffi-handle-error",
    area: Area::Uniffi,
    title: "Exported fallible functions return `ApiResult` via `#[handle_error]`; internal code uses the crate `Error`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let mut crates: BTreeMap<&str, Vec<&RustFile>> = BTreeMap::new();
    for file in cx.ws.rust_files() {
        if is_component_src(&file.path) && api::is_shipped_api_file(&file.path) {
            crates
                .entry(api::crate_dir(&file.path))
                .or_default()
                .push(file);
        }
    }
    for (dir, files) in crates {
        let external = external_errors(&files);
        if external.is_empty() {
            continue; // the crate doesn't use error_support's conversion
        }
        let mut public_results: BTreeSet<String> = BTreeSet::new();
        for file in &files {
            for (alias, err) in api::result_aliases(&file.ast) {
                if external.contains_key(&err) {
                    public_results.insert(alias);
                }
            }
        }
        let udl = udl_throwing(cx, dir);
        let foreign_callees = foreign_impl_callees(cx, dir, &files);
        let crate_info = Crate {
            external: &external,
            public_results: &public_results,
            udl: &udl,
            foreign_callees: &foreign_callees,
        };
        for file in files {
            crate_info.check_file(file, out);
        }
    }
}

/// The `ExternalError` types of the crate's `impl GetErrorHandling` blocks,
/// with the internal error each converts from.
fn external_errors(files: &[&RustFile]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for file in files {
        for item in syntax::all_items(&file.ast) {
            let syn::Item::Impl(i) = item else { continue };
            let Some((_, path, _)) = &i.trait_ else {
                continue;
            };
            if !syntax::path_ends_with(path, "GetErrorHandling") {
                continue;
            }
            for it in &i.items {
                if let syn::ImplItem::Type(t) = it {
                    if t.ident == "ExternalError" {
                        out.insert(api::rust_type(&t.ty), api::rust_type(&i.self_ty));
                    }
                }
            }
        }
    }
    out
}

/// Throwing functions the crate's UDL files declare: `(interface or "", fn)`.
/// Constructors are `new` (or their `[Name=..]`).
fn udl_throwing(cx: &Context, dir: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for file in cx.ws.files_with_ext(&["udl"]) {
        if api::crate_dir(&file.path) != dir {
            continue;
        }
        for item in udl::parse(&file.text) {
            let owner = match item.kind {
                UdlKind::Namespace => String::new(),
                UdlKind::Interface if !item.has_attr("Error") && !item.has_attr("Enum") => {
                    item.name.clone()
                }
                _ => continue,
            };
            for m in item.members.iter().filter(|m| m.has_attr("Throws")) {
                let name = if m.name == "constructor" {
                    "new".to_string()
                } else {
                    m.name.clone()
                };
                out.insert((owner.clone(), name));
            }
        }
    }
    out
}

/// Names of the functions and methods called from Rust implementations of
/// foreign traits (`with_foreign`, callback interfaces), which must speak the
/// public error type.
fn foreign_impl_callees(cx: &Context, dir: &str, files: &[&RustFile]) -> BTreeSet<String> {
    let mut foreign: BTreeSet<String> = BTreeSet::new();
    let typedef = regex::Regex::new(r"typedef\s+(?:trait_with_foreign|callback)\s+(\w+)")
        .expect("valid regex");
    for file in cx.ws.files_with_ext(&["udl"]) {
        if api::crate_dir(&file.path) != dir {
            continue;
        }
        for item in udl::parse(&file.text) {
            if item.kind == UdlKind::CallbackInterface || item.has_attr("WithForeign") {
                foreign.insert(item.name.clone());
            }
        }
        for c in typedef.captures_iter(&file.text) {
            foreign.insert(c[1].to_string());
        }
    }
    for file in files {
        for item in api::rust_items(file) {
            if item.foreign {
                foreign.insert(item.name);
            }
        }
    }
    let mut out = BTreeSet::new();
    for file in files {
        for item in syntax::all_items(&file.ast) {
            let syn::Item::Impl(i) = item else { continue };
            let Some((_, path, _)) = &i.trait_ else {
                continue;
            };
            let Some(last) = path.segments.last() else {
                continue;
            };
            if !foreign.contains(&last.ident.to_string()) {
                continue;
            }
            collect_idents(quote::ToTokens::to_token_stream(i), &mut out);
        }
    }
    out
}

fn collect_idents(stream: proc_macro2::TokenStream, out: &mut BTreeSet<String>) {
    for tt in stream {
        match tt {
            proc_macro2::TokenTree::Group(g) => collect_idents(g.stream(), out),
            proc_macro2::TokenTree::Ident(i) => {
                out.insert(i.to_string());
            }
            _ => {}
        }
    }
}

/// The first of `names` the tokens build from a stringified error:
/// `XApiError::Other { reason: e.to_string() }`, `XApiError::Unexpected(format!(..))`.
/// A specific variant built without a message (`XApiError::InvalidKey`) is a
/// deliberate mapping, not a bypass.
fn builds_any(stream: proc_macro2::TokenStream, names: &BTreeSet<String>) -> Option<String> {
    use proc_macro2::TokenTree;
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (i, tt) in tokens.iter().enumerate() {
        match tt {
            TokenTree::Group(g) => {
                if let Some(found) = builds_any(g.stream(), names) {
                    return Some(found);
                }
            }
            TokenTree::Ident(id) if names.contains(&id.to_string()) => {
                // `Name :: Variant { .. }` or `Name :: Variant ( .. )`
                let fields = match (tokens.get(i + 1), tokens.get(i + 3), tokens.get(i + 4)) {
                    (
                        Some(TokenTree::Punct(p)),
                        Some(TokenTree::Ident(_)),
                        Some(TokenTree::Group(g)),
                    ) if p.as_char() == ':' => Some(g.stream()),
                    _ => None,
                };
                if fields.is_some_and(stringifies) {
                    return Some(id.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether tokens call `.to_string()` or `format!`.
fn stringifies(stream: proc_macro2::TokenStream) -> bool {
    stream.into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Group(g) => stringifies(g.stream()),
        proc_macro2::TokenTree::Ident(i) => i == "to_string" || i == "format",
        _ => false,
    })
}

struct Crate<'a> {
    /// External error -> the internal error it is converted from.
    external: &'a BTreeMap<String, String>,
    /// `Result` aliases whose error is an external error (`ApiResult`).
    public_results: &'a BTreeSet<String>,
    udl: &'a BTreeSet<(String, String)>,
    foreign_callees: &'a BTreeSet<String>,
}

impl Crate<'_> {
    fn check_file(&self, file: &RustFile, out: &mut Findings) {
        for item in syntax::all_items(&file.ast) {
            match item {
                syn::Item::Fn(f) => {
                    let attrs = effective_attrs(&f.attrs);
                    let exported = is_exported(&attrs)
                        || self.udl.contains(&(String::new(), f.sig.ident.to_string()));
                    self.check_fn(file, &f.sig, &f.block, &attrs, exported, out);
                }
                syn::Item::Impl(i) => self.check_impl(file, i, out),
                _ => {}
            }
        }
    }

    fn check_impl(&self, file: &RustFile, i: &syn::ItemImpl, out: &mut Findings) {
        if let Some((_, path, _)) = &i.trait_ {
            self.check_from_impl(file, i, path, out);
            return; // trait methods follow the trait's signature
        }
        let exported_impl = is_exported(&effective_attrs(&i.attrs));
        let self_name = syntax::type_ident(&i.self_ty).unwrap_or_default();
        for it in &i.items {
            let syn::ImplItem::Fn(f) = it else { continue };
            let attrs = effective_attrs(&f.attrs);
            let exported = exported_impl
                || self
                    .udl
                    .contains(&(self_name.clone(), f.sig.ident.to_string()));
            self.check_fn(file, &f.sig, &f.block, &attrs, exported, out);
        }
    }

    fn check_fn(
        &self,
        file: &RustFile,
        sig: &syn::Signature,
        body: &syn::Block,
        attrs: &[syn::Attribute],
        exported: bool,
        out: &mut Findings,
    ) {
        let syn::ReturnType::Type(_, ret) = &sig.output else {
            return;
        };
        if syntax::has_attr(attrs, "handle_error") || !self.is_public_result(ret) {
            return;
        }
        let name = sig.ident.to_string();
        let line = file.line_of_node(&sig.ident);
        if exported {
            let tokens = quote::ToTokens::to_token_stream(body);
            let names: BTreeSet<String> = self.external.keys().cloned().collect();
            if let Some(error) = builds_any(tokens, &names) {
                let internal = &self.external[&error];
                out.add(
                    &file.path,
                    line,
                    format!(
                        "exported `{name}` converts its error to `{error}` by hand, so it skips `get_error_handling` and is never logged or reported; return the internal error and add `#[handle_error({internal})]`"
                    ),
                );
            }
        } else if !self.foreign_callees.contains(&name) {
            out.add(
                &file.path,
                line,
                format!(
                    "`{name}` is not exported but returns the public error type; return the crate's internal error and convert at the exported boundary with `#[handle_error(..)]`"
                ),
            );
        }
    }

    /// `ApiResult<T>`, or `Result<T, SomeApiError>` with the crate's external error.
    fn is_public_result(&self, ty: &syn::Type) -> bool {
        let Some(ident) = syntax::type_ident(ty) else {
            return false;
        };
        if self.public_results.contains(&ident) {
            return true;
        }
        if ident == "Result" {
            if let Some(err) = syntax::type_args(ty).get(1) {
                return self.external.contains_key(&api::rust_type(err));
            }
        }
        false
    }

    /// `impl From<X> for <ExternalError>` builds the public error without
    /// going through `get_error_handling`.
    fn check_from_impl(
        &self,
        file: &RustFile,
        i: &syn::ItemImpl,
        trait_path: &syn::Path,
        out: &mut Findings,
    ) {
        let Some(last) = trait_path.segments.last() else {
            return;
        };
        if last.ident != "From" {
            return;
        }
        let target = api::rust_type(&i.self_ty);
        let Some(internal) = self.external.get(&target) else {
            return;
        };
        let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
            return;
        };
        let Some(syn::GenericArgument::Type(source)) = args.args.first() else {
            return;
        };
        let short = api::rust_type(source);
        let source = syntax::type_string(source);
        // UniFFI requires the first for callback interfaces; bridged sync
        // engines speak `anyhow` and need the second.
        if short == "UnexpectedUniFFICallbackError" || source == "anyhow::Error" {
            return;
        }
        let fix = if &source == internal {
            format!("put `#[handle_error({internal})]` on the exported functions instead")
        } else {
            format!("convert it into `{internal}` and let `#[handle_error]` map it")
        };
        out.add(
            &file.path,
            file.line_of_node(&i.self_ty),
            format!(
                "`impl From<{source}> for {target}` builds the public error without `get_error_handling`, so it is never logged or reported; {fix}"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/store/src/lib.rs";
    const ERRORS: (&str, &str) = (
        "components/store/src/error.rs",
        "pub type ApiResult<T> = std::result::Result<T, StoreApiError>;\n\
         impl GetErrorHandling for Error {\n    type ExternalError = StoreApiError;\n    fn get_error_handling(&self) -> ErrorHandling<StoreApiError> { todo!() }\n}\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                ERRORS,
                (
                    PATH,
                    include_str!("../../rules/uniffi-handle-error/dont.rs"),
                ),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 11], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                ERRORS,
                (PATH, include_str!("../../rules/uniffi-handle-error/do.rs")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_crates_without_error_support() {
        let src = "#[uniffi::export]\npub fn f() -> Result<(), StoreApiError> { Ok(()) }\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }

    #[test]
    fn checks_udl_declared_methods() {
        let udl = (
            "components/store/src/store.udl",
            "interface Store {\n    [Throws=StoreApiError]\n    void wipe();\n    void ping();\n};\n",
        );
        let src = "impl Store {\n    pub fn wipe(&self) -> ApiResult<()> { self.db.wipe().map_err(|e| StoreApiError::Other(e.to_string())) }\n    #[handle_error(Error)]\n    pub fn other(&self) -> ApiResult<()> { Ok(()) }\n    fn helper(&self) -> ApiResult<()> { Ok(()) }\n}\n";
        let found = check(&RULE, &[ERRORS, udl, (PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 5], "{found:#?}");
        assert!(found[0].message.starts_with("exported `wipe`"));
        assert!(found[1].message.starts_with("`helper` is not exported"));
    }

    #[test]
    fn allows_specific_error_mappings() {
        let src = "#[uniffi::export]\npub fn check(key: &str) -> ApiResult<bool> {\n    let k = Key::new(key).map_err(|_| StoreApiError::InvalidKey)?;\n    Ok(k.ok())\n}\n";
        assert!(check(&RULE, &[ERRORS, (PATH, src)]).is_empty());
    }

    #[test]
    fn allows_forwarding_exported_results() {
        let src = "#[uniffi::export]\npub fn open(path: String) -> ApiResult<Arc<Store>> {\n    Ok(Arc::new(Store::new(path)?))\n}\n";
        assert!(check(&RULE, &[ERRORS, (PATH, src)]).is_empty());
    }

    #[test]
    fn allows_helpers_of_foreign_trait_impls() {
        let src = "#[uniffi::export(with_foreign)]\npub trait KeyManager: Send + Sync {\n    fn get_key(&self) -> ApiResult<Vec<u8>>;\n}\nfn fetch() -> ApiResult<Vec<u8>> { todo!() }\nimpl KeyManager for Nss {\n    fn get_key(&self) -> ApiResult<Vec<u8>> { fetch() }\n}\n";
        assert!(check(&RULE, &[ERRORS, (PATH, src)]).is_empty());
    }

    #[test]
    fn allows_the_callback_error_conversion_and_internal_results() {
        let src = "impl From<uniffi::UnexpectedUniFFICallbackError> for StoreApiError {\n    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self { todo!() }\n}\nimpl From<anyhow::Error> for StoreApiError {\n    fn from(e: anyhow::Error) -> Self { todo!() }\n}\nfn internal() -> Result<()> { Ok(()) }\n";
        assert!(check(&RULE, &[ERRORS, (PATH, src)]).is_empty());
    }
}
