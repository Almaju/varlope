//! Never panic in lifts, custom-type conversions or exported constructors
//!
//! Docs and examples: `rules/uniffi-no-panic-across-ffi/`.
//!
//! Looks for `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` and
//! `unimplemented!` where they run on a value the foreign side handed over,
//! or where the foreign caller has no way to get an error back:
//! - the conversion closures of `uniffi::custom_type!` / `custom_newtype!`
//!   and `impl UniffiCustomTypeConverter` / `FfiConverter` / `Lift` / `Lower`;
//! - exported constructors (`#[uniffi::constructor]`, or the Rust side of a
//!   UDL `constructor` without `[Throws]`) that take arguments and return
//!   `Self`, not a `Result`;
//! - default bodies of exported trait methods;
//! - `todo!()` / `unimplemented!()` anywhere in an exported function.

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;

use crate::helpers::uniffi::{self as api, effective_attrs, is_exported};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::workspace::is_component_src;

pub static RULE: Rule = Rule {
    id: "uniffi-no-panic-across-ffi",
    area: Area::Uniffi,
    title: "Never panic in lifts, custom-type conversions or exported constructors",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Converter traits whose methods run on foreign values.
const CONVERTERS: &[&str] = &[
    "UniffiCustomTypeConverter",
    "FfiConverter",
    "Lift",
    "Lower",
    "LiftReturn",
    "LowerReturn",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if !is_component_src(&file.path) || !api::is_shipped_api_file(&file.path) {
            continue;
        }
        let udl_constructors = udl_infallible_constructors(cx, api::crate_dir(&file.path));
        for item in syntax::all_items(&file.ast) {
            match item {
                syn::Item::Macro(m) => {
                    let name = syntax::macro_name(&m.mac);
                    if name == "custom_type" || name == "custom_newtype" {
                        custom_type(file, &m.mac, out);
                    }
                }
                syn::Item::Fn(f) if is_exported(&effective_attrs(&f.attrs)) => {
                    unfinished(file, &f.block, &f.sig.ident.to_string(), out);
                }
                syn::Item::Impl(i) => impl_block(file, i, &udl_constructors, out),
                syn::Item::Trait(t) if is_exported(&effective_attrs(&t.attrs)) => {
                    for it in &t.items {
                        let syn::TraitItem::Fn(f) = it else { continue };
                        let Some(body) = &f.default else { continue };
                        for (line, what) in panics(body.to_token_stream()) {
                            out.add(
                                &file.path,
                                line,
                                format!(
                                    "`{what}` in the default body of exported trait method `{}::{}`: when Kotlin/Swift/JS calls it, the app crashes; return an error or require implementers to provide it",
                                    t.ident, f.sig.ident
                                ),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Constructors of UDL interfaces without `[Throws]`: `(interface, fn name)`.
fn udl_infallible_constructors(cx: &Context, dir: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for file in cx.ws.files_with_ext(&["udl"]) {
        if api::crate_dir(&file.path) != dir {
            continue;
        }
        for item in udl::parse(&file.text) {
            if item.kind != UdlKind::Interface || item.has_attr("Error") || item.has_attr("Enum") {
                continue;
            }
            for m in &item.members {
                let constructor = m.name == "constructor" || m.has_attr("Name");
                if !constructor || m.has_attr("Throws") {
                    continue;
                }
                let name = m
                    .attrs
                    .split(',')
                    .find_map(|a| a.trim().strip_prefix("Name="))
                    .map(|n| n.trim().trim_matches('"').to_string())
                    .unwrap_or_else(|| "new".to_string());
                out.insert((item.name.clone(), name));
            }
        }
    }
    out
}

fn impl_block(
    file: &RustFile,
    i: &syn::ItemImpl,
    udl_constructors: &BTreeSet<(String, String)>,
    out: &mut Findings,
) {
    let self_name = syntax::type_ident(&i.self_ty).unwrap_or_default();
    if let Some((_, path, _)) = &i.trait_ {
        let is_converter = path
            .segments
            .last()
            .is_some_and(|s| CONVERTERS.contains(&s.ident.to_string().as_str()));
        if is_converter {
            for (line, what) in panics(i.to_token_stream()) {
                out.add(
                    &file.path,
                    line,
                    format!(
                        "`{what}` in the UniFFI conversion of `{self_name}`: a bad value from Kotlin/Swift/JS crashes the app; return an error instead"
                    ),
                );
            }
        }
        return;
    }
    let exported = is_exported(&effective_attrs(&i.attrs));
    for it in &i.items {
        let syn::ImplItem::Fn(f) = it else { continue };
        let attrs = effective_attrs(&f.attrs);
        let name = f.sig.ident.to_string();
        let constructor = (exported && syntax::has_attr(&attrs, "uniffi::constructor"))
            || udl_constructors.contains(&(self_name.clone(), name.clone()));
        // A constructor without arguments only panics on its own invariants.
        let takes_input = f
            .sig
            .inputs
            .iter()
            .any(|a| matches!(a, syn::FnArg::Typed(_)));
        if constructor && takes_input && !returns_result(&f.sig) {
            let found = panics(f.block.to_token_stream());
            if let Some((line, what)) = found.first() {
                let more = match found.len() {
                    1 => String::new(),
                    n => format!(" (and {} more)", n - 1),
                };
                out.add(
                    &file.path,
                    *line,
                    format!(
                        "`{what}`{more} in exported constructor `{self_name}::{name}`, which returns `Self`: bad input crashes the app instead of throwing; return `ApiResult<Self>`"
                    ),
                );
            }
        } else if exported || constructor {
            unfinished(file, &f.block, &format!("{self_name}::{name}"), out);
        }
    }
}

fn returns_result(sig: &syn::Signature) -> bool {
    match &sig.output {
        syn::ReturnType::Type(_, ty) => {
            syntax::type_ident(ty).is_some_and(|i| i.ends_with("Result"))
        }
        syn::ReturnType::Default => false,
    }
}

/// `todo!()` / `unimplemented!()` in an exported function.
fn unfinished(file: &RustFile, body: &syn::Block, name: &str, out: &mut Findings) {
    for (line, what) in panics(body.to_token_stream()) {
        if what == "todo!" || what == "unimplemented!" {
            out.add(
                &file.path,
                line,
                format!("`{what}` in exported `{name}`: calling it from Kotlin/Swift/JS crashes the app; implement it, return an error, or don't export it yet"),
            );
        }
    }
}

/// The conversion closures of `custom_type!(Name, Builtin, { try_lift: .., lower: .. })`.
fn custom_type(file: &RustFile, mac: &syn::Macro, out: &mut Findings) {
    let name = mac
        .tokens
        .clone()
        .into_iter()
        .next()
        .map(|t| t.to_string())
        .unwrap_or_default();
    for tt in mac.tokens.clone() {
        let TokenTree::Group(g) = tt else { continue };
        if g.delimiter() != Delimiter::Brace {
            continue;
        }
        let mut key = String::new();
        let mut entry = Vec::new();
        let mut flush = |key: &str, entry: &mut Vec<TokenTree>| {
            for (line, what) in panics(entry.drain(..).collect()) {
                out.add(
                    &file.path,
                    line,
                    format!(
                        "`{what}` in the `{key}` of custom type `{name}`: a bad value from Kotlin/Swift/JS crashes the app; return an error instead"
                    ),
                );
            }
        };
        let tokens: Vec<TokenTree> = g.stream().into_iter().collect();
        for (i, tt) in tokens.iter().enumerate() {
            match tt {
                TokenTree::Punct(p) if p.as_char() == ',' => {
                    flush(&key, &mut entry);
                    key.clear();
                }
                TokenTree::Ident(id)
                    if entry.is_empty()
                        && matches!(tokens.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':') =>
                {
                    key = id.to_string();
                }
                TokenTree::Punct(p)
                    if p.as_char() == ':' && entry.is_empty() && !key.is_empty() => {}
                other => entry.push(other.clone()),
            }
        }
        flush(&key, &mut entry);
    }
}

/// Panicking calls in tokens: `(line, "unwrap()" | "expect()" | "panic!" ..)`.
fn panics(stream: TokenStream) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    collect_panics(stream, &mut out);
    out
}

fn collect_panics(stream: TokenStream, out: &mut Vec<(usize, String)>) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (i, tt) in tokens.iter().enumerate() {
        match tt {
            TokenTree::Group(g) => collect_panics(g.stream(), out),
            TokenTree::Ident(id) => {
                let name = id.to_string();
                let prev_dot =
                    i > 0 && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '.');
                let next = tokens.get(i + 1);
                let call = matches!(next, Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis);
                let bang = matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == '!');
                let line = id.span().start().line;
                if prev_dot && call && (name == "unwrap" || name == "expect") {
                    out.push((line, format!("{name}()")));
                } else if bang
                    && matches!(
                        name.as_str(),
                        "panic" | "unreachable" | "todo" | "unimplemented"
                    )
                {
                    out.push((line, format!("{name}!")));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/x/src/lib.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/uniffi-no-panic-across-ffi/dont.rs"),
            )],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 12], "{found:#?}");
        assert!(found[0]
            .message
            .contains("`try_lift` of custom type `JsonValue`"));
        assert!(found[1].message.contains("constructor `Store::new`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/uniffi-no-panic-across-ffi/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_udl_constructors_without_throws() {
        let udl = (
            "components/x/src/x.udl",
            "interface Store {\n    constructor(string path);\n};\ninterface Other {\n    [Throws=E]\n    constructor(string path);\n};\n",
        );
        let src = "impl Store {\n    pub fn new(path: String) -> Self { Self(open(&path).unwrap()) }\n}\nimpl Other {\n    pub fn new(path: String) -> Self { Self(open(&path).unwrap()) }\n}\n";
        let found = check(&RULE, &[udl, (PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2], "{found:#?}");
    }

    #[test]
    fn flags_todo_in_exported_fns_and_trait_defaults() {
        let src = "#[uniffi::export]\npub fn later() -> u32 { todo!() }\n#[uniffi::export]\nimpl Store {\n    pub fn get(&self) -> u32 { self.x.lock().unwrap().get() }\n}\n#[uniffi::export(with_foreign)]\npub trait Engine: Send + Sync {\n    fn sync(&self) { unimplemented!() }\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 9], "{found:#?}");
    }

    #[test]
    fn ignores_constructors_without_input() {
        let src = "#[uniffi::export]\nimpl TestServer {\n    #[uniffi::constructor]\n    pub fn new() -> Self { Self(Key::generate().unwrap()) }\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }

    #[test]
    fn ignores_non_exported_code_and_tests() {
        let src = "uniffi::custom_type!(Guid, String, {\n    try_lift: |val| Ok(Guid::new(&val)),\n    lower: |obj| obj.to_string(),\n});\nimpl Store {\n    pub fn new() -> Self { Self(open().unwrap()) }\n}\n#[cfg(test)]\nmod tests {\n    uniffi::custom_type!(T, String, { try_lift: |v| Ok(v.parse().unwrap()), lower: |o| o });\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }
}
