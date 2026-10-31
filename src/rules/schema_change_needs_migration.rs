//! Any schema change bumps `VERSION`, adds an `upgrade_from` step and an upgrade test
//!
//! Docs and examples: `rules/schema-change-needs-migration/`.

use std::collections::BTreeMap;

use regex::Regex;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::diff::{FileChange, Status};
use crate::helpers::sql::{self, SqlText};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "schema-change-needs-migration",
    area: Area::Sql,
    title: "Any schema change bumps `VERSION`, adds an `upgrade_from` step and an upgrade test",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

/// What one PR changes in one crate.
#[derive(Default)]
struct CrateChange {
    old_ddl: BTreeMap<String, (String, usize)>,
    new_ddl: BTreeMap<String, (String, usize)>,
    /// `(path, line, from, to)` of each schema version bump.
    bumps: Vec<(String, usize, u64, u64)>,
    /// New texts of the crate's changed Rust files.
    rust: Vec<(String, String)>,
}

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    let mut crates: BTreeMap<String, CrateChange> = BTreeMap::new();
    for change in &diff.files {
        let path = change.path.as_str();
        if sql::skip_path(path)
            || is_test_path(path)
            || !(path.ends_with(".rs") || path.ends_with(".sql"))
        {
            continue;
        }
        let key = crate_key(cx, path);
        let entry = crates.entry(key).or_default();
        let new_text = change.new_text.clone().or_else(|| {
            (change.status != Status::Deleted)
                .then(|| cx.ws.file(path).map(|f| f.text.clone()))
                .flatten()
        });
        if change.status != Status::Added {
            let old_path = change.old_path.as_deref().unwrap_or(path);
            match &change.old_text {
                Some(old) => entry.old_ddl.extend(live_ddl(old_path, old)),
                None => entry.old_ddl.extend(ddl_in_lines(path, &change.removed)),
            }
            match (&change.old_text, &new_text) {
                (Some(_), Some(new)) => entry.new_ddl.extend(live_ddl(path, new)),
                _ => entry.new_ddl.extend(ddl_in_lines(path, &change.added)),
            }
        }
        entry.bumps.extend(version_bumps(change));
        if let (true, Some(text)) = (path.ends_with(".rs"), new_text) {
            entry.rust.push((path.to_string(), text));
        }
    }
    for (key, change) in &crates {
        let added: Vec<_> = change
            .new_ddl
            .iter()
            .filter(|(k, _)| !change.old_ddl.contains_key(*k))
            .collect();
        let removed: Vec<_> = change
            .old_ddl
            .iter()
            .filter(|(k, _)| !change.new_ddl.contains_key(*k))
            .collect();
        let schema_changed = !added.is_empty() || !removed.is_empty();
        if schema_changed && change.bumps.is_empty() {
            let (stmt, (path, line)) = added
                .first()
                .or(removed.first())
                .copied()
                .expect("a change");
            out.add(
                path,
                *line,
                format!(
                    "this changes the persistent schema (`{}`) without bumping the schema `VERSION`; existing databases will never get it, so bump it and add an `upgrade_from` step",
                    summary(stmt)
                ),
            );
        }
        for (path, line, _from, to) in &change.bumps {
            let Some(upgrades) = find_upgrades(cx, key, &change.rust, path) else {
                continue;
            };
            let step = to.saturating_sub(1);
            if !upgrades.covers(step) {
                out.add(
                    path,
                    *line,
                    format!("the schema version is now {to} but `upgrade_from` has no step for version {step}; add one"),
                );
            }
            if !has_upgrade_test(cx, key, &change.rust) {
                out.add(
                    path,
                    *line,
                    "this crate has no schema upgrade test; add a `MigratedDatabaseFile` test that runs `run_all_upgrades()` and `assert_schema_matches_new_database()`",
                );
            }
        }
        // A changed `upgrade_from` without a fallback arm.
        for (path, text) in &change.rust {
            let Some(upgrades) = upgrades_in(path, text) else {
                continue;
            };
            let Some(diff_file) = diff.file(path) else {
                continue;
            };
            let touched = diff_file
                .added
                .iter()
                .any(|(l, _)| upgrades.span.contains(l));
            if touched && upgrades.is_match && !upgrades.fallback {
                out.add(
                    path,
                    upgrades.match_line,
                    "`upgrade_from` has no `_ =>` arm; return `IncompatibleVersion` (or panic) for versions it does not know",
                );
            }
        }
    }
}

