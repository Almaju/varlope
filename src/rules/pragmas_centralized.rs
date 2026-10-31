//! PRAGMAs are set only in the connection initializer and documented
//!
//! Docs and examples: `rules/pragmas-centralized/`.

use syn::visit::Visit;

use crate::helpers::sql::{self, FnCtx, TokKind};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "pragmas-centralized",
    area: Area::Sql,
    title: "PRAGMAs are set only in the connection initializer and documented",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const DOC: &str = "docs/design/db-pragmas.md";

/// Pragmas that run an operation rather than configure the connection.
const ACTIONS: &[&str] = &[
    "optimize",
    "wal_checkpoint",
    "incremental_vacuum",
    "shrink_memory",
    "integrity_check",
    "quick_check",
    "foreign_key_check",
];

/// Pragmas that only read, even with an argument.
const READS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "table_list",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "function_list",
    "module_list",
    "pragma_list",
    "collation_list",
    "database_list",
    "compile_options",
];

/// The schema version bookkeeping that migrations own.
const VERSIONING: &[&str] = &[
    "user_version",
    "application_id",
    "schema_version",
    "data_version",
];

/// A pragma assignment found in the code.
struct Setting {
    path: String,
    line: usize,
    name: String,
    func: Option<FnCtx>,
}

fn check(cx: &Context, out: &mut Findings) {
    let mut settings = Vec::new();
    for text in sql::sql_texts(cx.ws) {
        if text.in_test || text.from_file {
            continue;
        }
        for stmt in text.statements() {
            if let Some(name) = assigned_pragma(&stmt.toks) {
                settings.push(Setting {
                    path: text.path.clone(),
                    line: text.line_at(stmt.start()),
                    name,
                    func: text.func.clone(),
                });
            }
        }
    }
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) {
            continue;
        }
        let mut calls = PragmaCalls { found: Vec::new() };
        calls.visit_file(&file.ast);
        let fns = sql::functions(file);
        for (line, name) in calls.found {
            let func = fns
                .iter()
                .filter(|(f, _, _)| f.start_line <= line && line <= f.end_line)
                .max_by_key(|(f, _, _)| f.start_line)
                .map(|(f, _, _)| f.clone());
            settings.push(Setting {
                path: file.path.clone(),
                line,
                name,
                func,
            });
        }
    }
    let doc = cx.ws.file(DOC).map(|f| f.text.to_ascii_lowercase());
    for s in settings {
        if !s.path.starts_with("components/") && !s.path.starts_with("megazords/") {
            continue;
        }
        if is_temporary_or_maintenance(s.func.as_ref()) {
            continue;
        }
        if !sets_up_connection(s.func.as_ref()) {
            out.add(
                &s.path,
                s.line,
                format!(
                    "`PRAGMA {}` is set outside the connection setup; move it into `ConnectionInitializer::prepare` so every connection gets it",
                    s.name
                ),
            );
        } else if let Some(doc) = &doc {
            if !s.name.starts_with("cipher")
                && !doc.contains(&format!("[{}]", s.name.to_ascii_lowercase()))
            {
                out.add(
                    &s.path,
                    s.line,
                    format!(
                        "`PRAGMA {}` is not listed in {DOC}; add a row for it",
                        s.name
                    ),
                );
            }
        }
    }
}

/// `PRAGMA name = value` or `PRAGMA name(value)` that configures the
/// connection: the pragma's name.
fn assigned_pragma(toks: &[sql::Tok]) -> Option<String> {
    if !toks.first()?.is("PRAGMA") {
        return None;
    }
    let mut i = 1;
    if toks.get(i + 1).is_some_and(|t| t.is_punct(".")) {
        i += 2;
    }
    let name = toks.get(i)?;
    if name.kind != TokKind::Word {
        return None;
    }
    let next = toks.get(i + 1)?;
    if !(next.is_punct("=") || next.is_punct("(")) {
        return None;
    }
    let lower = name.text.to_ascii_lowercase();
    if ACTIONS.contains(&lower.as_str())
        || READS.contains(&lower.as_str())
        || VERSIONING.contains(&lower.as_str())
    {
        return None;
    }
    Some(name.text.to_string())
}

