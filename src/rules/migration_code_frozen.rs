//! Migrations use frozen SQL and fixtures; tests assert the `VERSION` constant
//!
//! Docs and examples: `rules/migration-code-frozen/`.

use std::collections::{BTreeMap, BTreeSet};

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::diff::{FileChange, Status};
use crate::helpers::sql;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "migration-code-frozen",
    area: Area::Sql,
    title: "Migrations use frozen SQL and fixtures; tests assert the `VERSION` constant",
    kind: Kind::Diff,
    clippy: &[],
    // Fixtures and version assertions live in tests; the live-schema check
    // filters test code itself.
    in_tests: true,
    check,
};

/// Accessors of a stored schema version.
const VERSION_READS: &[&str] = &[
    "user_version",
    "DB_KEY_DB_VERSION",
    "migrated_version",
    "schema_version",
    "get_schema_version",
    "db_version",
];

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    for change in &diff.files {
        let path = change.path.as_str();
        if sql::skip_path(path) || change.status == Status::Deleted {
            continue;
        }
        if path.ends_with(".sql") {
            edited_sql_fixture(change, out);
            continue;
        }
        if !path.ends_with(".rs") {
            continue;
        }
        let Some(text) = change
            .new_text
            .clone()
            .or_else(|| cx.ws.file(path).map(|f| f.text.clone()))
        else {
            continue;
        };
        let Ok(file) = RustFile::parse(path, text) else {
            continue;
        };
        let added: BTreeSet<usize> = change.added.iter().map(|(l, _)| *l).collect();
        let moved: BTreeSet<String> = change
            .removed
            .iter()
            .map(|(_, t)| t.trim().to_string())
            .collect();
        let fresh = |line: usize| {
            added.contains(&line)
                && change
                    .added
                    .iter()
                    .find(|(l, _)| *l == line)
                    .is_some_and(|(_, t)| !moved.contains(t.trim()))
        };
        live_schema_in_migrations(&file, &fresh, out);
        version_literals(&file, &fresh, out);
        if let Some(old) = &change.old_text {
            edited_rust_fixtures(change, old, &file, out);
        }
    }
}

/// A new migration step that runs the live schema (`CREATE_SHARED_SCHEMA_SQL`,
/// the `SQL` that `init` runs): later edits to it would change what this
/// step does.
fn live_schema_in_migrations(file: &RustFile, fresh: &dyn Fn(usize) -> bool, out: &mut Findings) {
    let live = live_schema_consts(file);
    for (func, block, _) in sql::functions(file) {
        if !sql::is_migration_fn(&func.name) || file.in_test(func.start_line) {
            continue;
        }
        let mut v = SchemaUses {
            live: &live,
            version: None,
            hits: Vec::new(),
        };
        v.visit_block(block);
        for (line, ident, version) in v.hits {
            if version == Some(0) || !fresh(line) {
                continue;
            }
            out.add(
                &file.path,
                line,
                format!(
                    "this migration step runs the live schema `{ident}`, so later schema edits change what it does; inline the DDL this step needs"
                ),
            );
        }
    }
}

/// Constants that an `init` function executes: the live schema.
fn live_schema_consts(file: &RustFile) -> BTreeSet<String> {
    let (_, uses) = sql::rust_sql(file);
    uses.into_iter()
        .filter(|u| {
            u.func
                .as_ref()
                .is_some_and(|f| f.name == "init" || f.name.starts_with("create_schema"))
        })
        .map(|u| u.ident)
        .collect()
}

/// `CREATE_SHARED_SCHEMA_SQL` and friends (not temp tables or triggers,
/// which are recreated on every open anyway).
fn is_schema_const(name: &str) -> bool {
    name.starts_with("CREATE_") && name.ends_with("_SQL") && !name.contains("TEMP")
}

/// Uses of live-schema constants in a migration function, with the version
/// of the step they are in (`N =>` arm or `migration(db, from, N, ..)`).
struct SchemaUses<'a> {
    live: &'a BTreeSet<String>,
    version: Option<u64>,
    hits: Vec<(usize, String, Option<u64>)>,
}

impl<'ast> Visit<'ast> for SchemaUses<'_> {
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let saved = self.version;
        if let syn::Pat::Lit(l) = &arm.pat {
            if let syn::Lit::Int(i) = &l.lit {
                self.version = i.base10_parse().ok();
            }
        }
        syn::visit::visit_arm(self, arm);
        self.version = saved;
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        let saved = self.version;
        let is_migration = matches!(&*c.func, syn::Expr::Path(p)
            if p.path.segments.last().is_some_and(|s| s.ident == "migration"));
        if is_migration {
            if let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(i),
                ..
            })) = c.args.iter().nth(2)
            {
                self.version = i.base10_parse().ok();
            }
        }
        syn::visit::visit_expr_call(self, c);
        self.version = saved;
    }

    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if let Some(last) = p.path.segments.last() {
            let name = last.ident.to_string();
            if self.live.contains(&name) || is_schema_const(&name) {
                self.hits
                    .push((last.ident.span().start().line, name, self.version));
            }
        }
    }
}