/// The crate a path belongs to: its package directory, or `components/<name>`.
fn crate_key(cx: &Context, path: &str) -> String {
    if let Some(p) = cx.ws.package_of(path) {
        return p.dir.clone();
    }
    let cut = ["/src/", "/sql/"]
        .iter()
        .filter_map(|m| path.find(m))
        .min()
        .unwrap_or(path.len());
    path[..cut].to_string()
}

/// `CREATE TABLE foo` from a normalized statement.
fn summary(stmt: &str) -> String {
    stmt.split(' ').take(3).collect::<Vec<_>>().join(" ")
}

/// Persistent DDL outside tests and migrations, normalized (tokens joined by
/// one space, so whitespace and comment edits do not count), with where it is.
fn live_ddl(path: &str, text: &str) -> BTreeMap<String, (String, usize)> {
    let texts: Vec<SqlText> = if path.ends_with(".sql") {
        vec![sql::sql_file(path, text)]
    } else {
        match RustFile::parse(path, text.to_string()) {
            Ok(file) => sql::rust_sql(&file).0,
            Err(_) => return BTreeMap::new(),
        }
    };
    let mut out = BTreeMap::new();
    for t in texts.iter().filter(|t| !t.in_test && !t.in_migration()) {
        for stmt in t.statements() {
            if stmt.ddl().is_some_and(|d| !d.temp) {
                out.entry(normalize(&stmt))
                    .or_insert((path.to_string(), t.line_at(stmt.start())));
            }
        }
    }
    out
}

/// DDL in the added (or removed) lines of a hand-written diff, which has no
/// whole files to place them in.
fn ddl_in_lines(path: &str, lines: &[(usize, String)]) -> BTreeMap<String, (String, usize)> {
    let joined: String = lines.iter().map(|(_, l)| format!("{l}\n")).collect();
    let text = sql::sql_file(path, &joined);
    let mut out = BTreeMap::new();
    for stmt in text.statements() {
        // Statements start inside string literals: skip to the DDL keyword.
        let Some(at) = stmt
            .toks
            .iter()
            .position(|t| t.is("CREATE") || t.is("DROP") || t.is("ALTER"))
        else {
            continue;
        };
        let rest = sql::Statement {
            toks: stmt.toks[at..].to_vec(),
        };
        if rest.ddl().is_some_and(|d| !d.temp) {
            let index = text.line_at(rest.start()) - 1;
            let line = lines.get(index).map(|(l, _)| *l).unwrap_or(0);
            out.entry(normalize(&rest))
                .or_insert((path.to_string(), line));
        }
    }
    out
}

fn normalize(stmt: &sql::Statement) -> String {
    stmt.toks
        .iter()
        .map(|t| t.text)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `const VERSION: u32 = 46;` going from one number to another.
fn version_bumps(change: &FileChange) -> Vec<(String, usize, u64, u64)> {
    if !change.path.ends_with(".rs") {
        return Vec::new();
    }
    let re = Regex::new(
        r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+((?:\w+_)?VERSION)\s*:\s*\w+\s*=\s*(\d+)\s*;",
    )
    .expect("valid regex");
    let parse = |lines: &[(usize, String)]| -> Vec<(String, usize, u64)> {
        lines
            .iter()
            .filter_map(|(n, l)| {
                let c = re.captures(l)?;
                Some((c[1].to_string(), *n, c[2].parse().ok()?))
            })
            .filter(|(name, _, _)| !name.contains("STORAGE") && !name.contains("RECORD"))
            .collect()
    };
    let old = parse(&change.removed);
    parse(&change.added)
        .into_iter()
        .filter_map(|(name, line, to)| {
            let (_, _, from) = old.iter().find(|(n, _, _)| *n == name)?;
            (to > *from).then(|| (change.path.clone(), line, *from, to))
        })
        .collect()
}

/// What an `upgrade_from` handles.
struct Upgrades {
    versions: Vec<(u64, u64)>,
    fallback: bool,
    is_match: bool,
    match_line: usize,
    span: std::ops::RangeInclusive<usize>,
}

impl Upgrades {
    fn covers(&self, version: u64) -> bool {
        self.versions
            .iter()
            .any(|(a, b)| *a <= version && version <= *b)
    }
}

/// The crate's `upgrade_from`: in the file that bumps the version, else in
/// the other changed files, else anywhere in the crate.
fn find_upgrades(
    cx: &Context,
    key: &str,
    rust: &[(String, String)],
    bumped: &str,
) -> Option<Upgrades> {
    let mut changed: Vec<&(String, String)> = rust.iter().collect();
    changed.sort_by_key(|(p, _)| p != bumped);
    for (path, text) in changed {
        if let Some(u) = upgrades_in(path, text) {
            return Some(u);
        }
    }
    let prefix = format!("{key}/");
    let mut files: Vec<_> = cx
        .ws
        .rust_files()
        .filter(|f| f.path.starts_with(&prefix) && !rust.iter().any(|(p, _)| *p == f.path))
        .collect();
    files.sort_by_key(|f| f.path != bumped);
    files
        .into_iter()
        .find_map(|f| upgrades_in(&f.path, &f.text))
}

fn upgrades_in(path: &str, text: &str) -> Option<Upgrades> {
    if !text.contains("upgrade_from") {
        return None;
    }
    let file = RustFile::parse(path, text.to_string()).ok()?;
    let mut finder = UpgradeFn { found: None };
    finder.visit_file(&file.ast);
    let (block, span) = finder.found?;
    let mut arms = Arms {
        upgrades: Upgrades {
            versions: Vec::new(),
            fallback: false,
            is_match: false,
            match_line: 0,
            span,
        },
        in_match: false,
    };
    arms.visit_block(block);
    let u = arms.upgrades;
    (u.is_match || !u.versions.is_empty()).then_some(u)
}

struct UpgradeFn<'a> {
    found: Option<(&'a syn::Block, std::ops::RangeInclusive<usize>)>,
}

impl<'a> Visit<'a> for UpgradeFn<'a> {
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        if f.sig.ident == "upgrade_from" && self.found.is_none() {
            self.found = Some((&f.block, f.span().start().line..=f.span().end().line));
        }
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
        if f.sig.ident == "upgrade_from" && self.found.is_none() {
            self.found = Some((&f.block, f.span().start().line..=f.span().end().line));
        }
        syn::visit::visit_impl_item_fn(self, f);
    }
}

