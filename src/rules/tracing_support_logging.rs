//! Log through `tracing_support`, not the `log` crate
//!
//! Docs and examples: `rules/tracing-support-logging/`.
//!
//! Flags `log::trace!` … `log::error!` (by path, or imported with `use log::..`)
//! and direct `tracing::` logging macros in component code outside
//! `components/support/tracing`, which wraps them. Macros reached through
//! `error_support::` or `tracing_support::` are the right ones.

use std::collections::{BTreeMap, BTreeSet};

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::workspace::{is_component_src, DepKind, Workspace};

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

/// The macros `error_support` and `tracing_support` provide.
const SINK_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// The crates that log to the app-facing sink.
const SINKS: &[&str] = &["error_support", "tracing_support"];

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
            sink_imports: BTreeSet::new(),
            hits: Vec::new(),
        };
        v.visit_file(&file.ast);
        let sink = sink(cx.ws, file);
        for hit in v.hits {
            if !support.contains(hit.line) {
                let (krate, name) = (&hit.krate, &hit.name);
                out.add_at(
                    &file.path,
                    hit.line,
                    hit.column,
                    format!("`{krate}::{name}!` bypasses the tracing_support sink the apps read; use `error_support::{name}!`"),
                );
                let fix = sink
                    .filter(|_| SINK_LEVELS.contains(&name.as_str()))
                    .and_then(|sink| fix(file, &hit, sink, &v.sink_imports));
                if let (Some(fix), Some(last)) = (fix, out.items.last_mut()) {
                    last.fix = Some(fix);
                }
            }
        }
    }
}

/// The crate to log through in `file`: `tracing_support` when the file
/// already does and its package depends on it, else `error_support` (what
/// the message says), else `tracing_support`. `None` when the package has
/// neither, as the fix would not build.
fn sink(ws: &Workspace, file: &RustFile) -> Option<&'static str> {
    let deps: Vec<String> = ws
        .package_of(&file.path)?
        .dependencies()
        .into_iter()
        .filter(|d| d.kind == DepKind::Normal && !d.optional)
        .map(|d| d.name.replace('-', "_"))
        .collect();
    let has = |c: &str| deps.iter().any(|d| d == c);
    let tracing_style =
        file.text.contains("tracing_support::") && !file.text.contains("error_support::");
    if tracing_style && has("tracing_support") {
        Some("tracing_support")
    } else if has("error_support") {
        Some("error_support")
    } else if has("tracing_support") {
        Some("tracing_support")
    } else {
        None
    }
}

/// `log::warn!` becomes `error_support::warn!`, or `warn!` when the file
/// imports it from a sink already. A macro imported with `use log::warn;`
/// gets the import changed, when it imports nothing else and rustfmt would
/// keep it where it is.
fn fix(file: &RustFile, hit: &Hit, sink: &str, sink_imports: &BTreeSet<String>) -> Option<Fix> {
    if hit.bare {
        return import_fix(file, &hit.krate, &hit.name, sink);
    }
    let text = file.line(hit.line);
    let start = text.char_indices().nth(hit.column - 1).map(|(i, _)| i)?;
    let rest = text[start..].strip_prefix(&format!("{}::", hit.krate))?;
    if !rest.starts_with(&format!("{}!", hit.name)) {
        return None;
    }
    let path = if sink_imports.contains(&hit.name) {
        String::new()
    } else {
        format!("{sink}::")
    };
    Some(Fix::replace_line(
        hit.line,
        format!("{}{path}{rest}", &text[..start]),
    ))
}

/// `use log::{debug, warn};` becomes `use error_support::{debug, warn};`.
fn import_fix(file: &RustFile, krate: &str, name: &str, sink: &str) -> Option<Fix> {
    let items = &file.ast.items;
    let at = items.iter().position(|item| {
        let syn::Item::Use(u) = item else {
            return false;
        };
        let syn::UseTree::Path(p) = &u.tree else {
            return false;
        };
        let mut names = Vec::new();
        let plain = match &*p.tree {
            syn::UseTree::Name(n) => {
                names.push(n.ident.to_string());
                true
            }
            syn::UseTree::Group(g) => g.items.iter().all(|t| match t {
                syn::UseTree::Name(n) => {
                    names.push(n.ident.to_string());
                    true
                }
                _ => false,
            }),
            _ => false,
        };
        p.ident == krate
            && u.attrs.is_empty()
            && plain
            && names.iter().all(|n| SINK_LEVELS.contains(&n.as_str()))
            && names.iter().any(|n| n == name)
    })?;
    let item = &items[at];
    let (line, end) = (item.span().start().line, item.span().end().line);
    if line != end {
        return None;
    }
    // rustfmt sorts a run of `use` items: the renamed one must stay in order.
    let next_to = |other: Option<&syn::Item>, before: bool| match other {
        Some(syn::Item::Use(u)) => {
            let adjacent = if before {
                u.span().end().line + 1 == line
            } else {
                u.span().start().line == end + 1
            };
            !adjacent || {
                let key = use_key(&u.tree);
                if before {
                    key.as_str() <= sink
                } else {
                    key.as_str() >= sink
                }
            }
        }
        _ => true,
    };
    let previous = at.checked_sub(1).and_then(|i| items.get(i));
    if !next_to(previous, true) || !next_to(items.get(at + 1), false) {
        return None;
    }
    let text = file.line(line);
    let from = format!("use {krate}::");
    let start = text.find(&from)?;
    Some(Fix::replace_line(
        line,
        format!(
            "{}use {sink}::{}",
            &text[..start],
            &text[start + from.len()..]
        ),
    ))
}

