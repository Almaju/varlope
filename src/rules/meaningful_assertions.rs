//! Tests must actually assert: no bare `matches!`, mockito mocks asserted, messages match
//!
//! Docs and examples: `rules/meaningful-assertions/`.

use std::collections::{BTreeMap, BTreeSet};

use syn::punctuated::Punctuated;
use syn::visit::Visit;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "meaningful-assertions",
    area: Area::Tests,
    title: "Tests must actually assert: no bare `matches!`, mockito mocks asserted, messages match",
    kind: Kind::Code,
    clippy: &[],
    in_tests: true,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        let mut discarded = Discarded { hits: Vec::new() };
        discarded.visit_file(&file.ast);
        for mac in discarded.hits {
            let (line, column) = file.pos(&mac.path);
            out.add_at(
                &file.path,
                line,
                column,
                "`matches!(..);` computes a bool and throws it away; wrap it in `assert!(..)`",
            );
        }
        for f in functions(&file.ast) {
            unasserted_mocks(file, f.block, out);
            if f.is_test() {
                repeated_messages(file, f.block, out);
            }
        }
    }
}

/// `matches!(..);` as a statement.
struct Discarded<'a> {
    hits: Vec<&'a syn::Macro>,
}

impl<'a> Visit<'a> for Discarded<'a> {
    fn visit_stmt(&mut self, stmt: &'a syn::Stmt) {
        let mac = match stmt {
            syn::Stmt::Macro(m) if m.semi_token.is_some() => Some(&m.mac),
            syn::Stmt::Expr(syn::Expr::Macro(m), Some(_)) => Some(&m.mac),
            _ => None,
        };
        if let Some(mac) = mac {
            if syntax::macro_name(mac) == "matches" {
                self.hits.push(mac);
            }
        }
        syn::visit::visit_stmt(self, stmt);
    }
}

/// mockito mocks created with `.expect(n)` whose expectation nobody checks:
/// bound to a variable that is never `.assert()`ed, or dropped at once.
fn unasserted_mocks(file: &RustFile, block: &syn::Block, out: &mut Findings) {
    let mut mocks = Mocks {
        lets: Vec::new(),
        dropped: Vec::new(),
    };
    mocks.visit_block(block);
    for (name, expr) in mocks.lets {
        if name == "_" {
            let (line, column) = file.pos(expr);
            out.add_at(
                &file.path,
                line,
                column,
                "this mock is bound to `_` and dropped at once, so its `.expect(..)` is never checked; bind it and call `.assert()`",
            );
            continue;
        }
        // Mentioned again (asserted, returned, moved): not ours to judge.
        if idioms::count_ident(block, &name) > 1 {
            continue;
        }
        let (line, column) = file.pos(expr);
        out.add_at(
            &file.path,
            line,
            column,
            format!("`{name}` sets `.expect(..)` but is never asserted, so the call count is not checked; add `{name}.assert()` at the end of the test"),
        );
    }
    for expr in mocks.dropped {
        let (line, column) = file.pos(expr);
        out.add_at(
            &file.path,
            line,
            column,
            "this mock is dropped at the end of the statement, so its `.expect(..)` is never checked; bind it and call `.assert()`",
        );
    }
}

struct Mocks<'a> {
    /// `let name = <mock with .expect(n)>;`
    lets: Vec<(String, &'a syn::Expr)>,
    /// `<mock with .expect(n)>;` as a statement.
    dropped: Vec<&'a syn::Expr>,
}

