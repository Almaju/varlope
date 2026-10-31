//! Small helpers over `syn` that many rules need.

use quote::ToTokens;
use syn::visit::Visit;

/// Whether `path` ends with the `::`-separated `suffix` (`uniffi::export`
/// matches `uniffi::export` and `::uniffi::export`; `export` matches both).
pub fn path_ends_with(path: &syn::Path, suffix: &str) -> bool {
    let want: Vec<&str> = suffix.split("::").collect();
    let have: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    have.len() >= want.len()
        && have[have.len() - want.len()..]
            .iter()
            .zip(&want)
            .all(|(h, w)| h == w)
}

/// The path of an attribute as a string: `uniffi::export`, `cfg`, `serde`...
pub fn attr_name(attr: &syn::Attribute) -> String {
    path_string(attr.path())
}

pub fn path_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// The first attribute whose path ends with `suffix`.
pub fn find_attr<'a>(attrs: &'a [syn::Attribute], suffix: &str) -> Option<&'a syn::Attribute> {
    attrs.iter().find(|a| path_ends_with(a.path(), suffix))
}

pub fn has_attr(attrs: &[syn::Attribute], suffix: &str) -> bool {
    find_attr(attrs, suffix).is_some()
}

/// The tokens inside an attribute's parentheses, as text (`""` if none).
pub fn attr_args(attr: &syn::Attribute) -> String {
    match &attr.meta {
        syn::Meta::List(list) => list.tokens.to_string(),
        syn::Meta::NameValue(nv) => nv.value.to_token_stream().to_string(),
        syn::Meta::Path(_) => String::new(),
    }
}

/// Every derive in `#[derive(..)]` lists, as written (`uniffi::Record`, `Debug`).
pub fn derives(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("derive")) {
        let _ = attr.parse_nested_meta(|meta| {
            out.push(path_string(&meta.path));
            Ok(())
        });
    }
    out
}

/// Whether a derive list holds `name` (matched on the path's last segments:
/// `uniffi::Record` matches only `uniffi::Record`, `Record` matches both).
pub fn has_derive(attrs: &[syn::Attribute], name: &str) -> bool {
    derives(attrs)
        .iter()
        .any(|d| d == name || d.ends_with(&format!("::{name}")))
}

/// The doc comment (`///` or `#[doc = ".."]`) of an item, joined, if any.
pub fn doc(attrs: &[syn::Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let joined = lines.join("\n");
    (!joined.trim().is_empty()).then_some(joined)
}

/// `cfg(test)`, `cfg(any(test, ..))`, `#[test]`: code that only exists in tests.
pub fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    crate::test_regions::is_test_gated(attrs)
}

/// Tokens of anything printable, as normalized text (`Option < String >`).
pub fn tokens<T: ToTokens>(node: &T) -> String {
    node.to_token_stream().to_string()
}

/// A type as compact text: `Option<Vec<String>>`.
pub fn type_string(ty: &syn::Type) -> String {
    compact(&tokens(ty))
}

/// Remove the spaces `TokenStream::to_string` puts everywhere.
pub fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == ' ' {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            let word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '"';
            let around_eq = (next == '=' || prev == '=') && next != '>' && prev != '-';
            if word(prev) && word(next) || prev == ',' || around_eq {
                out.push(' ');
            }
            continue;
        }
        out.push(*c);
    }
    out
}

/// The last segment of a type's path: `Foo` for `a::Foo<T>`, `&Foo`.
pub fn type_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_ident(&r.elem),
        syn::Type::Group(g) => type_ident(&g.elem),
        syn::Type::Paren(p) => type_ident(&p.elem),
        _ => None,
    }
}

/// The generic arguments of a type's last path segment: `[T]` for `Option<T>`.
pub fn type_args(ty: &syn::Type) -> Vec<&syn::Type> {
    let syn::Type::Path(p) = ty else {
        return Vec::new();
    };
    let Some(last) = p.path.segments.last() else {
        return Vec::new();
    };
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return Vec::new();
    };
    args.args
        .iter()
        .filter_map(|a| match a {
            syn::GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .collect()
}

/// A function's return type as text, `""` for `()`.
pub fn return_type(sig: &syn::Signature) -> String {
    match &sig.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, ty) => type_string(ty),
    }
}

