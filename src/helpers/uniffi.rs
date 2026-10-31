//! Helpers shared by the uniffi rules.
//!
//! The main one is a structured view of a crate's UniFFI surface: the items
//! `crate::uniffi` finds in UDL files and proc-macros, with each member's
//! shape (field types and defaults, variant fields, argument lists, return and
//! error types) normalized so that a UDL declaration and the equivalent
//! proc-macro one compare equal. `uniffi-api-break-gate` diffs two of these.

use std::collections::{BTreeMap, HashMap};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind, UdlMember};
use crate::uniffi::{ApiKind, ApiSource, MemberKind};
use crate::workspace::is_test_path;

/// One exported item, as the foreign side sees it.
#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ApiKind,
    /// The exported name.
    pub name: String,
    pub path: String,
    pub line: usize,
    pub source: ApiSource,
    /// Declares the type (a struct, enum, trait or UDL declaration), as
    /// opposed to an `#[uniffi::export] impl` block that only adds methods.
    pub decl: bool,
    /// Implemented on the foreign side: a callback interface or a
    /// `with_foreign` trait.
    pub foreign: bool,
    /// An error whose variants carry no fields across the FFI.
    pub flat: bool,
    pub members: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub kind: MemberKind,
    /// The exported name (`new` for a UDL `constructor`).
    pub name: String,
    /// Where the member is declared (an impl block may sit in another file
    /// than its type).
    pub path: String,
    pub line: usize,
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Field(Field),
    Variant(Vec<Field>),
    Fn(FnSig),
}

/// A record field, a variant field or a function argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// Where it is declared (for UDL arguments, the line naming them).
    pub line: usize,
    /// Normalized type, see [`rust_type`] and [`udl_type`].
    pub ty: String,
    /// The UniFFI default, as written (`""` for a bare `#[uniffi(default)]`).
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub args: Vec<Field>,
    /// Normalized return type, `""` for none, `Self` for constructors.
    pub ret: String,
    /// The error type. A `Name<>` marker is a `Result` alias not resolved yet
    /// (see [`resolve_aliases`]); it compares as unknown.
    pub throws: Option<String>,
    pub is_async: bool,
}

impl FnSig {
    pub fn throws_known(&self) -> Option<&str> {
        self.throws.as_deref().filter(|t| !t.ends_with("<>"))
    }
}

// ---------------------------------------------------------------------------
// Which files and crates count.

/// UniFFI sources shipped to consumers: component `.rs` and `.udl` files, not
/// tests, vendored or generated code, nor the example component.
pub fn is_shipped_api_file(path: &str) -> bool {
    if !path.starts_with("components/")
        || path.starts_with("components/external/")
        || path.starts_with("components/example/")
        || path.contains("/generated/")
        || is_test_path(path)
    {
        return false;
    }
    (path.ends_with(".rs") || path.ends_with(".udl")) && path.contains("/src/")
}

/// The crate directory a source file belongs to: `components/suggest` for
/// `components/suggest/src/store.rs`.
pub fn crate_dir(path: &str) -> &str {
    match path.find("/src/") {
        Some(i) => &path[..i],
        None => path.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
    }
}

// ---------------------------------------------------------------------------
// Attributes.

/// The attributes of an item with every `#[cfg_attr(cond, a, b)]` expanded to
/// `#[a] #[b]` (UniFFI derives and exports often sit behind a feature).
pub fn effective_attrs(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
    let mut out = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("cfg_attr") {
            let parsed =
                attr.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated);
            if let Ok(metas) = parsed {
                for meta in metas.into_iter().skip(1) {
                    let mut inner = attr.clone();
                    inner.meta = meta;
                    out.push(inner);
                }
            }
            continue;
        }
        out.push(attr.clone());
    }
    out
}

/// Whether one of `attrs` is `#[uniffi::export]` (any arguments).
pub fn is_exported(attrs: &[syn::Attribute]) -> bool {
    syntax::has_attr(attrs, "uniffi::export")
}

/// Top-level `name = value` and bare `name` entries of an attribute's
/// argument list, with `name(..)` groups as `name` -> group text.
fn attr_entries(attr: &syn::Attribute) -> Vec<(String, Option<TokenStream>)> {
    let syn::Meta::List(list) = &attr.meta else {
        return Vec::new();
    };
    entries(list.tokens.clone())
}

