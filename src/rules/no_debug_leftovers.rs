//! No `println!`/`eprintln!`/`dbg!` (or `printStackTrace`/`print`) in shipped code
//!
//! Docs and examples: `rules/no-debug-leftovers/`.

use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Kind, Rule};
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