/// The first segment of a `use` path, for ordering (`crate`, `self` and
/// `super` sort first).
fn use_key(tree: &syn::UseTree) -> String {
    let first = match tree {
        syn::UseTree::Path(p) => p.ident.to_string(),
        syn::UseTree::Name(n) => n.ident.to_string(),
        syn::UseTree::Rename(r) => r.ident.to_string(),
        syn::UseTree::Glob(_) | syn::UseTree::Group(_) => return "~".to_string(),
    };
    match first.as_str() {
        "self" | "super" | "crate" => String::new(),
        _ => first,
    }
}

struct Hit {
    line: usize,
    column: usize,
    krate: String,
    name: String,
    /// Called by its name alone, imported with `use log::..`.
    bare: bool,
}

struct Macros {
    /// Macro name -> crate, from `use log::{debug, warn}`.
    imported: BTreeMap<String, String>,
    /// Macro names imported from a sink: `use error_support::{debug, warn}`.
    sink_imports: BTreeSet<String>,
    hits: Vec<Hit>,
}

impl<'ast> Visit<'ast> for Macros {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        fn walk(tree: &syn::UseTree, prefix: &mut Vec<String>, v: &mut Macros) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    walk(&p.tree, prefix, v);
                    prefix.pop();
                }
                syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, prefix, v)),
                syn::UseTree::Name(n) => {
                    let name = n.ident.to_string();
                    if let [krate] = prefix.as_slice() {
                        if BYPASS.contains(&krate.as_str()) && LEVELS.contains(&name.as_str()) {
                            v.imported.insert(name, krate.clone());
                        } else if SINKS.contains(&krate.as_str())
                            && SINK_LEVELS.contains(&name.as_str())
                        {
                            v.sink_imports.insert(name);
                        }
                    }
                }
                _ => {}
            }
        }
        walk(&u.tree, &mut Vec::new(), self);
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
            self.hits.push(Hit {
                line: start.line,
                column: start.column + 1,
                krate,
                name,
                bare: segs.len() == 1,
            });
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

    const MANIFEST: (&str, &str) = (
        "components/relay/Cargo.toml",
        "[package]\nname = \"relay\"\n\n[dependencies]\nerror-support = { path = \"../support/error\" }\nlog = \"0.4\"\n",
    );

    fn edits(found: &[crate::Finding]) -> Vec<Option<(usize, usize, Vec<String>)>> {
        found
            .iter()
            .map(|f| {
                let fix = f.fix.as_ref()?;
                let e = &fix.edits[0];
                Some((e.line, e.remove, e.insert.clone()))
            })
            .collect()
    }

    #[test]
    fn fixes_the_import_or_the_path() {
        let dont = include_str!("../../rules/tracing-support-logging/dont.rs");
        let found = check(&RULE, &[MANIFEST, (PATH, dont)]);
        // `trace!` stays; it comes from the sink now.
        let import = Some((1, 1, vec!["use error_support::trace;".to_string()]));
        assert_eq!(edits(&found), vec![import]);

        let src = "use error_support::warn;\nfn f() {\n    log::warn!(\"a\");\n    log::trace!(\"b\");\n    log::log!(Level::Info, \"c\");\n}\n";
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        let line = |n, s: &str| Some((n, 1, vec![s.to_string()]));
        assert_eq!(
            edits(&found),
            vec![
                // Imported from the sink already: the file's own style.
                line(3, "    warn!(\"a\");"),
                line(4, "    error_support::trace!(\"b\");"),
                // No `log!` in the sink.
                None,
            ]
        );

        // Without a dependency on a sink, the fix would not build.
        let found = check(&RULE, &[(PATH, src)]);
        assert!(edits(&found).iter().all(Option::is_none), "{found:#?}");
    }

    #[test]
    fn keeps_imports_in_rustfmt_order() {
        let src = "use anyhow::Result;\nuse log::{debug, warn};\nuse serde::Deserialize;\nfn f() { warn!(\"a\"); }\n";
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        let fixed = "use error_support::{debug, warn};".to_string();
        assert_eq!(edits(&found), vec![Some((2, 1, vec![fixed]))]);
        // `error_support` would sort before `ffi_support`: no fix.
        let src = "use ffi_support::Handle;\nuse log::warn;\nfn f() { warn!(\"a\"); }\n";
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        assert_eq!(edits(&found), vec![None]);
    }

    #[test]
    fn ignores_the_tracing_support_crate() {
        let src = "fn f() { tracing::event!(Level::INFO, \"b\"); }";
        assert!(check(&RULE, &[("components/support/tracing/src/lib.rs", src)]).is_empty());
    }
}