/// Version arms of the first `match` with integer patterns, and places-style
/// `migration(db, from, N, ..)` calls.
struct Arms {
    upgrades: Upgrades,
    in_match: bool,
}

impl<'ast> Visit<'ast> for Arms {
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        if self.upgrades.is_match || self.in_match {
            return syn::visit::visit_expr_match(self, m);
        }
        let mut versions = Vec::new();
        let mut fallback = false;
        for arm in &m.arms {
            fallback |= pattern_versions(&arm.pat, &mut versions);
        }
        if !versions.is_empty() {
            self.upgrades.versions.extend(versions);
            self.upgrades.fallback = fallback;
            self.upgrades.is_match = true;
            self.upgrades.match_line = m.match_token.span.start().line;
        }
        self.in_match = true;
        syn::visit::visit_expr_match(self, m);
        self.in_match = false;
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        let is_migration = matches!(&*c.func, syn::Expr::Path(p)
            if p.path.segments.last().is_some_and(|s| s.ident == "migration"));
        if is_migration {
            if let Some(v) = c.args.iter().nth(2).and_then(int_lit) {
                self.upgrades.versions.push((v, v));
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

/// Collect the versions a pattern matches; true for a catch-all.
fn pattern_versions(pat: &syn::Pat, out: &mut Vec<(u64, u64)>) -> bool {
    match pat {
        syn::Pat::Wild(_) | syn::Pat::Ident(_) => true,
        syn::Pat::Lit(l) => {
            if let syn::Lit::Int(i) = &l.lit {
                if let Ok(v) = i.base10_parse() {
                    out.push((v, v));
                }
            }
            false
        }
        syn::Pat::Range(r) => {
            let lo = r.start.as_deref().and_then(int_lit).unwrap_or(0);
            let hi = r.end.as_deref().and_then(int_lit).map(|h| match r.limits {
                syn::RangeLimits::Closed(_) => h,
                syn::RangeLimits::HalfOpen(_) => h.saturating_sub(1),
            });
            out.push((lo, hi.unwrap_or(u64::MAX)));
            false
        }
        syn::Pat::Or(o) => {
            // Visit every case: each one adds its versions.
            let mut fallback = false;
            for p in &o.cases {
                fallback |= pattern_versions(p, out);
            }
            fallback
        }
        syn::Pat::Paren(p) => pattern_versions(&p.pat, out),
        _ => false,
    }
}

fn int_lit(e: &syn::Expr) -> Option<u64> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => i.base10_parse().ok(),
        _ => None,
    }
}

/// Whether the crate has a test that upgrades an old database.
fn has_upgrade_test(cx: &Context, key: &str, rust: &[(String, String)]) -> bool {
    let markers = ["run_all_upgrades", "MigratedDatabaseFile", "upgrade_to("];
    let prefix = format!("{key}/");
    rust.iter()
        .map(|(_, t)| t.as_str())
        .chain(
            cx.ws
                .files()
                .filter(|f| f.path.starts_with(&prefix) && f.path.ends_with(".rs"))
                .map(|f| f.text.as_str()),
        )
        .any(|t| markers.iter().any(|m| t.contains(m)))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check_diff, check_unified};