/// `assert_eq!(…user_version…, 3)` or `migrated_version: Some(3)` added to a
/// test: the next version bump has to edit it. Tests that upgrade to a
/// literal version on purpose (`upgrade_to(43)`) are fine.
fn version_literals(file: &RustFile, fresh: &dyn Fn(usize) -> bool, out: &mut Findings) {
    for (func, block, _) in sql::functions(file) {
        if !file.in_test(func.start_line) {
            continue;
        }
        let mut v = VersionAsserts {
            hits: Vec::new(),
            pinned: false,
        };
        v.visit_block(block);
        if v.pinned {
            continue;
        }
        for (line, literal) in v.hits {
            if fresh(line) {
                out.add(
                    &file.path,
                    line,
                    format!("this test pins the stored schema version to `{literal}`; compare with the `VERSION` constant so the next bump does not have to edit it"),
                );
            }
        }
    }
}

struct VersionAsserts {
    hits: Vec<(usize, String)>,
    /// The test upgrades to a literal version on purpose.
    pinned: bool,
}

impl<'ast> Visit<'ast> for VersionAsserts {
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if m.method == "upgrade_to" && m.args.first().and_then(int_value).is_some() {
            self.pinned = true;
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_field_value(&mut self, f: &'ast syn::FieldValue) {
        if let syn::Member::Named(name) = &f.member {
            if name == "migrated_version" {
                if let Some(v) = int_value(&f.expr).filter(|v| *v != 0) {
                    self.hits.push((f.expr.span().start().line, v.to_string()));
                }
            }
        }
        syn::visit::visit_field_value(self, f);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let name = crate::syntax::macro_name(m);
        let Ok(args) = m.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) else {
            return;
        };
        if name == "assert_eq" && args.len() >= 2 {
            let (a, b) = (&args[0], &args[1]);
            for (reads, literal) in [(a, b), (b, a)] {
                let mentions = crate::syntax::tokens(reads);
                if VERSION_READS.iter().any(|r| mentions.contains(r)) {
                    if let Some(v) = int_value(literal).filter(|v| *v != 0) {
                        self.hits.push((literal.span().start().line, v.to_string()));
                    }
                }
            }
        }
        for e in &args {
            self.visit_expr(e);
        }
    }
}

/// `3`, `Some(3)`, `3u32`.
fn int_value(e: &syn::Expr) -> Option<u64> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => i.base10_parse().ok(),
        syn::Expr::Call(c) if matches!(&*c.func, syn::Expr::Path(p) if p.path.is_ident("Some")) => {
            c.args.first().and_then(int_value)
        }
        syn::Expr::Paren(p) => int_value(&p.expr),
        _ => None,
    }
}

/// A historical schema snapshot (`sql/tests/create_v15_db.sql`) edited in
/// place: upgrade tests then start from a database no user has.
fn edited_sql_fixture(change: &FileChange, out: &mut Findings) {
    let path = change.path.as_str();
    let name = path.rsplit('/').next().unwrap_or(path);
    let historical = is_test_path(path) && name.chars().any(|c| c.is_ascii_digit());
    if !historical || change.status != Status::Modified {
        return;
    }
    if let Some(line) = first_real_edit(&change.removed, &change.added) {
        out.add(
            path,
            line,
            "this edits a snapshot of an old schema; leave historical fixtures as they were and add a new one for the new version",
        );
    }
}

/// The new-side line of the first removed line that is not just re-indented.
fn first_real_edit(removed: &[(usize, String)], added: &[(usize, String)]) -> Option<usize> {
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    let re_added: BTreeSet<String> = added.iter().map(|(_, t)| squash(t)).collect();
    let (old_line, _) = removed
        .iter()
        .find(|(_, t)| !t.trim().is_empty() && !re_added.contains(&squash(t)))?;
    // Point at the nearest added line, or where the removal was.
    Some(
        added
            .iter()
            .map(|(l, _)| *l)
            .min_by_key(|l| l.abs_diff(*old_line))
            .unwrap_or(*old_line),
    )
}

/// Edits to `V16_SCHEMA`-style constants that the new file still has.
fn edited_rust_fixtures(change: &FileChange, old: &str, new: &RustFile, out: &mut Findings) {
    let Ok(old_file) = RustFile::parse(&change.path, old.to_string()) else {
        return;
    };
    let old_consts = fixture_consts(&old_file);
    let new_consts: BTreeSet<String> = fixture_consts(new).into_keys().collect();
    let mut per_const: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for (line, text) in &change.removed {
        if let Some((name, _)) = old_consts.iter().find(|(_, r)| r.contains(line)) {
            per_const
                .entry(name.clone())
                .or_default()
                .push((*line, text.clone()));
        }
    }
    for (name, removed) in per_const {
        if !new_consts.contains(&name) {
            continue; // the fixture was retired with its test
        }
        if let Some(line) = first_real_edit(&removed, &change.added) {
            out.add(
                &change.path,
                line,
                format!("this edits `{name}`, a snapshot of an old schema; leave it as it was and add a new snapshot for the new version"),
            );
        }
    }
}