fn entries(tokens: TokenStream) -> Vec<(String, Option<TokenStream>)> {
    let mut out = Vec::new();
    let mut parts: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in tokens {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => parts.push(Vec::new()),
            _ => parts.last_mut().expect("one part").push(tt),
        }
    }
    for part in parts {
        let mut it = part.into_iter();
        let Some(TokenTree::Ident(name)) = it.next() else {
            continue;
        };
        let rest: Vec<TokenTree> = it.collect();
        let value = match rest.first() {
            Some(TokenTree::Punct(p)) if p.as_char() == '=' => {
                Some(rest[1..].iter().cloned().collect::<TokenStream>())
            }
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
                Some(g.stream())
            }
            _ => None,
        };
        out.push((name.to_string(), value));
    }
    out
}

/// `default(a = 1, b)` in `#[uniffi::method(..)]`, `#[uniffi::constructor(..)]`
/// or `#[uniffi::export(..)]`: argument name -> default text.
fn arg_defaults(attrs: &[syn::Attribute]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for attr in attrs {
        let name = syntax::attr_name(attr);
        if !matches!(
            name.as_str(),
            "uniffi::method" | "uniffi::constructor" | "uniffi::export"
        ) {
            continue;
        }
        for (key, value) in attr_entries(attr) {
            if key != "default" {
                continue;
            }
            for (arg, v) in entries(value.unwrap_or_default()) {
                out.insert(
                    arg,
                    v.map(|v| syntax::compact(&v.to_string()))
                        .unwrap_or_default(),
                );
            }
        }
    }
    out
}

/// `name = "x"` in a UniFFI export/method/constructor attribute.
fn export_name(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        let name = syntax::attr_name(attr);
        if !name.starts_with("uniffi::") {
            continue;
        }
        for (key, value) in attr_entries(attr) {
            if key == "name" {
                let text = value?.to_string();
                return Some(text.trim_matches('"').to_string());
            }
        }
    }
    None
}

/// `#[uniffi(default)]` / `#[uniffi(default = x)]` on a field.
pub fn field_default(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs.iter().filter(|a| a.path().is_ident("uniffi")) {
        for (key, value) in attr_entries(attr) {
            if key == "default" {
                return Some(
                    value
                        .map(|v| syntax::compact(&v.to_string()))
                        .unwrap_or_default(),
                );
            }
        }
    }
    None
}

fn has_uniffi_flag(attrs: &[syn::Attribute], flag: &str) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("uniffi"))
        .any(|a| attr_entries(a).iter().any(|(k, _)| k == flag))
}

// ---------------------------------------------------------------------------
// Types.

/// A Rust type as the bindings see it: references, `Arc`/`Box` and paths
/// dropped, `&str` as `String`, maps as `Map<K, V>`, `()` as `""`.
pub fn rust_type(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Reference(r) => match &*r.elem {
            syn::Type::Slice(s) => format!("Vec<{}>", rust_type(&s.elem)),
            other => rust_type(other),
        },
        syn::Type::Paren(p) => rust_type(&p.elem),
        syn::Type::Group(g) => rust_type(&g.elem),
        syn::Type::Slice(s) => format!("Vec<{}>", rust_type(&s.elem)),
        syn::Type::Tuple(t) if t.elems.is_empty() => String::new(),
        syn::Type::TraitObject(t) => t
            .bounds
            .iter()
            .find_map(|b| match b {
                syn::TypeParamBound::Trait(t) => {
                    t.path.segments.last().map(|s| s.ident.to_string())
                }
                _ => None,
            })
            .unwrap_or_default(),
        syn::Type::Path(p) => {
            let Some(last) = p.path.segments.last() else {
                return String::new();
            };
            let ident = last.ident.to_string();
            let args: Vec<String> = syntax::type_args(ty).into_iter().map(rust_type).collect();
            match ident.as_str() {
                "Arc" | "Box" | "Rc" if args.len() == 1 => args[0].clone(),
                "str" => "String".into(),
                "HashMap" | "BTreeMap" | "IndexMap" => format!("Map<{}>", args.join(", ")),
                _ if args.is_empty() => ident,
                _ => format!("{ident}<{}>", args.join(", ")),
            }
        }
        other => syntax::type_string(other),
    }
}

/// A UDL type in the same normal form as [`rust_type`].
pub fn udl_type(text: &str) -> String {
    let t = text.trim();
    if let Some(inner) = t.strip_suffix('?') {
        return format!("Option<{}>", udl_type(inner));
    }
    if let Some(inner) = generic_arg(t, "sequence") {
        return format!("Vec<{}>", udl_type(inner));
    }
    if let Some(inner) = generic_arg(t, "record") {
        let parts = split_top(inner, ',');
        let parts: Vec<String> = parts.iter().map(|p| udl_type(p)).collect();
        return format!("Map<{}>", parts.join(", "));
    }
    match t {
        "string" | "DOMString" => "String".into(),
        "boolean" => "bool".into(),
        "bytes" => "Vec<u8>".into(),
        "timestamp" => "SystemTime".into(),
        "duration" => "Duration".into(),
        "void" => String::new(),
        other => other.to_string(),
    }
}

