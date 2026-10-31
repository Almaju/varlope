//! Let rustfmt own imports and layout (incl. inside macros)
//!
//! Docs and examples: `rules/rustfmt-config/`.

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;

pub static RULE: Rule = Rule {
    id: "rustfmt-config",
    area: Area::Idioms,
    title: "Let rustfmt own imports and layout (incl. inside macros)",
    kind: Kind::Clippy,
    clippy: &["clippy::wildcard_imports", "unused_qualifications"],
    in_tests: false,
    check,
};

/// Import grouping and nesting belong to `cargo +nightly fmt` and clippy's
/// `wildcard_imports`. varlope adds what rustfmt does not move: a `use`
/// declared below the code of its module, where readers do not look for it.
fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        late_uses(file, &file.ast.items, out);
    }
}

/// Reports the first `use` below code in each module, with how many follow.
fn late_uses(file: &RustFile, items: &[syn::Item], out: &mut Findings) {
    let mut first_code: Option<usize> = None;
    let mut late: Vec<(usize, usize)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            syn::Item::Use(u) => {
                // `pub use` re-exports are API layout, often kept next to
                // what they re-export; feature-gated imports sit by their use.
                let reexport = !matches!(u.vis, syn::Visibility::Inherited);
                let gated = u.attrs.iter().any(|a| a.path().is_ident("cfg"));
                let paired = i > 0 && paired(u, &items[i - 1], items.get(i + 1));
                if first_code.is_some() && !gated && !reexport && !paired {
                    late.push(file.pos(&u.use_token));
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    late_uses(file, inner, out);
                }
            }
            syn::Item::ExternCrate(_) => {}
            // `macro_rules!` and macro calls (`uniffi::setup_scaffolding!()`)
            // often have to come first.
            syn::Item::Macro(_) => {}
            other => {
                first_code.get_or_insert(file.line_of_node(other));
            }
        }
    }
    if let (Some(&(line, column)), Some(code_line)) = (late.first(), first_code) {
        let (what, them) = match late.len() {
            1 => ("a `use`".to_string(), "it"),
            n => (format!("{n} `use` declarations"), "them"),
        };
        out.add_at(
            &file.path,
            line,
            column,
            format!("{what} below the module's code (which starts on line {code_line}); move {them} up with the other imports"),
        );
    }
}

/// A `use` that belongs with its neighbour: `mod backend { .. }` then
/// `use backend::*;`, or `use serde_json::Value as JsonValue;` right before
/// the `uniffi::custom_type!(JsonValue, ..)` that needs it.
fn paired(u: &syn::ItemUse, previous: &syn::Item, next: Option<&syn::Item>) -> bool {
    let names = idioms::idents(&u.tree);
    let after_its_module = matches!(previous, syn::Item::Mod(m)
        if m.content.is_some() && names.contains(&m.ident.to_string()));
    let before_its_macro = matches!(next, Some(syn::Item::Macro(m))
        if idioms::idents(&m.mac.tokens).iter().any(|i| names.contains(i)));
    after_its_module || before_its_macro
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/stateful/enrollment.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/rustfmt-config/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 9);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/rustfmt-config/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn accepts_mod_declarations_macros_and_cfg_imports() {
        let src = "uniffi::setup_scaffolding!();\nmod db;\npub use db::Db;\n\
                   fn f() {}\n#[cfg(feature = \"x\")]\nuse x::Y;\npub use db::Row;\n\
                   mod backend { pub fn open() {} }\nuse backend::*;\n\
                   use serde_json::Value as JsonValue;\nuniffi::custom_type!(JsonValue, String);\n\
                   #[cfg(test)]\nmod tests {\n    use super::*;\n    fn g() {}\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }
}