    #[test]
    fn flags_dont() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/schema-change-needs-migration/dont.diff"),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(
            found[0].path,
            "components/ads-client/src/http_cache/connection_initializer.rs"
        );
        assert!(found[0].message.contains("VERSION"));
    }

    #[test]
    fn accepts_do() {
        let found = check_unified(
            &RULE,
            &[],
            include_str!("../../rules/schema-change-needs-migration/do.diff"),
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    const OLD: &str = r#"
pub const VERSION: u32 = 2;
pub const SQL: &str = "
CREATE TABLE t(a TEXT NOT NULL);
";
impl ConnectionInitializer for I {
    const END_VERSION: u32 = VERSION;
    fn init(&self, tx: &Transaction<'_>) -> Result<()> { tx.execute_batch(SQL) }
    fn upgrade_from(&self, tx: &Transaction<'_>, version: u32) -> Result<()> {
        match version {
            1 => Ok(()),
            _ => Err(Error::IncompatibleVersion(version)),
        }
    }
}
#[cfg(test)]
mod tests {
    const V1_SCHEMA: &str = "CREATE TABLE t(a TEXT);";
    fn test_all_upgrades() { MigratedDatabaseFile::new(I, V1_SCHEMA).run_all_upgrades(); }
}
"#;
    const PATH: &str = "components/x/src/schema.rs";

    #[test]
    fn whitespace_and_fixture_edits_are_not_schema_changes() {
        let new = OLD
            .replace(
                "CREATE TABLE t(a TEXT NOT NULL);",
                "CREATE TABLE t(\n    a TEXT NOT NULL -- the a\n);",
            )
            .replace("CREATE TABLE t(a TEXT);", "CREATE TABLE t(a TEXT, b TEXT);");
        let found = check_diff(&RULE, &[(PATH, OLD)], &[(PATH, &new)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn bump_needs_a_step() {
        let new = OLD
            .replace(
                "CREATE TABLE t(a TEXT NOT NULL);",
                "CREATE TABLE t(a TEXT NOT NULL);\nCREATE INDEX t_a ON t(a);",
            )
            .replace("VERSION: u32 = 2", "VERSION: u32 = 3");
        let found = check_diff(&RULE, &[(PATH, OLD)], &[(PATH, &new)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("no step for version 2"));
        let fixed = new.replace(
            "1 => Ok(()),",
            "1 => Ok(()),\n            2 => Ok(tx.execute_batch(\"CREATE INDEX t_a ON t(a);\")?),",
        );
        assert!(check_diff(&RULE, &[(PATH, OLD)], &[(PATH, &fixed)]).is_empty());
    }

    #[test]
    fn flags_a_changed_upgrade_from_without_fallback() {
        let new = OLD
            .replace(
                "CREATE TABLE t(a TEXT NOT NULL);",
                "CREATE TABLE t(a TEXT NOT NULL, b TEXT);",
            )
            .replace("VERSION: u32 = 2", "VERSION: u32 = 3")
            .replace(
                "_ => Err(Error::IncompatibleVersion(version)),",
                "2 => Ok(tx.execute_batch(\"ALTER TABLE t ADD b TEXT\")?),",
            );
        let found = check_diff(&RULE, &[(PATH, OLD)], &[(PATH, &new)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("`_ =>`"));
    }

    #[test]
    fn new_crates_and_temp_tables_need_nothing() {
        let found = check_diff(&RULE, &[], &[(PATH, OLD)]);
        assert!(found.is_empty(), "{found:#?}");
        let temp = OLD.replace(
            "fn init(&self, tx: &Transaction<'_>) -> Result<()> { tx.execute_batch(SQL) }",
            "fn init(&self, tx: &Transaction<'_>) -> Result<()> { tx.execute_batch(SQL) }\n    fn finish(&self, c: &Connection) -> Result<()> { c.execute_batch(\"CREATE TEMP TABLE s(a)\") }",
        );
        assert!(check_diff(&RULE, &[(PATH, OLD)], &[(PATH, &temp)]).is_empty());
    }
}
