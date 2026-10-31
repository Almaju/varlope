//! Helpers shared by the idioms rules.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::source::RustFile;
use crate::syntax;
use crate::workspace::{DepKind, Package, Workspace};

/// Generated or vendored code: nobody reviews it line by line.
pub fn is_generated(path: &str) -> bool {
    path.starts_with("components/external/")
        || path.contains("/generated/")
        || path.contains("/vendor/")
        || path.contains("third_party/")
        || path.ends_with(".uniffi.rs")
}

/// Rust files the idioms rules read: everything that parses, minus generated
/// and vendored code.
pub fn rust_files(ws: &Workspace) -> impl Iterator<Item = &RustFile> {
    ws.rust_files().filter(|f| !is_generated(&f.path))
}

/// Tool, example and integration-test crates: maintained, but not shipped.
pub fn is_tooling(path: &str) -> bool {
    ["tools/", "examples/", "testing/", "automation/"]
        .iter()
        .any(|p| path.starts_with(p))
}

/// Code that ships in a library: not a build script, a binary, a benchmark,
/// or a tool, example or integration-test crate.
pub fn is_library_code(ws: &Workspace, path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "build.rs" || name == "main.rs" || path.contains("/src/bin/") {
        return false;
    }
    if path.contains("/benchmarks/") || path.contains("/bench/") {
        return false;
    }
    if is_tooling(path) || path.starts_with("megazords/") {
        return false;
    }
    match ws.package_of(path) {
        Some(package) => !is_cli_package(ws, package) && !is_build_helper(ws, package),
        None => true,
    }
}

/// A crate other crates only use from their `build.rs` (`nss_build_common`).
pub fn is_build_helper(ws: &Workspace, package: &Package) -> bool {
    let mut kinds = ws.packages().iter().flat_map(|p| {
        p.dependencies()
            .into_iter()
            .filter(|d| d.name == package.name || d.name.replace('-', "_") == package.lib_name())
            .map(|d| d.kind)
    });
    let mut any = false;
    let only_build = kinds.all(|k| {
        any = true;
        k == DepKind::Build
    });
    any && only_build
}

/// A crate whose main job is a command-line tool (it has `src/main.rs`).
pub fn is_cli_package(ws: &Workspace, package: &Package) -> bool {
    let main = if package.dir.is_empty() {
        "src/main.rs".to_string()
    } else {
        format!("{}/src/main.rs", package.dir)
    };
    ws.file(&main).is_some()
}

/// Whether a `#[cfg(..)]` among `attrs` gates the item behind a feature.
pub fn feature_gated(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && syntax::attr_args(a).contains("feature"))
}

/// A function body together with where it sits.
pub struct FnInfo<'a> {
    pub name: String,
    pub attrs: &'a [syn::Attribute],
    pub sig: &'a syn::Signature,
    pub block: &'a syn::Block,
    /// The trait implemented by the enclosing `impl`, if any (`Drop`, `Hash`).
    pub trait_name: Option<String>,
    /// The type of the enclosing `impl` (`SearchEngineUrl`), if any.
    pub self_ty: Option<String>,
    /// Attributes of the enclosing `impl` and modules, outermost first.
    pub outer_attrs: Vec<&'a [syn::Attribute]>,
    /// `pub` (any `pub(..)` excluded).
    pub is_pub: bool,
}

impl FnInfo<'_> {
    /// `#[test]`-like attribute on the function itself.
    pub fn is_test(&self) -> bool {
        syntax::is_test_gated(self.attrs)
    }

    /// Whether the function, or an `impl`/module around it, is behind
    /// `#[cfg(feature = ..)]`.
    pub fn feature_gated(&self) -> bool {
        feature_gated(self.attrs) || self.outer_attrs.iter().any(|a| feature_gated(a))
    }
}

/// Every function with a body in a file: free functions, inherent and trait
/// impl methods, and trait default methods.
pub fn functions(file: &syn::File) -> Vec<FnInfo<'_>> {
    let mut out = Vec::new();
    collect_fns(&file.items, &mut Vec::new(), &mut out);
    out
}

