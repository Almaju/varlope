//! The UniFFI surface of a crate, from both UDL files and proc-macros, in one
//! model: what Kotlin/Swift/JS callers see.

use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::workspace::{Package, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApiKind {
    Record,
    Enum,
    Object,
    Error,
    Function,
    Trait,
    CallbackInterface,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiSource {
    Udl,
    ProcMacro,
}

#[derive(Clone, Debug)]
pub struct ApiItem {
    pub kind: ApiKind,
    pub name: String,
    pub path: String,
    pub line: usize,
    pub has_doc: bool,
    pub source: ApiSource,
    pub members: Vec<ApiMember>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Variant,
    Method,
    Constructor,
}

#[derive(Clone, Debug)]
pub struct ApiMember {
    pub kind: MemberKind,
    pub name: String,
    /// Normalized signature: `title: Option<String> = None`, `fn get(id: &str) -> ApiResult<Item>`.
    pub signature: String,
    pub line: usize,
    pub has_doc: bool,
    pub has_default: bool,
}

/// Exported items declared in one UDL file.
pub fn from_udl(path: &str, text: &str) -> Vec<ApiItem> {
    let mut out = Vec::new();
    for item in udl::parse(text) {
        let kind = match item.kind {
            UdlKind::Dictionary => ApiKind::Record,
            UdlKind::Enum if item.has_attr("Error") => ApiKind::Error,
            UdlKind::Enum => ApiKind::Enum,
            UdlKind::Interface if item.has_attr("Error") => ApiKind::Error,
            UdlKind::Interface if item.has_attr("Enum") => ApiKind::Enum,
            UdlKind::Interface if item.has_attr("Trait") => ApiKind::Trait,
            UdlKind::Interface => ApiKind::Object,
            UdlKind::CallbackInterface => ApiKind::CallbackInterface,
            UdlKind::Namespace => {
                for m in &item.members {
                    out.push(ApiItem {
                        kind: ApiKind::Function,
                        name: m.name.clone(),
                        path: path.to_string(),
                        line: m.line,
                        has_doc: m.has_doc,
                        source: ApiSource::Udl,
                        members: Vec::new(),
                    });
                }
                continue;
            }
            UdlKind::Typedef
                if !item.attrs.contains("External") && !item.attrs.contains("Rust") =>
            {
                ApiKind::Custom
            }
            _ => continue,
        };
        let members = item
            .members
            .iter()
            .map(|m| ApiMember {
                kind: match kind {
                    ApiKind::Record => MemberKind::Field,
                    ApiKind::Enum | ApiKind::Error => MemberKind::Variant,
                    _ if m.name == "constructor" || m.has_attr("Name") => MemberKind::Constructor,
                    _ => MemberKind::Method,
                },
                name: m.name.clone(),
                signature: format!("{} {}", m.attrs, m.signature).trim().to_string(),
                line: m.line,
                has_doc: m.has_doc,
                has_default: m.has_default(),
            })
            .collect();
        out.push(ApiItem {
            kind,
            name: item.name,
            path: path.to_string(),
            line: item.line,
            has_doc: item.has_doc,
            source: ApiSource::Udl,
            members,
        });
    }
    out
}

/// Exported items declared with UniFFI proc-macros in one Rust file.
pub fn from_rust(file: &RustFile) -> Vec<ApiItem> {
    let mut out = Vec::new();
    for item in syntax::all_items(&file.ast) {
        match item {
            syn::Item::Struct(s) => {
                let kind = if syntax::has_derive(&s.attrs, "uniffi::Record") {
                    ApiKind::Record
                } else if syntax::has_derive(&s.attrs, "uniffi::Object") {
                    ApiKind::Object
                } else if syntax::has_derive(&s.attrs, "uniffi::Error") {
                    ApiKind::Error
                } else {
                    continue;
                };
                // An Object's fields are private state, not API.
                let fields = if kind == ApiKind::Object {
                    None
                } else {
                    Some(&s.fields)
                };
                let members = fields
                    .into_iter()
                    .flatten()
                    .filter_map(|f| {
                        let name = f.ident.as_ref()?.to_string();
                        let default = syntax::find_attr(&f.attrs, "uniffi")
                            .map(syntax::attr_args)
                            .filter(|a| a.contains("default"));
                        let ty = syntax::type_string(&f.ty);
                        Some(ApiMember {
                            kind: MemberKind::Field,
                            signature: match &default {
                                Some(d) => format!("{name}: {ty} [{}]", syntax::compact(d)),
                                None => format!("{name}: {ty}"),
                            },
                            name,
                            line: file.line_of_node(f),
                            has_doc: syntax::doc(&f.attrs).is_some(),
                            has_default: default.is_some(),
                        })
                    })
                    .collect();
                out.push(item_of(file, kind, &s.ident, &s.attrs, s, members));
            }
            syn::Item::Enum(e) => {
                let kind = if syntax::has_derive(&e.attrs, "uniffi::Error") {
                    ApiKind::Error
                } else if syntax::has_derive(&e.attrs, "uniffi::Enum") {
                    ApiKind::Enum
                } else if syntax::has_derive(&e.attrs, "uniffi::Object") {
                    ApiKind::Object
                } else {
                    continue;
                };
                let members = e
                    .variants
                    .iter()
                    .map(|v| ApiMember {
                        kind: MemberKind::Variant,
                        name: v.ident.to_string(),
                        signature: syntax::compact(&syntax::tokens(&strip_attrs_variant(v))),
                        line: file.line_of_node(v),
                        has_doc: syntax::doc(&v.attrs).is_some(),
                        has_default: false,
                    })
                    .collect();
                out.push(item_of(file, kind, &e.ident, &e.attrs, e, members));
            }
            syn::Item::Fn(f) if syntax::has_attr(&f.attrs, "uniffi::export") => {
                out.push(ApiItem {
                    kind: ApiKind::Function,
                    name: f.sig.ident.to_string(),
                    path: file.path.clone(),
                    line: file.line_of_node(&f.sig),
                    has_doc: syntax::doc(&f.attrs).is_some(),
                    source: ApiSource::ProcMacro,
                    members: vec![fn_member(file, &f.sig, &f.attrs, MemberKind::Method)],
                });
            }
            syn::Item::Impl(i) if syntax::has_attr(&i.attrs, "uniffi::export") => {
                let Some(name) = syntax::type_ident(&i.self_ty) else {
                    continue;
                };
                let methods = i.items.iter().filter_map(|it| match it {
                    syn::ImplItem::Fn(f)
                        if matches!(f.vis, syn::Visibility::Public(_)) || i.trait_.is_some() =>
                    {
                        let kind = if syntax::has_attr(&f.attrs, "uniffi::constructor") {
                            MemberKind::Constructor
                        } else {
                            MemberKind::Method
                        };
                        Some(fn_member(file, &f.sig, &f.attrs, kind))
                    }
                    _ => None,
                });
                let members: Vec<ApiMember> = methods.collect();
                // Methods attach to the Object declared elsewhere; keep them on
                // an item of the same name so a crate-level view can merge them.
                out.push(ApiItem {
                    kind: ApiKind::Object,
                    name,
                    path: file.path.clone(),
                    line: file.line_of_node(&i.self_ty),
                    has_doc: true, // documented on the type
                    source: ApiSource::ProcMacro,
                    members,
                });
            }
            syn::Item::Trait(t) if syntax::has_attr(&t.attrs, "uniffi::export") => {
                let args = syntax::find_attr(&t.attrs, "uniffi::export")
                    .map(syntax::attr_args)
                    .unwrap_or_default();
                let kind = if args.contains("callback_interface") {
                    ApiKind::CallbackInterface
                } else {
                    ApiKind::Trait
                };
                let members = t
                    .items
                    .iter()
                    .filter_map(|it| match it {
                        syn::TraitItem::Fn(f) => {
                            Some(fn_member(file, &f.sig, &f.attrs, MemberKind::Method))
                        }
                        _ => None,
                    })
                    .collect();
                out.push(item_of(file, kind, &t.ident, &t.attrs, t, members));
            }
            syn::Item::Macro(m) => {
                let name = syntax::macro_name(&m.mac);
                if name == "custom_type" || name == "custom_newtype" {
                    if let Some(first) = m.mac.tokens.clone().into_iter().next() {
                        out.push(ApiItem {
                            kind: ApiKind::Custom,
                            name: first.to_string(),
                            path: file.path.clone(),
                            line: file.line_of_node(&m.mac),
                            has_doc: syntax::doc(&m.attrs).is_some(),
                            source: ApiSource::ProcMacro,
                            members: Vec::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn item_of<T: syn::spanned::Spanned>(
    file: &RustFile,
    kind: ApiKind,
    ident: &syn::Ident,
    attrs: &[syn::Attribute],
    node: &T,
    members: Vec<ApiMember>,
) -> ApiItem {
    let _ = node;
    ApiItem {
        kind,
        name: ident.to_string(),
        path: file.path.clone(),
        line: ident.span().start().line,
        has_doc: syntax::doc(attrs).is_some(),
        source: ApiSource::ProcMacro,
        members,
    }
}

fn fn_member(
    file: &RustFile,
    sig: &syn::Signature,
    attrs: &[syn::Attribute],
    kind: MemberKind,
) -> ApiMember {
    let args: Vec<String> = sig
        .inputs
        .iter()
        .map(|a| match a {
            syn::FnArg::Receiver(r) => syntax::compact(&syntax::tokens(r)),
            syn::FnArg::Typed(t) => format!(
                "{}: {}",
                syntax::compact(&syntax::tokens(&t.pat)),
                syntax::type_string(&t.ty)
            ),
        })
        .collect();
    let ret = syntax::return_type(sig);
    let asyncness = if sig.asyncness.is_some() {
        "async "
    } else {
        ""
    };
    ApiMember {
        kind,
        name: sig.ident.to_string(),
        signature: format!(
            "{asyncness}fn {}({}){}",
            sig.ident,
            args.join(", "),
            if ret.is_empty() {
                String::new()
            } else {
                format!(" -> {ret}")
            }
        ),
        line: file.line_of_node(sig),
        has_doc: syntax::doc(attrs).is_some(),
        has_default: false,
    }
}

fn strip_attrs_variant(v: &syn::Variant) -> syn::Variant {
    let mut v = v.clone();
    v.attrs.clear();
    if let syn::Fields::Named(named) = &mut v.fields {
        for f in named.named.iter_mut() {
            f.attrs.clear();
        }
    }
    v
}

/// The UniFFI surface of one package: its UDL files plus its proc-macro items.
pub fn package_api(ws: &Workspace, package: &Package) -> Vec<ApiItem> {
    let mut out = Vec::new();
    for file in ws.package_files(package) {
        if crate::workspace::is_test_path(&file.path) {
            continue;
        }
        if file.extension() == "udl" {
            out.extend(from_udl(&file.path, &file.text));
        } else if let Some(rust) = ws.rust(&file.path) {
            out.extend(
                from_rust(rust)
                    .into_iter()
                    .filter(|i| !rust.in_test(i.line)),
            );
        }
    }
    out
}

/// Every package that exposes a UniFFI surface (has a `.udl` or uses `uniffi::`).
pub fn uniffi_packages(ws: &Workspace) -> Vec<&Package> {
    ws.packages()
        .iter()
        .filter(|p| p.dependencies().iter().any(|d| d.name == "uniffi"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proc_macro_items() {
        let src = r#"
/// A record.
#[derive(uniffi::Record)]
pub struct Query {
    pub keyword: String,
    #[uniffi(default = None)]
    pub limit: Option<i32>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ApiError {
    #[error("x")]
    Other { reason: String },
}

#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    pub fn new(path: String) -> Self { todo!() }
    pub fn get(&self, id: &str) -> ApiResult<Option<Item>> { todo!() }
    fn private(&self) {}
}

#[uniffi::export(callback_interface)]
pub trait Logger { fn log(&self, msg: String); }

uniffi::custom_type!(Guid, String);
"#;
        let file = RustFile::parse("components/x/src/lib.rs", src.to_string()).unwrap();
        let items = from_rust(&file);
        let summary: Vec<_> = items
            .iter()
            .map(|i| (i.kind, i.name.as_str(), i.members.len()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (ApiKind::Record, "Query", 2),
                (ApiKind::Error, "ApiError", 1),
                (ApiKind::Object, "Store", 2),
                (ApiKind::CallbackInterface, "Logger", 1),
                (ApiKind::Custom, "Guid", 0),
            ]
        );
        assert!(items[0].has_doc);
        assert!(items[0].members[1].has_default);
        assert_eq!(
            items[0].members[1].signature,
            "limit: Option<i32> [default = None]"
        );
        assert_eq!(items[2].members[0].kind, MemberKind::Constructor);
        assert_eq!(
            items[2].members[1].signature,
            "fn get(&self, id: &str) -> ApiResult<Option<Item>>"
        );
    }
}
