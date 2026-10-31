//! No `println!`/`eprintln!`/`dbg!` (or `printStackTrace`/`print`) in shipped code
//!
//! Docs and examples: `rules/no-debug-leftovers/`.

use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "no-debug-leftovers",
    area: Area::DeadCode,
    title: "No `println!`/`eprintln!`/`dbg!` (or `printStackTrace`/`print`) in shipped code",
    kind: Kind::Code,
    clippy: &[
        "clippy::dbg_macro",
        "clippy::print_stdout",
        "clippy::print_stderr",
    ],
    in_tests: false,
    check,
};

const PRINTS: &[&str] = &["println", "print", "eprintln", "eprint", "dbg"];

/// Functions whose job is to print (`print_all_counts`, `dump_table`).
const PRINTING_FNS: &[&str] = &["print", "dump", "display", "show", "debug"];

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        if !idioms::is_library_code(cx.ws, &file.path) {
            continue;
        }
        for f in functions(&file.ast) {
            if f.is_test() || f.feature_gated() {
                continue;
            }
            let printing_fn = PRINTING_FNS.iter().any(|p| f.name.contains(p));
            let mut statements = StatementMacros::default();
            statements.visit_block(f.block);
            idioms::macros_in(f.block, |mac| {
                let name = syntax::macro_name(mac);
                if !PRINTS.contains(&name.as_str()) {
                    return;
                }
                if name != "dbg" && printing_fn {
                    return;
                }
                // `println!("cargo:..")` is the build-script protocol.
                if idioms::first_string_literal(mac).is_some_and(|s| s.starts_with("cargo:")) {
                    return;
                }
                let (line, column) = file.pos(&mac.path);
                let message = if name == "dbg" {
                    "`dbg!` left in shipped code; remove it".to_string()
                } else {
                    format!(
                        "`{name}!` in library code writes to the app's console; remove it or log with `error_support::{}!`",
                        if name.starts_with('e') { "warn" } else { "trace" }
                    )
                };
                out.add_at(&file.path, line, column, message);
                // A statement of its own, in a block that keeps others.
                if statements.0.get(&(line, column)) == Some(&true) {
                    if let (Some(fix), Some(last)) =
                        (delete_statement(file, mac), out.items.last_mut())
                    {
                        last.fix = Some(fix);
                    }
                }
            });
        }
    }
    for file in cx.ws.files_with_ext(&["kt", "swift"]) {
        let kotlin = file.extension() == "kt";
        let shipped = if kotlin {
            file.path.contains("/android/src/main/")
        } else {
            file.path.contains("/ios/") && !file.path.contains("Tests")
        };
        if !shipped || idioms::is_generated(&file.path) {
            continue;
        }
        for (n, line) in file.lines() {
            let code = line.split("//").next().unwrap_or("");
            let hit = if kotlin {
                [
                    "printStackTrace()",
                    "println(",
                    "System.out.print",
                    "System.err.print",
                ]
                .iter()
                .find(|p| code.contains(*p))
                .copied()
            } else {
                swift_print(code)
            };
            if let Some(what) = hit {
                out.add(
                    &file.path,
                    n,
                    format!("`{what}` in shipped code; remove it or log through the app's logger"),
                );
            }
        }
    }
}

/// Statement macros (`dbg!(x);`) by the position of their path, and whether
/// their block keeps another statement without them. A block left empty or
/// with only a tail expression is one rustfmt rewrites (`None => x,`), so
/// deleting the line alone would not do.
#[derive(Default)]
struct StatementMacros(BTreeMap<(usize, usize), bool>);

impl<'a> Visit<'a> for StatementMacros {
    fn visit_block(&mut self, block: &'a syn::Block) {
        for (i, stmt) in block.stmts.iter().enumerate() {
            if let syn::Stmt::Macro(m) = stmt {
                if m.semi_token.is_some() {
                    let others = block
                        .stmts
                        .iter()
                        .enumerate()
                        .any(|(j, s)| j != i && !matches!(s, syn::Stmt::Expr(_, None)));
                    let start = m.mac.path.span().start();
                    self.0.insert((start.line, start.column + 1), others);
                }
            }
        }
        syn::visit::visit_block(self, block);
    }
}

