//! Log through `tracing_support`, not the `log` crate
//!
//! Docs and examples: `rules/tracing-support-logging/`.
//!
//! Flags `log::trace!` … `log::error!` (by path, or imported with `use log::..`)
//! and direct `tracing::` logging macros in component code outside
//! `components/support/tracing`, which wraps them. Macros reached through
//! `error_support::` or `tracing_support::` are the right ones.

use std::collections::BTreeMap;

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::workspace::is_component_src;

pub static RULE: Rule = Rule {
    id: "tracing-support-logging",
    area: Area::Privacy,
    title: "Log through `tracing_support`, not the `log` crate",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_macros"],
    in_tests: false,
    check,
};

const LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "event", "log"];

/// The crates whose logging macros bypass the app-facing sink.
const BYPASS: &[&str] = &["log", "tracing"];

fn check(cx: &Context, out: &mut Findings) {
    let files = cx.ws.rust_files().filter(|f| {
        (errors::is_shipped(&f.path) || f.path.starts_with("components/crashtest/"))
            && is_component_src(&f.path)
            && !f.path.starts_with("components/support/tracing/")
    });
    for file in files {
        let support = TestSupport::of(file);
        let mut v = Macros {
            imported: BTreeMap::new(),
            hits: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, column, krate, name) in v.hits {
            if !support.contains(line) {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("`{krate}::{name}!` bypasses the tracing_support sink the apps read; use `error_support::{name}!`"),
                );
            }
        }
    }
}

struct Macros {
    /// Macro name -> crate, from `use log::{debug, warn}`.
    imported: BTreeMap<String, String>,
    hits: Vec<(usize, usize, String, String)>,
}

impl<'ast> Visit<'ast> for Macros {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        fn walk(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut BTreeMap<String, String>) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    walk(&p.tree, prefix, out);
                    prefix.pop();
                }
                syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, prefix, out)),
                syn::UseTree::Name(n) => {
                    let name = n.ident.to_string();
                    if let [krate] = prefix.as_slice() {
                        if BYPASS.contains(&krate.as_str()) && LEVELS.contains(&name.as_str()) {
                            out.insert(name, krate.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        walk(&u.tree, &mut Vec::new(), &mut self.imported);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let segs: Vec<String> = m
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let hit = match segs.as_slice() {
            [krate, name]
                if BYPASS.contains(&krate.as_str()) && LEVELS.contains(&name.as_str()) =>
            {
                Some((krate.clone(), name.clone()))
            }
            [name] => self.imported.get(name).map(|k| (k.clone(), name.clone())),
            _ => None,
        };
        if let Some((krate, name)) = hit {
            let start = m.path.segments[0].ident.span().start();
            self.hits.push((start.line, start.column + 1, krate, name));
        }
        syn::visit::visit_macro(self, m);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/relay/src/lib.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/tracing-support-logging/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 5);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/tracing-support-logging/do.rs")).is_empty());
    }

    #[test]
    fn flags_full_paths_and_tracing() {
        let src = "fn f() {\n    log::warn!(\"a\");\n    tracing::event!(Level::INFO, \"b\");\n    error_support::warn!(\"c\");\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 3]);
    }

    #[test]
    fn ignores_the_tracing_support_crate() {
        let src = "fn f() { tracing::event!(Level::INFO, \"b\"); }";
        assert!(check(&RULE, &[("components/support/tracing/src/lib.rs", src)]).is_empty());
    }
}