fn generic_arg<'a>(t: &'a str, name: &str) -> Option<&'a str> {
    t.strip_prefix(name)?
        .trim_start()
        .strip_prefix('<')?
        .strip_suffix('>')
}

/// Split on `sep` outside `<>`, `()`, `[]`.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(&text[start..]);
    }
    out
}

/// `(ok type, error type)` of a return type. `Result<T, E>` gives `E`; a
/// one-argument `*Result<T>` alias gives the `Name<>` marker.
fn split_result(ty: &syn::Type) -> (String, Option<String>) {
    if let syn::Type::Path(p) = ty {
        if let Some(last) = p.path.segments.last() {
            let ident = last.ident.to_string();
            let args = syntax::type_args(ty);
            if ident.ends_with("Result") && !args.is_empty() {
                let ok = rust_type(args[0]);
                return match args.get(1) {
                    Some(err) => (ok, Some(rust_type(err))),
                    None => (ok, Some(format!("{ident}<>"))),
                };
            }
        }
    }
    (rust_type(ty), None)
}

/// `type Name<T> = Result<T, E>` aliases in a file: name -> `E`.
pub fn result_aliases(file: &syn::File) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for item in syntax::all_items(file) {
        let syn::Item::Type(alias) = item else {
            continue;
        };
        let syn::Type::Path(target) = &*alias.ty else {
            continue;
        };
        if target.path.segments.last().map(|s| s.ident.to_string()) != Some("Result".into()) {
            continue;
        }
        let args = syntax::type_args(&alias.ty);
        let Some(err) = args.get(1) else { continue };
        let err_name = rust_type(err);
        // `type Result<T, E = Error> = Result<T, E>`: the default is the error.
        let resolved = alias
            .generics
            .params
            .iter()
            .find_map(|p| match p {
                syn::GenericParam::Type(t) if t.ident == err_name => {
                    Some(t.default.as_ref().map(rust_type))
                }
                _ => None,
            })
            .unwrap_or(Some(err_name));
        if let Some(err) = resolved {
            out.push((alias.ident.to_string(), err));
        }
    }
    out
}

/// Replace `Name<>` error markers with the aliased error type, when the
/// alias is known and unambiguous.
pub fn resolve_aliases(items: &mut [Item], aliases: &BTreeMap<String, Option<String>>) {
    for item in items {
        for member in &mut item.members {
            if let Shape::Fn(sig) = &mut member.shape {
                if let Some(marker) = sig.throws.as_ref().and_then(|t| t.strip_suffix("<>")) {
                    if let Some(Some(err)) = aliases.get(marker) {
                        sig.throws = Some(err.clone());
                    }
                }
            }
        }
    }
}