fn collect_fns<'a>(
    items: &'a [syn::Item],
    outer: &mut Vec<&'a [syn::Attribute]>,
    out: &mut Vec<FnInfo<'a>>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) => out.push(FnInfo {
                name: f.sig.ident.to_string(),
                attrs: &f.attrs,
                sig: &f.sig,
                block: &f.block,
                trait_name: None,
                self_ty: None,
                outer_attrs: outer.clone(),
                is_pub: matches!(f.vis, syn::Visibility::Public(_)),
            }),
            syn::Item::Impl(imp) => {
                outer.push(&imp.attrs);
                let trait_name = imp
                    .trait_
                    .as_ref()
                    .and_then(|(_, p, _)| p.segments.last())
                    .map(|s| s.ident.to_string());
                let self_ty = syntax::type_ident(&imp.self_ty);
                for ii in &imp.items {
                    if let syn::ImplItem::Fn(f) = ii {
                        out.push(FnInfo {
                            name: f.sig.ident.to_string(),
                            attrs: &f.attrs,
                            sig: &f.sig,
                            block: &f.block,
                            trait_name: trait_name.clone(),
                            self_ty: self_ty.clone(),
                            outer_attrs: outer.clone(),
                            is_pub: matches!(f.vis, syn::Visibility::Public(_)),
                        });
                    }
                }
                outer.pop();
            }
            syn::Item::Trait(t) => {
                outer.push(&t.attrs);
                for ti in &t.items {
                    if let syn::TraitItem::Fn(f) = ti {
                        if let Some(block) = &f.default {
                            out.push(FnInfo {
                                name: f.sig.ident.to_string(),
                                attrs: &f.attrs,
                                sig: &f.sig,
                                block,
                                trait_name: Some(t.ident.to_string()),
                                self_ty: None,
                                outer_attrs: outer.clone(),
                                is_pub: false,
                            });
                        }
                    }
                }
                outer.pop();
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    outer.push(&m.attrs);
                    collect_fns(inner, outer, out);
                    outer.pop();
                }
            }
            _ => {}
        }
    }
}

/// Identifiers used anywhere in a node, including inside macro invocations.
pub fn idents<T: quote::ToTokens>(node: &T) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_idents(node.to_token_stream(), &mut out);
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

/// Every macro invocation in a block, with the path's last segment.
pub fn macros_in<'a, T>(node: &'a T, f: impl FnMut(&'a syn::Macro))
where
    T: VisitNode<'a>,
{
    struct V<F>(F);
    impl<'a, F: FnMut(&'a syn::Macro)> Visit<'a> for V<F> {
        fn visit_macro(&mut self, node: &'a syn::Macro) {
            (self.0)(node);
            syn::visit::visit_macro(self, node);
        }
    }
    let mut v = V(f);
    node.visit_with(&mut v);
}

/// Nodes a visitor can start from.
pub trait VisitNode<'a> {
    fn visit_with<V: Visit<'a>>(&'a self, v: &mut V);
}

impl<'a> VisitNode<'a> for syn::Block {
    fn visit_with<V: Visit<'a>>(&'a self, v: &mut V) {
        v.visit_block(self);
    }
}

impl<'a> VisitNode<'a> for syn::File {
    fn visit_with<V: Visit<'a>>(&'a self, v: &mut V) {
        v.visit_file(self);
    }
}

impl<'a> VisitNode<'a> for syn::Expr {
    fn visit_with<V: Visit<'a>>(&'a self, v: &mut V) {
        v.visit_expr(self);
    }
}

/// The first string literal among a macro's tokens, if any.
pub fn first_string_literal(mac: &syn::Macro) -> Option<String> {
    for tt in mac.tokens.clone() {
        if let proc_macro2::TokenTree::Literal(lit) = tt {
            let tokens = proc_macro2::TokenStream::from(proc_macro2::TokenTree::Literal(lit));
            if let Ok(syn::Lit::Str(s)) = syn::parse2::<syn::Lit>(tokens) {
                return Some(s.value());
            }
            return None;
        }
    }
    None
}
