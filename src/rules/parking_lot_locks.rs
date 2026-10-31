//! Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`
//!
//! Docs and examples: `rules/parking-lot-locks/`.
//!
//! Flags `use std::sync::{Mutex, RwLock}` imports and types written as
//! `std::sync::Mutex<..>` in shipped component code (clippy's
//! `disallowed_types` does the same with full type resolution). A file that
//! uses `Condvar` keeps the std mutex it needs.

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "parking-lot-locks",
    area: Area::Concurrency,
    title: "Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_types"],
    in_tests: false,
    check,
};

const STD_LOCKS: &[&str] = &["Mutex", "RwLock"];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        if file.text.contains("Condvar") {
            continue;
        }
        let support = TestSupport::of(file);
        let mut v = StdLocks { hits: Vec::new() };
        v.visit_file(&file.ast);
        for (line, column, names) in v.hits {
            if !support.contains(line) {
                let std: Vec<String> = names.iter().map(|n| format!("`std::sync::{n}`")).collect();
                let pl: Vec<String> = names
                    .iter()
                    .map(|n| format!("`parking_lot::{n}`"))
                    .collect();
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!(
                        "{} {} on panic and {} `.lock().unwrap()`; use {}",
                        std.join(" and "),
                        if names.len() > 1 { "poison" } else { "poisons" },
                        if names.len() > 1 { "need" } else { "needs" },
                        pl.join(" and ")
                    ),
                );
            }
        }
    }
}

struct StdLocks {
    hits: Vec<(usize, usize, Vec<String>)>,
}

impl<'ast> Visit<'ast> for StdLocks {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut found = Vec::new();
        collect_use(&u.tree, &mut Vec::new(), &mut found);
        // One finding per `use` item.
        if !found.is_empty() {
            let start = syn::spanned::Spanned::span(u).start();
            self.hits.push((start.line, start.column + 1, found));
        }
    }
    fn visit_item_macro(&mut self, m: &'ast syn::ItemMacro) {
        // A `macro_rules!` body expands in other crates, which may not depend
        // on parking_lot.
        if !m.mac.path.is_ident("macro_rules") {
            syn::visit::visit_item_macro(self, m);
        }
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // `lazy_static! { static ref X: std::sync::Mutex<..> = .. }`
        let mut found: Option<(usize, usize, String)> = None;
        errors::scan_tokens(m.tokens.clone(), &mut |tokens, i| {
            let ident = |k: usize| match tokens.get(k) {
                Some(proc_macro2::TokenTree::Ident(id)) => Some(id.to_string()),
                _ => None,
            };
            if found.is_none()
                && ident(i).as_deref() == Some("std")
                && ident(i + 3).as_deref() == Some("sync")
            {
                if let Some(name) = ident(i + 6).filter(|n| STD_LOCKS.contains(&n.as_str())) {
                    let start = tokens[i].span().start();
                    found = Some((start.line, start.column + 1, name));
                }
            }
        });
        if let Some((line, column, name)) = found {
            self.hits.push((line, column, vec![name]));
        }
        syn::visit::visit_macro(self, m);
    }
    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        let segs: Vec<String> = t
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if let [.., a, b, c] = segs.as_slice() {
            if a == "std" && b == "sync" && STD_LOCKS.contains(&c.as_str()) {
                let start = syn::spanned::Spanned::span(t).start();
                self.hits
                    .push((start.line, start.column + 1, vec![c.clone()]));
            }
        }
        syn::visit::visit_type_path(self, t);
    }
}

/// The std lock names a `use` tree imports from `std::sync`.
fn collect_use(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            collect_use(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Group(g) => {
            for t in &g.items {
                collect_use(t, prefix, out);
            }
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            if prefix.as_slice() == ["std", "sync"] && STD_LOCKS.contains(&name.as_str()) {
                out.push(name);
            }
        }
        syn::UseTree::Rename(r) => {
            let name = r.ident.to_string();
            if prefix.as_slice() == ["std", "sync"] && STD_LOCKS.contains(&name.as_str()) {
                out.push(name);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/tabs/src/store.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/parking-lot-locks/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/parking-lot-locks/do.rs")).is_empty());
    }

    #[test]
    fn flags_qualified_types() {
        let src = "struct S {\n    events: std::sync::RwLock<Vec<u8>>,\n}\n";
        let found = run(src);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("parking_lot::RwLock"));
    }

    #[test]
    fn ignores_other_sync_items_and_condvars() {
        assert!(run("use std::sync::{Arc, OnceLock, atomic::AtomicBool};\n").is_empty());
        assert!(run("use std::sync::{Condvar, Mutex};\n").is_empty());
    }

    #[test]
    fn flags_lazy_statics_but_not_macro_rules() {
        let src = "lazy_static! {\n    static ref X: std::sync::Mutex<u8> = std::sync::Mutex::new(0);\n}\nmacro_rules! m { () => { struct S(std::sync::Mutex<u8>); } }\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2]);
    }

    #[test]
    fn ignores_test_modules() {
        assert!(run("#[cfg(test)]\nmod tests {\n    use std::sync::Mutex;\n}\n").is_empty());
    }
}