/// Delete the lines of a statement macro when it has them to itself (code
/// before it or after its `;` keeps it), with a blank line that would be
/// left doubled or opening/closing a block (rustfmt removes those).
fn delete_statement(file: &RustFile, mac: &syn::Macro) -> Option<Fix> {
    let range = file.range(mac.span());
    let text = &file.text;
    let line_start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    if !text[line_start..range.start].trim().is_empty() {
        return None;
    }
    let line_end = text[range.end..]
        .find('\n')
        .map_or(text.len(), |i| range.end + i);
    let rest = text[range.end..line_end]
        .trim_start()
        .strip_prefix(';')?
        .trim();
    if !(rest.is_empty() || rest.starts_with("//")) {
        return None;
    }
    let (first, last) = (file.line_of(range.start), file.line_of(range.end));
    let lines: Vec<&str> = text.lines().collect();
    let blank = |n: usize| n >= 1 && lines.get(n - 1).is_some_and(|l| l.trim().is_empty());
    let line = |n: usize| {
        n.checked_sub(1)
            .and_then(|i| lines.get(i))
            .map_or("", |l| l.trim())
    };
    let (mut from, mut to) = (first, last);
    if blank(from - 1) && (blank(to + 1) || line(to + 1).starts_with('}')) {
        from -= 1;
    } else if blank(to + 1) && line(from - 1).ends_with('{') {
        to += 1;
    }
    Some(Fix::delete(from, to - from + 1))
}

/// `print(` or `debugPrint(` as a free function call in a Swift line.
fn swift_print(code: &str) -> Option<&'static str> {
    for what in ["debugPrint(", "print("] {
        let mut from = 0;
        while let Some(at) = code[from..].find(what) {
            let start = from + at;
            let before = code[..start].chars().next_back();
            if !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                return Some(what);
            }
            from = start + what.len();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/no-debug-leftovers/dont.rs"))],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert_eq!(found[1].line, 5);
        // Each statement has its line to itself: the fix deletes it.
        for (f, line) in found.iter().zip([3, 5]) {
            let edit = &f.fix.as_ref().expect("a fix").edits[0];
            assert_eq!((edit.line, edit.remove), (line, 1));
            assert!(edit.insert.is_empty());
        }
    }

    #[test]
    fn deletes_lone_statements_only() {
        let src = "fn f(x: u8) -> u8 {\n    a();\n\n    println!(\"x = {x}\");\n\n    let y = dbg!(x);\n    dbg!(\n        y,\n    );\n    b(); eprintln!(\"b\");\n    match x {\n        0 => {\n            println!(\"zero\");\n            1\n        }\n        _ => x,\n    }\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        let fixes: Vec<(usize, Option<(usize, usize)>)> = found
            .iter()
            .map(|f| {
                let edit = f.fix.as_ref().map(|fix| &fix.edits[0]);
                (f.line, edit.map(|e| (e.line, e.remove)))
            })
            .collect();
        assert_eq!(
            fixes,
            vec![
                // The blank line above goes too, so no two are left in a row.
                (4, Some((3, 2))),
                // Part of a `let`.
                (6, None),
                // Over three lines.
                (7, Some((7, 3))),
                // Shares its line.
                (10, None),
                // The arm would keep only `1`, which rustfmt rewrites.
                (13, None),
            ],
            "{found:#?}"
        );
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/no-debug-leftovers/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_build_scripts_tests_and_printing_helpers() {
        let src = "fn link() { println!(\"cargo:rustc-link-lib=nss3\"); }\n\
                   pub fn print_table(&self) { println!(\"{}\", self); }\n\
                   #[cfg(feature = \"debug-tools\")]\n\
                   pub fn q() { eprintln!(\"x\"); }\n\
                   #[cfg(test)]\nmod tests { fn t() { println!(\"x\"); } }\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
        let main = "fn main() { println!(\"hello\"); }";
        assert!(check(&RULE, &[("components/x/src/main.rs", main)]).is_empty());
        assert!(check(&RULE, &[("components/x/build.rs", main)]).is_empty());
        assert!(check(&RULE, &[("examples/cli/src/lib.rs", main)]).is_empty());
    }

    #[test]
    fn flags_kotlin_and_swift_prints() {
        let kt = "fun f() {\n    try { g() } catch (e: Exception) { e.printStackTrace() }\n}\n";
        let swift = "func f() {\n    print(\"here\")\n    self.print(x) // a method\n}\n";
        let found = check(
            &RULE,
            &[
                ("components/places/android/src/main/java/A.kt", kt),
                ("components/places/ios/Places/A.swift", swift),
                ("components/places/android/src/test/java/B.kt", kt),
            ],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
    }
}
