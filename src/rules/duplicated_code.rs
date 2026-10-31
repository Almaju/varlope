//! Don't copy-paste logic; reuse existing helpers
//!
//! Docs and examples: `rules/duplicated-code/`.

use std::collections::BTreeMap;

use quote::ToTokens;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "duplicated-code",
    area: Area::Idioms,
    title: "Don't copy-paste logic; reuse existing helpers",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// Code shorter than this many tokens is an idiom, not logic worth sharing.
const MIN_TOKENS: usize = 60;
/// Runs of this many statements copied into another function.
const MIN_STMTS: usize = 5;

/// Where a piece of code lives.
#[derive(Clone)]
struct Site {
    path: String,
    line: usize,
    name: String,
}

fn check(cx: &Context, out: &mut Findings) {
    let mut bodies: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    let mut windows: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for file in idioms::rust_files(cx.ws) {
        if !file.path.starts_with("components/") || file.is_test_file() {
            continue;
        }
        for f in functions(&file.ast) {
            let line = file.line_of_node(&f.sig.ident);
            // Trait impls for different types cannot call each other.
            if f.is_test()
                || file.in_test(line)
                || is_migration(&f.name)
                || f.trait_name.is_some() && f.self_ty.is_some()
            {
                continue;
            }
            let site = Site {
                path: file.path.clone(),
                line,
                name: f.name.clone(),
            };
            let (text, count) = normalized(f.block);
            if count >= MIN_TOKENS {
                bodies.entry(text).or_default().push(site.clone());
            }
            let stmts = &f.block.stmts;
            if stmts.len() > MIN_STMTS {
                for (i, window) in stmts.windows(MIN_STMTS).enumerate() {
                    let (text, count) = normalized_stmts(window);
                    if count >= MIN_TOKENS {
                        let line = file.line_of_node(&stmts[i]);
                        windows.entry(text).or_default().push(Site {
                            line,
                            ..site.clone()
                        });
                    }
                }
            }
        }
    }
    // The shared helper is the original, the other one the copy.
    for sites in bodies.values_mut().chain(windows.values_mut()) {
        sites.sort_by_key(|s| (!s.path.starts_with("components/support/"), s.path.clone(), s.line));
    }
    let mut reported: Vec<(String, String)> = Vec::new();
    for sites in bodies.values().filter(|s| s.len() > 1) {
        let first = &sites[0];
        for dup in sites[1..].iter().filter(|d| !parallel_modules(&first.path, &d.path)) {
            reported.push((dup.path.clone(), dup.name.clone()));
            out.add(
                &dup.path,
                dup.line,
                format!(
                    "`{}` has the same body as `{}` ({}:{}); keep one and call it from the other",
                    dup.name, first.name, first.path, first.line
                ),
            );
        }
    }
    // Copied blocks inside otherwise different functions, once per function
    // pair, at the start of the copied run.
    type Pair = (String, String, String, String);
    let mut pairs: BTreeMap<Pair, (usize, usize)> = BTreeMap::new();
    for sites in windows.values().filter(|s| s.len() > 1) {
        let first = &sites[0];
        for dup in &sites[1..] {
            let same_fn = dup.path == first.path && dup.name == first.name;
            let whole = reported.iter().any(|(p, n)| {
                (p == &dup.path && n == &dup.name) || (p == &first.path && n == &first.name)
            });
            if same_fn || whole || parallel_modules(&first.path, &dup.path) {
                continue;
            }
            let key = (
                first.path.clone(),
                first.name.clone(),
                dup.path.clone(),
                dup.name.clone(),
            );
            let lines = pairs.entry(key).or_insert((first.line, dup.line));
            *lines = (lines.0.min(first.line), lines.1.min(dup.line));
        }
    }
    for ((first_path, first_name, dup_path, _), (first_line, dup_line)) in pairs {
        out.add(
            &dup_path,
            dup_line,
            format!(
                "these {MIN_STMTS}+ statements also appear in `{first_name}` ({first_path}:{first_line}); move them into a helper both call"
            ),
        );
    }
}

/// Files at the same place in parallel modules (`backends/kotlin/x.rs` and
/// `backends/swift/x.rs`): the same tokens resolve to different types there.
fn parallel_modules(a: &str, b: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('/').collect(), b.split('/').collect());
    a.len() == b.len() && a.iter().zip(&b).filter(|(x, y)| x != y).count() == 1 && a.last() == b.last()
}

/// Schema upgrades are frozen history: they may repeat current code.
fn is_migration(name: &str) -> bool {
    name.starts_with("upgrade_from") || name.starts_with("migrate")
}

/// Token text of a block and its token count.
fn normalized(block: &syn::Block) -> (String, usize) {
    let tokens = block.to_token_stream();
    (tokens.to_string(), count(tokens))
}

fn normalized_stmts(stmts: &[syn::Stmt]) -> (String, usize) {
    let mut text = String::new();
    let mut n = 0;
    for s in stmts {
        let tokens = s.to_token_stream();
        n += count(tokens.clone());
        text.push_str(&tokens.to_string());
        text.push('\n');
    }
    (text, n)
}

fn count(stream: proc_macro2::TokenStream) -> usize {
    stream
        .into_iter()
        .map(|tt| match tt {
            proc_macro2::TokenTree::Group(g) => 2 + count(g.stream()),
            _ => 1,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/places/src/storage/mod.rs";
    const HELPER: (&str, &str) = (
        "components/support/sql/src/maintenance.rs",
        "pub fn vacuum(conn: &Connection) -> Result<()> {\n\
            let auto_vacuum: u32 = conn.query_one(\"PRAGMA auto_vacuum\")?;\n\
            if auto_vacuum != 2 {\n\
                conn.execute_one(\"PRAGMA auto_vacuum = incremental\")?;\n\
                conn.execute_one(\"VACUUM\")?;\n\
            }\n\
            conn.execute_one(\"PRAGMA incremental_vacuum\")?;\n\
            conn.execute_one(\"PRAGMA optimize\")?;\n\
            conn.execute_one(\"PRAGMA wal_checkpoint(PASSIVE)\")?;\n\
            Ok(())\n\
        }\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[HELPER, (PATH, include_str!("../../rules/duplicated-code/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!((found[0].path.as_str(), found[0].line), (PATH, 4));
        assert!(found[0].message.contains("also appear in `vacuum`"), "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[HELPER, (PATH, include_str!("../../rules/duplicated-code/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_identical_bodies_once_and_skips_migrations() {
        let body = "{\n    let a: u32 = conn.query_one(\"SELECT count(*) FROM moz_places\")?;\n    let b: u32 = conn.query_one(\"SELECT count(*) FROM moz_bookmarks\")?;\n    let c: u32 = conn.query_one(\"SELECT count(*) FROM moz_origins\")?;\n    let d: u32 = conn.query_one(\"SELECT count(*) FROM moz_keywords\")?;\n    Ok(a.max(b).max(c).max(d))\n}\n";
        let src = format!("fn one(conn: &Conn) -> Result<u32> {body}fn two(conn: &Conn) -> Result<u32> {body}fn upgrade_from_v5(conn: &Conn) -> Result<u32> {body}");
        let found = check(&RULE, &[(PATH, &src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.starts_with("`two` has the same body as `one`"));
    }
}
