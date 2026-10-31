//! Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`
//!
//! Docs and examples: `rules/borrowed-params/`.

use std::collections::BTreeSet;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::uniffi::{self, ApiKind};

pub static RULE: Rule = Rule {
    id: "borrowed-params",
    area: Area::Idioms,
    title: "Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::ptr_arg",
        "clippy::ref_option",
        "clippy::needless_pass_by_value",
        "clippy::trivially_copy_pass_by_ref",
    ],
    in_tests: false,
    check,
};

/// Clippy's `ptr_arg` and `ref_option` are the lints for this; the
/// parameter types they flag can be read off the signature, so varlope
/// reports them without a build (`needless_pass_by_value` and
/// `trivially_copy_pass_by_ref` need types and stay with clippy).
fn check(cx: &Context, out: &mut Findings) {
    let exported = exported_signatures(cx);
    for file in idioms::rust_files(cx.ws) {
        // Functions named in strings (`#[serde(skip_serializing_if = "is_none")]`)
        // have a signature someone else chose.
        let named_in_strings: BTreeSet<String> = syntax::string_literals(&file.ast)
            .into_iter()
            .filter_map(|(_, s)| s.rsplit("::").next().map(str::to_string))
            .collect();
        for f in functions(&file.ast) {
            let trait_impl = f.trait_name.is_some() && f.self_ty.is_some();
            // UniFFI decides these signatures (`[ByRef] string?` is `&Option<String>`).
            let exported = syntax::has_attr(f.attrs, "export")
                || f.outer_attrs.iter().any(|a| syntax::has_attr(a, "export"))
                || exported.contains(&(f.self_ty.clone().unwrap_or_default(), f.name.clone()));
            if trait_impl || exported || f.is_test() || named_in_strings.contains(&f.name) {
                continue;
            }
            for input in &f.sig.inputs {
                let syn::FnArg::Typed(arg) = input else {
                    continue;
                };
                let Some(better) = borrowed_form(&arg.ty) else {
                    continue;
                };
                let (line, column) = file.pos(&arg.ty);
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!(
                        "parameter typed `{}`; take `{better}` so callers can pass any borrow without building one",
                        syntax::type_string(&arg.ty)
                    ),
                );
            }
        }
    }
}

/// `(type, method)` and `("", function)` pairs that UDL files export.
fn exported_signatures(cx: &Context) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for package in cx.ws.packages() {
        for item in uniffi::package_api(cx.ws, package) {
            match item.kind {
                ApiKind::Function => {
                    out.insert((String::new(), item.name.clone()));
                }
                _ => {
                    for m in &item.members {
                        let name = if m.name == "constructor" {
                            "new"
                        } else {
                            m.name.as_str()
                        };
                        out.insert((item.name.clone(), name.to_string()));
                    }
                }
            }
        }
    }
    out
}

/// The parameter type to suggest for `&String`, `&Vec<T>`, `&PathBuf` and
/// `&Option<T>`.
fn borrowed_form(ty: &syn::Type) -> Option<String> {
    let syn::Type::Reference(r) = ty else {
        return None;
    };
    if r.mutability.is_some() {
        return None; // `&mut Vec<T>` can push; `&mut Option<T>` can set
    }
    let name = syntax::type_ident(&r.elem)?;
    let args = syntax::type_args(&r.elem);
    match (name.as_str(), args.as_slice()) {
        ("String", []) => Some("&str".into()),
        ("PathBuf", []) => Some("&Path".into()),
        ("Vec", [t]) => Some(format!("&[{}]", syntax::type_string(t))),
        ("Option", [t]) => Some(match t {
            syn::Type::Reference(_) => format!("Option<{}>", syntax::type_string(t)),
            _ => {
                let inner = syn::Type::Reference(syn::TypeReference {
                    and_token: Default::default(),
                    lifetime: None,
                    mutability: None,
                    elem: Box::new((*t).clone()),
                });
                // `&Option<String>` reads best as `Option<&str>` (`.as_deref()`).
                let target =
                    borrowed_form(&inner).unwrap_or_else(|| format!("&{}", syntax::type_string(t)));
                format!("Option<{target}>")
            }
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/stateful/gecko_prefs.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/borrowed-params/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 5, 6], "{found:#?}");
        assert!(found[0].message.contains("take `&[GeckoPrefState]`"));
        assert!(found[1]
            .message
            .contains("take `Option<&Arc<GeckoPrefStore>>`"));
        let opt = check(
            &RULE,
            &[(PATH, "fn f(a: &Option<String>, b: &Option<Vec<u8>>) {}")],
        );
        assert!(opt[0].message.contains("take `Option<&str>`"), "{opt:#?}");
        assert!(opt[1].message.contains("take `Option<&[u8]>`"), "{opt:#?}");
        assert!(found[2].message.contains("take `&str`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/borrowed-params/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_trait_impls_exports_mut_refs_and_serde_helpers() {
        let src = "impl Display for A { fn fmt(&self, s: &String) {} }\n\
            #[uniffi::export]\nimpl Store { pub fn put(&self, v: &Vec<u8>) {} }\n\
            fn push(v: &mut Vec<u8>, o: &mut Option<u8>) {}\n\
            #[derive(Serialize)]\nstruct T { #[serde(skip_serializing_if = \"T::is_none\")] x: Option<u8> }\n\
            impl T { fn is_none(x: &Option<u8>) -> bool { x.is_none() } }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
        let udl = "namespace push {};\ninterface PushManager {\n  void subscribe([ByRef] string? key);\n};\n";
        let rust = "impl PushManager { pub fn subscribe(&self, key: &Option<String>) {} }\n";
        let found = check(
            &RULE,
            &[
                ("components/push/Cargo.toml", "[package]\nname = \"push\"\n"),
                ("components/push/src/push.udl", udl),
                ("components/push/src/lib.rs", rust),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
