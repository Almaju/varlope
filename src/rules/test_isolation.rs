//! Tests are isolated: no wall clock, shared global state, fixed DB paths or prod endpoints
//!
//! Docs and examples: `rules/test-isolation/`.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "test-isolation",
    area: Area::Tests,
    title:
        "Tests are isolated: no wall clock, shared global state, fixed DB paths or prod endpoints",
    kind: Kind::Code,
    clippy: &[],
    in_tests: true,
    check,
};

/// Process-wide setters: tests run in parallel threads of one process.
const GLOBAL_SETTERS: &[&str] = &[
    "set_var",
    "remove_var",
    "set_max_level",
    "set_logger",
    "set_boxed_logger",
];

/// Calendar arithmetic whose result depends on the date the test runs.
/// (Fixed-length intervals relative to `now()` are fine.)
const CALENDAR_METHODS: &[&str] = &["weekday", "year", "month", "ordinal", "ordinal0", "iso_week"];

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        if file.is_test_file() && idioms::is_tooling(&file.path) {
            continue;
        }
        let fns = functions(&file.ast);
        // Helpers such as `same_week(now, then)` that do calendar arithmetic.
        let calendar_helpers: BTreeSet<String> = fns
            .iter()
            .filter(|f| {
                let mut v = Body::default();
                v.visit_block(f.block);
                v.calendar
            })
            .map(|f| f.name.clone())
            .collect();
        for f in &fns {
            let line = file.line_of_node(&f.sig.ident);
            if !file.in_test(line) {
                continue;
            }
            let mut v = Body::default();
            v.visit_block(f.block);
            v.calendar |= idioms::idents(f.block)
                .iter()
                .any(|i| calendar_helpers.contains(i));
            if f.is_test() {
                wall_clock(file, &v, out);
                if !syntax::has_attr(f.attrs, "serial") && !v.locks {
                    for call in &v.global_setters {
                        let (line, column) = file.pos(*call);
                        out.add_at(
                            &file.path,
                            line,
                            column,
                            "this test changes process-wide state while other tests run in parallel; hold a shared `static TEST_LOCK: Mutex<()>` for the whole test",
                        );
                    }
                }
            }
            for (lit, value) in &v.literals {
                if let Some(message) = fixed_path(value) {
                    let (line, column) = file.pos(*lit);
                    out.add_at(&file.path, line, column, message);
                }
            }
        }
    }
}

/// A test that reads the wall clock, does calendar arithmetic and asserts.
fn wall_clock(file: &RustFile, v: &Body, out: &mut Findings) {
    let Some(now) = v.now_calls.first() else {
        return;
    };
    if !v.calendar || !v.asserts {
        return;
    }
    let (line, column) = file.pos(*now);
    out.add_at(
        &file.path,
        line,
        column,
        "this test reads the wall clock and does calendar arithmetic (weekday, month, year), so its result depends on the day it runs; use a fixed timestamp",
    );
}

/// The message for a literal database location shared by every test run.
fn fixed_path(value: &str) -> Option<&'static str> {
    if value.contains('{') {
        return None;
    }
    if value.starts_with("file:") && value.contains("mode=memory") && value.contains("cache=shared")
    {
        return Some("a named shared-cache in-memory DB is shared by every test in the process; use `open_database::unique_in_memory_db_path()`");
    }
    if value.starts_with("/tmp/") {
        return Some("a fixed `/tmp` path is shared by parallel tests and earlier runs; use `tempfile::tempdir()`");
    }
    None
}

#[derive(Default)]
struct Body<'a> {
    now_calls: Vec<&'a syn::Path>,
    calendar: bool,
    asserts: bool,
    locks: bool,
    global_setters: Vec<&'a syn::Path>,
    literals: Vec<(&'a syn::LitStr, String)>,
}

impl<'a> Visit<'a> for Body<'a> {
    fn visit_expr_path(&mut self, p: &'a syn::ExprPath) {
        let path = &p.path;
        if ["Utc::now", "Local::now", "SystemTime::now"]
            .iter()
            .any(|s| syntax::path_ends_with(path, s))
        {
            self.now_calls.push(path);
        }
        let last = path.segments.last().map(|s| s.ident.to_string());
        if let Some(last) = last {
            let qualifier = path
                .segments
                .iter()
                .rev()
                .nth(1)
                .map(|s| s.ident.to_string());
            let global = GLOBAL_SETTERS.contains(&last.as_str())
                && match qualifier.as_deref() {
                    Some("env") => last.ends_with("_var"),
                    Some("log") => !last.ends_with("_var"),
                    None => last.contains("log") || last == "set_max_level",
                    _ => false,
                };
            if global {
                self.global_setters.push(path);
            }
        }
        syn::visit::visit_expr_path(self, p);
    }

    fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
        let name = m.method.to_string();
        if m.args.is_empty() && CALENDAR_METHODS.contains(&name.as_str()) {
            self.calendar = true;
        }
        if name == "lock" {
            self.locks = true;
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        if syntax::macro_name(mac).starts_with("assert") {
            self.asserts = true;
        }
        // Look inside macro arguments that parse as expressions.
        if let Ok(args) = mac.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            for arg in &args {
                let mut inner = Body::default();
                inner.visit_expr(arg);
                self.calendar |= inner.calendar;
                self.locks |= inner.locks;
                self.asserts |= inner.asserts;
                // The spans point into this file; the expressions do not
                // outlive `args`, so only flags are kept.
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_lit_str(&mut self, lit: &'a syn::LitStr) {
        self.literals.push((lit, lit.value()));
    }

    // Nested functions are visited on their own.
    fn visit_item_fn(&mut self, _: &'a syn::ItemFn) {}
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/testing/mod.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/test-isolation/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/test-isolation/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_wall_clock_calendar_tests_and_unlocked_globals() {
        let src = "#[cfg(test)]\nmod tests {\n\
            fn same_week(a: Date, b: Date) -> bool { a.iso_week() == b.iso_week() }\n\
            #[test]\nfn weeks() {\n\
                let now = Utc::now();\n\
                let then = now - Duration::days(1);\n\
                assert_eq!(same_week(now, then), store.query(Interval::Weeks));\n\
            }\n\
            #[test]\nfn logs() {\n\
                log::set_max_level(LevelFilter::Debug);\n\
            }\n\
            #[test]\nfn logs_locked() {\n\
                let _guard = TEST_LOCK.lock();\n\
                log::set_max_level(LevelFilter::Debug);\n\
            }\n\
        }\n";
        let found = check(&RULE, &[("components/nimbus/src/behavior.rs", src)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 6);
        assert_eq!(found[1].line, 12);
    }

    #[test]
    fn accepts_relative_days_and_production_code() {
        let src = "fn open() -> Db { Db::open(\"file:main?mode=memory&cache=shared\") }\n\
            #[cfg(test)]\nmod tests {\n\
            #[test]\nfn days() {\n\
                let then = Utc::now() - Duration::days(3);\n\
                assert_eq!(1.0, store.query(Interval::Months, then));\n\
            }\n\
        }\n";
        assert!(check(&RULE, &[("components/nimbus/src/behavior.rs", src)]).is_empty());
    }
}