/// A guard that toggles a pragma and restores it (places' `PragmaGuard`), or
/// a maintenance function (`auto_vacuum` only takes effect with a `VACUUM`).
fn is_temporary_or_maintenance(func: Option<&FnCtx>) -> bool {
    func.is_some_and(|f| {
        f.self_ty.as_deref().is_some_and(|t| t.ends_with("Guard"))
            || f.name.contains("maintenance")
            || f.name.contains("vacuum")
    })
}

/// Where connection settings belong: the `ConnectionInitializer`, the shared
/// defaults, or a crate's connection constructor.
fn sets_up_connection(func: Option<&FnCtx>) -> bool {
    let Some(f) = func else {
        // A `const` or `static`: judged where it is executed.
        return true;
    };
    if f.trait_name.as_deref() == Some("ConnectionInitializer") {
        return true;
    }
    let name = f.name.as_str();
    matches!(
        name,
        "setup_sqlite_defaults" | "with_connection" | "new" | "drop"
    ) || [
        "open",
        "new_",
        "init",
        "setup",
        "prepare",
        "connect",
        "create_connection",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// `conn.pragma_update(None, "name", v)` and `conn.set_pragma("name", v)`.
struct PragmaCalls {
    found: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for PragmaCalls {
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let index = match m.method.to_string().as_str() {
            "pragma_update" | "pragma_update_and_check" => Some(1),
            "set_pragma" => Some(0),
            _ => None,
        };
        if let Some(syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        })) = index.and_then(|i| m.args.iter().nth(i))
        {
            let name = s.value();
            let lower = name.to_ascii_lowercase();
            if !ACTIONS.contains(&lower.as_str()) && !VERSIONING.contains(&lower.as_str()) {
                self.found.push((m.method.span().start().line, name));
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/autofill/src/db/store.rs";
    const DOC: (&str, &str) = (
        "docs/design/db-pragmas.md",
        "| [secure_delete](https://www.sqlite.org/pragma.html#pragma_secure_delete) | true | autofill |\n\
         | [journal_mode](https://www.sqlite.org/pragma.html#pragma_journal_mode) | WAL | autofill |\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                DOC,
                (
                    PATH,
                    include_str!("../../rules/pragmas-centralized/dont.rs"),
                ),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 11);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                DOC,
                (PATH, include_str!("../../rules/pragmas-centralized/do.rs")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_reads_actions_versions_and_guards() {
        let code = r#"
fn sync_finished(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "wal_checkpoint", "PASSIVE")?;
    let v: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA user_version = 3; PRAGMA optimize(0x12);")?;
    let cols = conn.prepare("PRAGMA table_info(moz_places)")?;
    Ok(())
}
impl PragmaGuard<'_> {
    fn new(conn: &Connection) -> Self { conn.pragma_update(None, "writable_schema", true).unwrap(); todo!() }
}
"#;
        let found = check(&RULE, &[DOC, (PATH, code)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn allows_auto_vacuum_in_maintenance() {
        let code = "pub fn vacuum(conn: &Connection) -> Result<()> {\n    conn.execute_one(\"PRAGMA auto_vacuum=incremental\")?;\n    conn.execute_one(\"VACUUM\")\n}\n";
        assert!(check(&RULE, &[DOC, (PATH, code)]).is_empty());
    }

    #[test]
    fn flags_undocumented_pragmas_in_setup() {
        let code = r#"
impl ConnectionInitializer for Init {
    fn prepare(&self, conn: &Connection, _db_empty: bool) -> Result<()> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        Ok(())
    }
}
"#;
        let found = check(&RULE, &[DOC, (PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("busy_timeout"));
    }
}