/// Every method call in a node: `(method name, receiver, the call)`.
pub fn method_calls<'a, F>(node: &'a syn::File, mut f: F)
where
    F: FnMut(&'a syn::ExprMethodCall),
{
    struct V<'f, F>(&'f mut F);
    impl<'a, F: FnMut(&'a syn::ExprMethodCall)> Visit<'a> for V<'_, F> {
        fn visit_expr_method_call(&mut self, node: &'a syn::ExprMethodCall) {
            (self.0)(node);
            syn::visit::visit_expr_method_call(self, node);
        }
    }
    V(&mut f).visit_file(node);
}

/// Every macro invocation in a file (expression, statement or item position).
pub fn macros<'a, F>(node: &'a syn::File, mut f: F)
where
    F: FnMut(&'a syn::Macro),
{
    struct V<'f, F>(&'f mut F);
    impl<'a, F: FnMut(&'a syn::Macro)> Visit<'a> for V<'_, F> {
        fn visit_macro(&mut self, node: &'a syn::Macro) {
            (self.0)(node);
            syn::visit::visit_macro(self, node);
        }
    }
    V(&mut f).visit_file(node);
}

/// The name a macro is invoked by: `report_error` for `error_support::report_error!`.
pub fn macro_name(mac: &syn::Macro) -> String {
    mac.path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

/// String literals in a file, with their value: `(literal, value)`. Includes
/// literals inside macro invocations (`format!`, `execute!`...).
pub fn string_literals(file: &syn::File) -> Vec<(proc_macro2::Span, String)> {
    let mut out = Vec::new();
    collect_strings(file.to_token_stream(), &mut out);
    out
}

fn collect_strings(stream: proc_macro2::TokenStream, out: &mut Vec<(proc_macro2::Span, String)>) {
    for tt in stream {
        match tt {
            proc_macro2::TokenTree::Group(g) => collect_strings(g.stream(), out),
            proc_macro2::TokenTree::Literal(lit) => {
                let tokens =
                    proc_macro2::TokenStream::from(proc_macro2::TokenTree::Literal(lit.clone()));
                if let Ok(syn::Lit::Str(s)) = syn::parse2::<syn::Lit>(tokens) {
                    out.push((lit.span(), s.value()));
                }
            }
            _ => {}
        }
    }
}

/// Items of a file, including those nested in inline modules, with the
/// attributes of the modules they sit in.
pub fn all_items(file: &syn::File) -> Vec<&syn::Item> {
    fn walk<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
        for item in items {
            out.push(item);
            if let syn::Item::Mod(m) = item {
                if let Some((_, inner)) = &m.content {
                    walk(inner, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_and_attrs() {
        let item: syn::ItemStruct = syn::parse_quote! {
            /// Docs.
            #[derive(Debug, uniffi::Record)]
            #[serde(default)]
            pub struct A { pub x: Option<Vec<String>> }
        };
        assert!(has_derive(&item.attrs, "uniffi::Record"));
        assert!(has_derive(&item.attrs, "Record"));
        assert!(!has_derive(&item.attrs, "uniffi::Enum"));
        assert_eq!(doc(&item.attrs).unwrap().trim(), "Docs.");
        assert_eq!(
            attr_args(find_attr(&item.attrs, "serde").unwrap()),
            "default"
        );
        let syn::Fields::Named(f) = &item.fields else {
            panic!()
        };
        assert_eq!(type_string(&f.named[0].ty), "Option<Vec<String>>");
        assert_eq!(type_ident(&f.named[0].ty).as_deref(), Some("Option"));
    }

    #[test]
    fn finds_strings_in_macros() {
        let file: syn::File = syn::parse_quote! {
            fn f() { conn.execute_batch("CREATE TABLE t (a)"); let s = format!("x {}", 1); }
        };
        let values: Vec<String> = string_literals(&file).into_iter().map(|(_, v)| v).collect();
        assert_eq!(
            values,
            vec!["CREATE TABLE t (a)".to_string(), "x {}".to_string()]
        );
    }
}