/// Constants in test code named like an old-version snapshot (`V16_SCHEMA`,
/// `CREATE_V1_DB`), with their line ranges.
fn fixture_consts(file: &RustFile) -> BTreeMap<String, std::ops::RangeInclusive<usize>> {
    let mut out = BTreeMap::new();
    for item in all_consts(&file.ast) {
        let name = item.ident.to_string();
        let versioned = name.split('_').any(|part| {
            part.len() > 1 && part.starts_with('V') && part[1..].chars().all(|c| c.is_ascii_digit())
        });
        let line = item.span().start().line;
        if versioned && file.in_test(line) {
            out.insert(name, line..=item.span().end().line);
        }
    }
    out
}

fn all_consts(file: &syn::File) -> Vec<&syn::ItemConst> {
    struct V<'a>(Vec<&'a syn::ItemConst>);
    impl<'a> Visit<'a> for V<'a> {
        fn visit_item_const(&mut self, c: &'a syn::ItemConst) {
            self.0.push(c);
        }
    }
    let mut v = V(Vec::new());
    v.visit_file(file);
    v.0
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check_diff;

    const PATH: &str = "components/places/src/db/schema.rs";

    fn wrap(step: &str) -> String {
        format!(
            "const CREATE_SHARED_SCHEMA_SQL: &str = include_str!(\"../../sql/create_shared_schema.sql\");\n\
             pub fn init(db: &Connection) -> Result<()> {{ db.execute_batch(CREATE_SHARED_SCHEMA_SQL) }}\n\
             {step}\n"
        )
    }

    /// Before the PR: no step for version 20 yet.
    const BEFORE: &str = "pub fn upgrade_from(db: &Connection, from: u32) -> rusqlite::Result<()> {
    match from {
        // …
        _ => panic!(\"Places does not have a v{} -> v{} migration\", from, from + 1),
    }
    Ok(())
}";

    #[test]
    fn flags_dont() {
        let old = wrap(BEFORE);
        let new = wrap(include_str!("../../rules/migration-code-frozen/dont.rs"));
        let found = check_diff(&RULE, &[(PATH, &old)], &[(PATH, &new)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("CREATE_SHARED_SCHEMA_SQL"));
    }

    #[test]
    fn accepts_do() {
        let old = wrap(BEFORE);
        let new = wrap(include_str!("../../rules/migration-code-frozen/do.rs"));
        let found = check_diff(&RULE, &[(PATH, &old)], &[(PATH, &new)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn existing_and_initial_steps_are_fine() {
        let old = wrap("pub fn upgrade_from(db: &Connection, from: u32) -> Result<()> {\n    migration(db, from, 2, &[CREATE_SHARED_SCHEMA_SQL], || Ok(()))?;\n    Ok(())\n}");
        // Reformatting an existing step, and replaying the schema from v0.
        let new = wrap("pub fn upgrade_from(db: &Connection, from: u32) -> Result<()> {\n    match from {\n        0 => db.execute_batch(CREATE_SHARED_SCHEMA_SQL)?,\n        _ => {}\n    }\n    migration(db, from, 2, &[CREATE_SHARED_SCHEMA_SQL], || Ok(()))?;\n    Ok(())\n}");
        let found = check_diff(&RULE, &[(PATH, &old)], &[(PATH, &new)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_edited_fixtures_and_literal_versions() {
        let old = "#[cfg(test)]\nmod tests {\n    const V16_SCHEMA: &str = \"\n        CREATE TABLE t(a TEXT);\n    \";\n    #[test]\n    fn upgrades() {\n        assert_eq!(db.get(StoreId::Meta, DB_KEY_DB_VERSION)?, Some(DB_VERSION));\n    }\n}\n";
        let new = old
            .replace("CREATE TABLE t(a TEXT);", "CREATE TABLE t(a TEXT, b TEXT);")
            .replace("Some(DB_VERSION)", "Some(3)");
        let found = check_diff(&RULE, &[(PATH, old)], &[(PATH, &new)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("V16_SCHEMA"));
        assert!(found[1].message.contains("`3`"));
        // Re-indenting the fixture, or pinning a version on purpose, is fine.
        let ok = old
            .replace("        CREATE TABLE t(a TEXT);", "    CREATE TABLE t(a TEXT);")
            .replace("assert_eq!(db.get(StoreId::Meta, DB_KEY_DB_VERSION)?, Some(DB_VERSION));", "db_file.upgrade_to(43);\n        assert_eq!(db.get(StoreId::Meta, DB_KEY_DB_VERSION)?, Some(43));");
        assert!(check_diff(&RULE, &[(PATH, old)], &[(PATH, &ok)]).is_empty());
    }

    #[test]
    fn flags_edited_sql_snapshots() {
        let path = "components/places/sql/tests/create_v15_db.sql";
        let old = "CREATE TABLE moz_places(id INTEGER PRIMARY KEY);\n";
        let new = "CREATE TABLE moz_places(id INTEGER PRIMARY KEY, frecency INTEGER);\n";
        assert_eq!(check_diff(&RULE, &[(path, old)], &[(path, new)]).len(), 1);
        assert!(check_diff(&RULE, &[(path, old)], &[]).is_empty());
    }
}