/// Collect aliases into a map, marking names that resolve to different
/// errors as ambiguous (`None`).
pub fn add_aliases(map: &mut BTreeMap<String, Option<String>>, found: Vec<(String, String)>) {
    for (name, err) in found {
        match map.get(&name) {
            Some(Some(existing)) if *existing != err => {
                map.insert(name, None);
            }
            Some(_) => {}
            None => {
                map.insert(name, Some(err));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Proc-macro items.

/// The exported items of a Rust file (proc-macros), outside test code.
pub fn rust_items(file: &RustFile) -> Vec<Item> {
    let mut out = Vec::new();
    for item in syntax::all_items(&file.ast) {
        if let Some(found) = rust_item(file, item) {
            if !file.in_test(found.line) {
                out.push(found);
            }
        }
    }
    out
}

fn base_item(file: &RustFile, kind: ApiKind, ident: &syn::Ident) -> Item {
    Item {
        kind,
        name: ident.to_string(),
        path: file.path.clone(),
        line: ident.span().start().line,
        source: ApiSource::ProcMacro,
        decl: true,
        foreign: false,
        flat: false,
        members: Vec::new(),
    }
}

fn rust_item(file: &RustFile, item: &syn::Item) -> Option<Item> {
    match item {
        syn::Item::Struct(s) => {
            let attrs = effective_attrs(&s.attrs);
            let kind = if syntax::has_derive(&attrs, "uniffi::Record") {
                ApiKind::Record
            } else if syntax::has_derive(&attrs, "uniffi::Object") {
                ApiKind::Object
            } else if syntax::has_derive(&attrs, "uniffi::Error") {
                ApiKind::Error
            } else {
                return None;
            };
            let mut out = base_item(file, kind, &s.ident);
            out.flat = kind == ApiKind::Error;
            if kind == ApiKind::Record {
                for f in &s.fields {
                    let Some(ident) = &f.ident else { continue };
                    out.members.push(Member {
                        kind: MemberKind::Field,
                        name: ident.to_string(),
                        path: file.path.clone(),
                        line: ident.span().start().line,
                        shape: Shape::Field(Field {
                            name: ident.to_string(),
                            line: ident.span().start().line,
                            ty: rust_type(&f.ty),
                            default: field_default(&effective_attrs(&f.attrs)),
                        }),
                    });
                }
            }
            Some(out)
        }
        syn::Item::Enum(e) => {
            let attrs = effective_attrs(&e.attrs);
            let kind = if syntax::has_derive(&attrs, "uniffi::Error") {
                ApiKind::Error
            } else if syntax::has_derive(&attrs, "uniffi::Enum") {
                ApiKind::Enum
            } else if syntax::has_derive(&attrs, "uniffi::Object") {
                ApiKind::Object
            } else {
                return None;
            };
            let mut out = base_item(file, kind, &e.ident);
            out.flat = kind == ApiKind::Error && has_uniffi_flag(&attrs, "flat_error");
            if kind != ApiKind::Object {
                for v in &e.variants {
                    let fields = v
                        .fields
                        .iter()
                        .enumerate()
                        .map(|(i, f)| Field {
                            line: f
                                .ident
                                .as_ref()
                                .map(|id| id.span().start().line)
                                .unwrap_or_else(|| file.line_of_node(f)),
                            name: f
                                .ident
                                .as_ref()
                                .map(|i| i.to_string())
                                .unwrap_or_else(|| i.to_string()),
                            ty: rust_type(&f.ty),
                            default: field_default(&effective_attrs(&f.attrs)),
                        })
                        .collect();
                    out.members.push(Member {
                        kind: MemberKind::Variant,
                        name: v.ident.to_string(),
                        path: file.path.clone(),
                        line: v.ident.span().start().line,
                        shape: Shape::Variant(fields),
                    });
                }
            }
            Some(out)
        }
        syn::Item::Fn(f) => {
            let attrs = effective_attrs(&f.attrs);
            if !is_exported(&attrs) {
                return None;
            }
            let name = export_name(&attrs).unwrap_or_else(|| f.sig.ident.to_string());
            let mut out = base_item(file, ApiKind::Function, &f.sig.ident);
            out.name = name.clone();
            out.line = file.line_of_node(&f.sig);
            out.members.push(Member {
                kind: MemberKind::Method,
                name,
                path: file.path.clone(),
                line: out.line,
                shape: Shape::Fn(rust_sig(&f.sig, &attrs, false, "")),
            });
            Some(out)
        }
        syn::Item::Impl(i) => {
            let attrs = effective_attrs(&i.attrs);
            if !is_exported(&attrs) || i.trait_.is_some() {
                return None;
            }
            let name = syntax::type_ident(&i.self_ty)?;
            let mut out = Item {
                kind: ApiKind::Object,
                name: name.clone(),
                path: file.path.clone(),
                line: file.line_of_node(&i.self_ty),
                source: ApiSource::ProcMacro,
                decl: false,
                foreign: false,
                flat: false,
                members: Vec::new(),
            };
            for it in &i.items {
                let syn::ImplItem::Fn(f) = it else { continue };
                if file.in_test(file.line_of_node(&f.sig)) {
                    continue;
                }
                let fattrs = effective_attrs(&f.attrs);
                let constructor = syntax::has_attr(&fattrs, "uniffi::constructor");
                out.members.push(Member {
                    kind: if constructor {
                        MemberKind::Constructor
                    } else {
                        MemberKind::Method
                    },
                    name: export_name(&fattrs).unwrap_or_else(|| f.sig.ident.to_string()),
                    path: file.path.clone(),
                    line: file.line_of_node(&f.sig),
                    shape: Shape::Fn(rust_sig(&f.sig, &fattrs, constructor, &name)),
                });
            }
            Some(out)
        }
        syn::Item::Trait(t) => {
            let attrs = effective_attrs(&t.attrs);
            let export = syntax::find_attr(&attrs, "uniffi::export")?;
            let args = syntax::attr_args(export);
            let callback = args.contains("callback_interface");
            let mut out = base_item(
                file,
                if callback {
                    ApiKind::CallbackInterface
                } else {
                    ApiKind::Trait
                },
                &t.ident,
            );
            out.foreign = callback || args.contains("with_foreign");
            for it in &t.items {
                let syn::TraitItem::Fn(f) = it else { continue };
                let fattrs = effective_attrs(&f.attrs);
                out.members.push(Member {
                    kind: MemberKind::Method,
                    name: f.sig.ident.to_string(),
                    path: file.path.clone(),
                    line: file.line_of_node(&f.sig),
                    shape: Shape::Fn(rust_sig(&f.sig, &fattrs, false, "")),
                });
            }
            Some(out)
        }
        syn::Item::Macro(m) => {
            let name = syntax::macro_name(&m.mac);
            if name != "custom_type" && name != "custom_newtype" {
                return None;
            }
            let first = m.mac.tokens.clone().into_iter().next()?;
            Some(Item {
                kind: ApiKind::Custom,
                name: first.to_string(),
                path: file.path.clone(),
                line: file.line_of_node(&m.mac),
                source: ApiSource::ProcMacro,
                decl: true,
                foreign: false,
                flat: false,
                members: Vec::new(),
            })
        }
        _ => None,
    }
}

/// The normalized signature of a Rust fn.
pub fn rust_sig(
    sig: &syn::Signature,
    attrs: &[syn::Attribute],
    constructor: bool,
    self_name: &str,
) -> FnSig {
    let defaults = arg_defaults(attrs);
    let args = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            syn::FnArg::Typed(t) => {
                let name = pat_name(&t.pat);
                Some(Field {
                    line: t.pat.span().start().line,
                    ty: rust_type(&t.ty),
                    default: defaults.get(&name).cloned(),
                    name,
                })
            }
            syn::FnArg::Receiver(_) => None,
        })
        .collect();
    let (mut ret, throws) = match &sig.output {
        syn::ReturnType::Default => (String::new(), None),
        syn::ReturnType::Type(_, ty) => split_result(ty),
    };
    if constructor && (ret == self_name || ret == "Self") {
        ret = "Self".into();
    }
    FnSig {
        args,
        ret,
        throws,
        is_async: sig.asyncness.is_some(),
    }
}

fn pat_name(pat: &syn::Pat) -> String {
    match pat {
        syn::Pat::Ident(i) => i.ident.to_string(),
        syn::Pat::Type(t) => pat_name(&t.pat),
        syn::Pat::Reference(r) => pat_name(&r.pat),
        other => syntax::compact(&syntax::tokens(other)),
    }
}

// ---------------------------------------------------------------------------
// UDL items.

/// The exported items of a UDL file.
pub fn udl_items(path: &str, text: &str) -> Vec<Item> {
    let typedefs = typedef_targets(text);
    let mut out = Vec::new();
    for item in udl::parse(text) {
        let mut base = Item {
            kind: ApiKind::Record,
            name: item.name.clone(),
            path: path.to_string(),
            line: item.line,
            source: ApiSource::Udl,
            decl: true,
            foreign: false,
            flat: false,
            members: Vec::new(),
        };
        match item.kind {
            UdlKind::Namespace => {
                for m in &item.members {
                    let Some((name, sig)) = udl_fn(m) else {
                        continue;
                    };
                    out.push(Item {
                        kind: ApiKind::Function,
                        name: name.clone(),
                        line: m.line,
                        members: vec![Member {
                            kind: MemberKind::Method,
                            name,
                            path: path.to_string(),
                            line: m.line,
                            shape: Shape::Fn(sig),
                        }],
                        ..base.clone()
                    });
                }
                continue;
            }
            UdlKind::Typedef => {
                // `typedef dictionary Foo;` only points at a proc-macro type.
                let builtin = typedefs
                    .get(&item.name)
                    .is_some_and(|target| !TYPEDEF_REFERENCES.contains(&target.as_str()));
                if !builtin || item.has_attr("External") || item.has_attr("Rust") {
                    continue;
                }
                base.kind = ApiKind::Custom;
            }
            UdlKind::Dictionary => {
                base.kind = ApiKind::Record;
                for m in &item.members {
                    let Some(field) = udl_field(&m.signature) else {
                        continue;
                    };
                    base.members.push(Member {
                        kind: MemberKind::Field,
                        name: field.name.clone(),
                        path: path.to_string(),
                        line: m.line,
                        shape: Shape::Field(field),
                    });
                }
            }
            UdlKind::Enum => {
                base.kind = if item.has_attr("Error") {
                    base.flat = true;
                    ApiKind::Error
                } else {
                    ApiKind::Enum
                };
                for m in &item.members {
                    base.members.push(Member {
                        kind: MemberKind::Variant,
                        name: m.name.clone(),
                        path: path.to_string(),
                        line: m.line,
                        shape: Shape::Variant(Vec::new()),
                    });
                }
            }
            UdlKind::Interface if item.has_attr("Error") || item.has_attr("Enum") => {
                base.kind = if item.has_attr("Error") {
                    ApiKind::Error
                } else {
                    ApiKind::Enum
                };
                for m in &item.members {
                    let args = m
                        .signature
                        .find('(')
                        .and_then(|open| {
                            let close = m.signature.rfind(')')?;
                            Some(udl_args(&m.signature[open + 1..close]))
                        })
                        .unwrap_or_default();
                    base.members.push(Member {
                        kind: MemberKind::Variant,
                        name: m.name.clone(),
                        path: path.to_string(),
                        line: m.line,
                        shape: Shape::Variant(args),
                    });
                }
            }
            UdlKind::Interface | UdlKind::CallbackInterface => {
                base.kind = if item.kind == UdlKind::CallbackInterface {
                    base.foreign = true;
                    ApiKind::CallbackInterface
                } else if item.has_attr("Trait") {
                    base.foreign = item.has_attr("WithForeign");
                    ApiKind::Trait
                } else {
                    ApiKind::Object
                };
                for m in &item.members {
                    let Some((name, sig)) = udl_fn(m) else {
                        continue;
                    };
                    let constructor = m.name == "constructor" || m.has_attr("Name");
                    base.members.push(Member {
                        kind: if constructor {
                            MemberKind::Constructor
                        } else {
                            MemberKind::Method
                        },
                        name,
                        path: path.to_string(),
                        line: m.line,
                        shape: Shape::Fn(sig),
                    });
                }
            }
            UdlKind::Other => continue,
        }
        out.push(base);
    }
    // Members start at their first attribute; point at the name instead.
    let lines: Vec<&str> = text.lines().collect();
    for item in &mut out {
        for m in &mut item.members {
            let start = m.line;
            let word = if m.kind == MemberKind::Constructor {
                "constructor"
            } else {
                m.name.as_str()
            };
            m.line = find_line(&lines, start, word);
            match &mut m.shape {
                Shape::Field(f) => f.line = m.line,
                Shape::Variant(fields) => {
                    for f in fields {
                        f.line = find_line(&lines, m.line, &f.name);
                    }
                }
                Shape::Fn(sig) => {
                    for f in &mut sig.args {
                        f.line = find_line(&lines, m.line, &f.name);
                    }
                }
            }
        }
        if let [only] = item.members.as_slice() {
            if item.kind == ApiKind::Function {
                item.line = only.line;
            }
        }
    }
    out
}

/// The first line from `from` (1-based) holding `word` as a whole word,
/// looking a few lines ahead; `from` if none does.
fn find_line(lines: &[&str], from: usize, word: &str) -> usize {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    for (offset, line) in lines
        .iter()
        .skip(from.saturating_sub(1))
        .take(12)
        .enumerate()
    {
        let code = line.split("//").next().unwrap_or("");
        let found = code.match_indices(word).any(|(at, _)| {
            let before = code[..at].chars().next_back();
            let after = code[at + word.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        });
        if found {
            return from + offset;
        }
    }
    from
}

/// Typedef targets that name a type declared elsewhere, not a builtin.
const TYPEDEF_REFERENCES: &[&str] = &[
    "dictionary",
    "record",
    "enum",
    "interface",
    "object",
    "trait",
    "trait_with_foreign",
    "callback",
    "custom",
    "extern",
];

fn typedef_targets(text: &str) -> HashMap<String, String> {
    let re = regex::Regex::new(r"typedef\s+([\w<>?, ]+?)\s+(\w+)\s*;").expect("valid regex");
    re.captures_iter(text)
        .map(|c| (c[2].to_string(), c[1].trim().to_string()))
        .collect()
}

/// `(exported name, signature)` of a UDL function, method or constructor.
fn udl_fn(m: &UdlMember) -> Option<(String, FnSig)> {
    let sig = &m.signature;
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    let before = sig[..open].trim();
    let args = udl_args(&sig[open + 1..close]);
    let mut throws = None;
    let mut is_async = false;
    let mut renamed = None;
    for attr in split_top(&m.attrs, ',') {
        let attr = attr.trim();
        if let Some(e) = attr.strip_prefix("Throws=") {
            throws = Some(e.trim().to_string());
        } else if attr == "Async" {
            is_async = true;
        } else if let Some(n) = attr.strip_prefix("Name=") {
            renamed = Some(n.trim().trim_matches('"').to_string());
        }
    }
    let (name, ret) = if before == "constructor" {
        (renamed.unwrap_or_else(|| "new".into()), "Self".to_string())
    } else {
        let (ret, name) = before.rsplit_once(' ')?;
        let name = name.trim().to_string();
        if m.has_attr("Name") {
            (renamed.unwrap_or(name), "Self".to_string())
        } else {
            (name, udl_type(ret))
        }
    };
    Some((
        name,
        FnSig {
            args,
            ret,
            throws,
            is_async,
        },
    ))
}

fn udl_args(text: &str) -> Vec<Field> {
    split_top(text, ',')
        .into_iter()
        .filter_map(udl_field)
        .collect()
}

/// `[ByRef] optional string? name = null` -> field.
fn udl_field(text: &str) -> Option<Field> {
    let mut t = text.trim();
    while let Some(rest) = t.strip_prefix('[') {
        let end = rest.find(']')?;
        t = rest[end + 1..].trim_start();
    }
    t = t.strip_prefix("optional ").unwrap_or(t).trim_start();
    t = t.strip_prefix("required ").unwrap_or(t).trim_start();
    let (decl, default) = match split_top(t, '=').as_slice() {
        [decl] => (*decl, None),
        [decl, default, ..] => (*decl, Some(default.trim().to_string())),
        [] => return None,
    };
    let (ty, name) = decl.trim().rsplit_once(' ')?;
    Some(Field {
        line: 0,
        name: name.trim().to_string(),
        ty: udl_type(ty),
        default,
    })
}

// ---------------------------------------------------------------------------
// Merging into a crate-level view.

/// Merge items by exported name: `#[uniffi::export] impl` blocks join the
/// type they add methods to; a member already seen (e.g. a cfg'd
/// alternative) is kept once.
pub fn merge(items: Vec<Item>) -> BTreeMap<String, Item> {
    let mut out: BTreeMap<String, Item> = BTreeMap::new();
    for item in items {
        match out.get_mut(&item.name) {
            None => {
                out.insert(item.name.clone(), item);
            }
            Some(existing) => {
                if item.decl && !existing.decl {
                    existing.kind = item.kind;
                    existing.path = item.path.clone();
                    existing.line = item.line;
                    existing.decl = true;
                    existing.foreign = item.foreign;
                    existing.flat = item.flat;
                    existing.source = item.source;
                }
                for m in item.members {
                    if !existing
                        .members
                        .iter()
                        .any(|e| e.name == m.name && same_slot(e.kind, m.kind))
                    {
                        existing.members.push(m);
                    }
                }
            }
        }
    }
    out
}

/// Members that occupy the same slot: same kind (constructor and method are
/// different slots).
pub fn same_slot(a: MemberKind, b: MemberKind) -> bool {
    a == b
}

/// Every type name mentioned in a normalized type: `Vec<Map<String, Foo>>`
/// gives `Vec`, `Map`, `String`, `Foo`.
pub fn type_names(ty: &str) -> impl Iterator<Item = &str> {
    ty.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

/// `(path, whole text)` of several files.
pub type Texts = Vec<(String, String)>;

/// Split a hand-written unified diff (the `.diff` examples under `rules/`)
/// into whole before/after texts per file: context and `-` lines make the
/// old file, context and `+` lines the new one. Files that are created or
/// deleted (`/dev/null`) are missing from one side.
pub fn texts_from_unified(diff: &str) -> (Texts, Texts) {
    struct File {
        path: String,
        old: Option<String>,
        new: Option<String>,
    }
    let lines: Vec<&str> = diff.lines().collect();
    let mut files: Vec<File> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let (Some(minus), Some(plus)) = (
            line.strip_prefix("--- "),
            lines.get(i + 1).and_then(|n| n.strip_prefix("+++ ")),
        ) {
            let path = plus
                .strip_prefix("b/")
                .or_else(|| minus.strip_prefix("a/"))
                .unwrap_or(plus);
            files.push(File {
                path: path.to_string(),
                old: (!minus.starts_with("/dev/null")).then(String::new),
                new: (!plus.starts_with("/dev/null")).then(String::new),
            });
            i += 2;
            continue;
        }
        i += 1;
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("@@") {
            continue;
        }
        let (to_old, to_new, text) = if let Some(t) = line.strip_prefix('+') {
            (false, true, t)
        } else if let Some(t) = line.strip_prefix('-') {
            (true, false, t)
        } else {
            (true, true, line.strip_prefix(' ').unwrap_or(line))
        };
        for (wanted, side) in [(to_old, &mut file.old), (to_new, &mut file.new)] {
            if let (true, Some(side)) = (wanted, side.as_mut()) {
                side.push_str(text);
                side.push('\n');
            }
        }
    }
    let side = |pick: fn(&File) -> &Option<String>| {
        files
            .iter()
            .filter_map(|f| pick(f).clone().map(|t| (f.path.clone(), t)))
            .collect()
    };
    (side(|f| &f.old), side(|f| &f.new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_hand_written_diffs() {
        let (old, new) = texts_from_unified(
            "--- a/x.rs\n+++ b/x.rs\n fn a() {}\n-fn b() {}\n+fn c() {}\n--- /dev/null\n+++ b/y.md\n+hi\n",
        );
        assert_eq!(
            old,
            vec![("x.rs".to_string(), "fn a() {}\nfn b() {}\n".to_string())]
        );
        assert_eq!(new[0].1, "fn a() {}\nfn c() {}\n");
        assert_eq!(new[1], ("y.md".to_string(), "hi\n".to_string()));
    }

    fn rust(src: &str) -> Vec<Item> {
        let file = RustFile::parse("components/x/src/lib.rs", src.to_string()).unwrap();
        rust_items(&file)
    }

    /// The signature of a member, with defaults reduced to "has one".
    fn sig(item: &Item, member: usize) -> FnSig {
        match &item.members[member].shape {
            Shape::Fn(s) => FnSig {
                args: s.args.iter().map(presence).collect(),
                ..s.clone()
            },
            other => panic!("not a fn: {other:?}"),
        }
    }

    fn presence(f: &Field) -> Field {
        Field {
            line: 0,
            default: f.default.as_ref().map(|_| String::new()),
            ..f.clone()
        }
    }

    #[test]
    fn udl_and_proc_macro_signatures_normalize_alike() {
        let udl = udl_items(
            "components/x/src/x.udl",
            r#"
namespace x {
    [Throws=XApiError]
    sequence<string> list([ByRef] string prefix, optional i64? limit = null);
};
dictionary Query { string keyword; record<string, i32>? weights = null; };
interface Store {
    [Throws=XApiError]
    constructor(string path);
    [Async]
    boolean ping();
};
"#,
        );
        let rs = rust(
            r#"
#[uniffi::export(default(limit = None))]
pub fn list(prefix: &str, limit: Option<i64>) -> Result<Vec<String>, XApiError> { todo!() }
#[derive(uniffi::Record)]
pub struct Query { pub keyword: String, #[uniffi(default = None)] pub weights: Option<HashMap<String, i32>> }
#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    pub fn new(path: String) -> ApiResult<Arc<Self>> { todo!() }
    pub async fn ping(&self) -> bool { todo!() }
}
"#,
        );
        let udl = merge(udl);
        let rs = merge(rs);
        assert_eq!(sig(&udl["list"], 0), sig(&rs["list"], 0));
        let field = |item: &Item| match &item.members[1].shape {
            Shape::Field(f) => presence(f),
            other => panic!("not a field: {other:?}"),
        };
        assert_eq!(field(&udl["Query"]), field(&rs["Query"]));
        let (u, r) = (&udl["Store"], &rs["Store"]);
        assert_eq!(u.members[0].name, "new");
        assert_eq!(sig(u, 0).ret, "Self");
        assert_eq!(sig(r, 0).ret, "Self");
        assert_eq!(sig(r, 0).throws.as_deref(), Some("ApiResult<>"));
        assert_eq!(sig(u, 1), sig(r, 1));
    }

    #[test]
    fn resolves_result_aliases() {
        let file: syn::File = syn::parse_quote! {
            pub type ApiResult<T> = std::result::Result<T, XApiError>;
            pub type Result<T, E = XError> = std::result::Result<T, E>;
        };
        assert_eq!(
            result_aliases(&file),
            vec![
                ("ApiResult".to_string(), "XApiError".to_string()),
                ("Result".to_string(), "XError".to_string())
            ]
        );
    }

    #[test]
    fn reads_cfg_attr_exports_and_flat_errors() {
        let items = rust(
            r#"
#[cfg_attr(feature = "t", uniffi::export(with_foreign))]
pub trait Telemetry: Send + Sync { fn report(&self, x: String); }
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum E { #[error("x")] A { reason: String } }
"#,
        );
        assert_eq!(items[0].kind, ApiKind::Trait);
        assert!(items[0].foreign);
        assert!(items[1].flat);
    }
}