impl<'a> Visit<'a> for Mocks<'a> {
    fn visit_local(&mut self, local: &'a syn::Local) {
        if let Some(init) = &local.init {
            if is_expecting_mock(&init.expr) {
                let name = match &local.pat {
                    syn::Pat::Ident(p) => Some(p.ident.to_string()),
                    syn::Pat::Wild(_) => Some("_".to_string()),
                    _ => None,
                };
                if let Some(name) = name {
                    self.lets.push((name, &init.expr));
                }
            }
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_stmt(&mut self, stmt: &'a syn::Stmt) {
        if let syn::Stmt::Expr(expr, Some(_)) = stmt {
            if is_expecting_mock(expr) {
                self.dropped.push(expr);
            }
        }
        syn::visit::visit_stmt(self, stmt);
    }

    // Nested functions and closures are visited on their own.
    fn visit_item_fn(&mut self, _: &'a syn::ItemFn) {}
}

/// A mockito builder chain: rooted at `mock(..)`/`server.mock(..)`, with an
/// `.expect(n)` (not `Result::expect("msg")`), ending in `.create()`.
fn is_expecting_mock(expr: &syn::Expr) -> bool {
    let mut expr = expr;
    if let syn::Expr::Await(a) = expr {
        expr = &a.base;
    }
    let syn::Expr::MethodCall(last) = expr else {
        return false;
    };
    if last.method != "create" && last.method != "create_async" {
        return false;
    }
    let mut expects = false;
    let mut cur = &*last.receiver;
    loop {
        match cur {
            syn::Expr::MethodCall(m) => {
                if (m.method == "expect" || m.method == "expect_at_least" || m.method == "expect_at_most")
                    && !matches!(
                        m.args.first(),
                        Some(syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(_),
                            ..
                        }))
                    )
                {
                    expects = true;
                }
                if m.method == "mock" {
                    return expects;
                }
                cur = &m.receiver;
            }
            syn::Expr::Call(c) => {
                return expects
                    && matches!(&*c.func, syn::Expr::Path(p) if syntax::path_ends_with(&p.path, "mock"));
            }
            _ => return false,
        }
    }
}

/// A message copied onto an assertion that checks something else: the same
/// literal as an earlier assertion in the test, naming a value the earlier one
/// checks and this one does not (`"Has correct suggestion_type"` on a check
/// of `dismissal_key`).
fn repeated_messages(file: &RustFile, block: &syn::Block, out: &mut Findings) {
    // message -> (line, idents of the checked expressions)
    let mut seen: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
    let mut asserts = Vec::new();
    idioms::macros_in(block, |mac| asserts.push(mac));
    for mac in asserts {
        let index = match syntax::macro_name(mac).as_str() {
            "assert" | "debug_assert" => 1,
            "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => 2,
            _ => continue,
        };
        let Ok(args) =
            mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };
        let Some(syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(message),
            ..
        })) = args.iter().nth(index)
        else {
            continue;
        };
        let text = message.value();
        let checked: BTreeSet<String> = args
            .iter()
            .take(index)
            .flat_map(idioms::idents)
            .collect();
        let line = file.line_of_node(&mac.path);
        let Some((first, earlier)) = seen.get(&text) else {
            seen.insert(text, (line, checked));
            continue;
        };
        let named = text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .find(|w| earlier.contains(*w) && !checked.contains(*w));
        if let Some(word) = named {
            out.add(
                &file.path,
                line,
                format!("assertion message copied from line {first}: it names `{word}`, which this assertion does not check; describe what it does check"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/http_cache.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/meaningful-assertions/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 4);
        assert!(found[0].message.contains("`_m`"));
        assert_eq!(found[1].line, 9);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/meaningful-assertions/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn accepts_matches_as_a_value_and_mocks_without_expect() {
        let src = "#[test]\nfn t() {\n\
            let _m = mock(\"GET\", \"/\").with_body(\"x\").create();\n\
            let ok = matches!(x, Some(_));\n\
            assert!(matches!(x, Some(_)));\n\
            let m = mock(\"GET\", \"/\").create();\n\
            m.expect(1).assert();\n\
            let v = get().expect(\"no value\");\n\
        }\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }

    #[test]
    fn flags_dropped_mocks_and_repeated_messages() {
        let src = "#[test]\nfn t() {\n\
            let _ = mock(\"GET\", \"/\").expect(1).create();\n\
            server.mock(\"GET\", \"/\").expect(2).create();\n\
            assert_eq!(kind, \"vpn\", \"has the right kind\");\n\
            assert_eq!(key, \"vpn-suggestions\", \"has the right kind\");\n\
            assert_eq!(kind, \"vpn\", \"has the right kind\");\n\
        }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 3, "{found:#?}");
        assert!(found[2].message.contains("copied from line 5: it names `kind`"));
    }
}
