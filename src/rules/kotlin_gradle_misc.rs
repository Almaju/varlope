//! Kotlin/Gradle hygiene: injectable dispatchers, KDoc params, lazy task APIs, no lint baselines
//!
//! Docs and examples: `rules/kotlin-gradle-misc/`.

use std::sync::OnceLock;

use regex::Regex;

use crate::diff::Status;
use crate::helpers::api_mobile::{
    is_fml_template, is_generated_or_vendored, shipped_mobile_files, Code, Lang,
};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "kotlin-gradle-misc",
    area: Area::Mobile,
    title: "Kotlin/Gradle hygiene: injectable dispatchers, KDoc params, lazy task APIs, no lint baselines",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for (path, lang, code) in shipped_mobile_files(cx.ws) {
        if lang != Lang::Kotlin {
            continue;
        }
        dispatchers(&path, &code, out);
        if !is_fml_template(&path) {
            kdoc_params(&path, &code, out);
        }
    }
    for file in cx.ws.files() {
        let gradle = file.path.ends_with(".gradle") || file.path.ends_with(".gradle.kts");
        if gradle && !is_generated_or_vendored(&file.path) {
            eager_tasks(
                &file.path,
                &Code::scan(&file.text, Lang::Kotlin, false),
                out,
            );
        }
    }
    baselines(cx, out);
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// `Dispatchers.IO` hard-coded where the embedder can't replace it. A
/// parameter default (`context: CoroutineContext = Dispatchers.IO`) is the fix.
fn dispatchers(path: &str, code: &Code, out: &mut Findings) {
    static USE: OnceLock<Regex> = OnceLock::new();
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    let uses = regex(&USE, r"\bDispatchers\.(IO|Default|Unconfined)\b");
    let default = regex(&DEFAULT, r"\w+\s*:\s*[\w.<>?]+\s*=\s*$");
    for m in uses.find_iter(&code.code) {
        let before = &code.code[..m.start()];
        let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
        if paren_depth(before) > 0 && default.is_match(line) {
            continue;
        }
        out.add_at(
            path,
            code.line_of(m.start()),
            code.column_of(m.start()),
            format!(
                "`{}` is hard-coded; take a `CoroutineContext` parameter that defaults to it so apps can inject their own",
                m.as_str()
            ),
        );
    }
}

fn paren_depth(text: &str) -> i32 {
    text.bytes().fold(0, |d, b| match b {
        b'(' => d + 1,
        b')' => d - 1,
        _ => d,
    })
}

/// KDoc `@param` names that don't match the declaration below them.
fn kdoc_params(path: &str, code: &Code, out: &mut Findings) {
    static PARAM: OnceLock<Regex> = OnceLock::new();
    let param = regex(&PARAM, r"@param\s+\[?(\w+)");
    for comment in &code.comments {
        if !comment.text.starts_with("/**") {
            continue;
        }
        let Some((name, params)) = declaration_after(&code.code[comment.end..]) else {
            continue;
        };
        let documented: Vec<&str> = param
            .captures_iter(&comment.text)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();
        for cap in param.captures_iter(&comment.text) {
            let Some(m) = cap.get(1) else { continue };
            if params.iter().any(|p| p == m.as_str()) {
                continue;
            }
            let undocumented: Vec<&String> = params
                .iter()
                .filter(|p| !documented.contains(&p.as_str()))
                .collect();
            let hint = match undocumented.as_slice() {
                [one] => format!("; did you mean `@param {one}`?"),
                _ => String::new(),
            };
            let offset = comment.start + cap.get(0).map_or(0, |c| c.start());
            out.add_at(
                path,
                code.line_of(offset),
                code.column_of(offset),
                format!(
                    "KDoc `@param {}` names no parameter of `{name}`{hint}",
                    m.as_str()
                ),
            );
        }
    }
}

/// The `fun`/`class`/`constructor` right after a KDoc comment: its name and
/// parameter names (type parameters included). `None` for anything else.
fn declaration_after(code: &str) -> Option<(String, Vec<String>)> {
    static DECL: OnceLock<Regex> = OnceLock::new();
    let decl = regex(
        &DECL,
        r"^\s*(?:@\w+(?:\([^)]*\))?\s*)*(?:(?:public|private|internal|protected|override|open|abstract|final|suspend|inline|operator|infix|data|sealed|inner|enum|annotation|value|external|tailrec)\s+)*(fun|class|constructor)\b\s*(<[^>]*>)?\s*([\w.]*)\s*(<[^>]*>)?\s*(?:(?:private|internal|protected|public)?\s*(?:@\w+\s*)*constructor\s*)?\(",
    );
    let caps = decl.captures(code)?;
    let whole = caps.get(0)?;
    let keyword = caps.get(1)?.as_str();
    let name = caps.get(3).map_or("", |m| m.as_str());
    let name = name.rsplit('.').next().unwrap_or(name);
    let name = if keyword == "constructor" {
        "constructor".to_string()
    } else {
        name.to_string()
    };
    let mut params: Vec<String> = Vec::new();
    for generics in [caps.get(2), caps.get(4)].into_iter().flatten() {
        params.extend(
            generics
                .as_str()
                .trim_matches(['<', '>'])
                .split(',')
                .filter_map(|g| {
                    g.split_whitespace()
                        .find(|w| !matches!(*w, "in" | "out" | "reified"))
                })
                .map(|g| g.trim_end_matches(':').to_string()),
        );
    }
    let body = &code[whole.end()..];
    let mut depth = 1;
    let mut end = body.len();
    for (i, c) in body.char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let list = &body[..end];
    let mut depth = 0;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (i, c) in list.char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&list[start..]);
    for piece in pieces {
        let head = piece.split(':').next().unwrap_or("");
        if let Some(param) = head.split_whitespace().last() {
            if param.chars().all(|c| c.is_alphanumeric() || c == '_') && !param.is_empty() {
                params.push(param.to_string());
            }
        }
    }
    Some((name, params))
}

/// Eager Gradle task APIs that realize tasks at configuration time.
fn eager_tasks(path: &str, code: &Code, out: &mut Findings) {
    static EAGER: OnceLock<Regex> = OnceLock::new();
    let eager = regex(
        &EAGER,
        r"\btasks\s*\[|\btasks\.(getByName|create|findByName|getByPath)\s*\(|\btasks\.named\([^()]*\)\.get\(\)",
    );
    let mut seen = std::collections::BTreeSet::new();
    for m in eager.find_iter(&code.code) {
        if !seen.insert(code.line_of(m.start())) {
            continue; // one finding per line
        }
        let what = m.as_str();
        let fix = if what.contains("create") {
            "use `tasks.register(..)`"
        } else if what.contains(".get()") {
            "pass the `TaskProvider` itself"
        } else {
            "use `tasks.named(..)`"
        };
        let shown = if what.contains('[') {
            "tasks[..]".to_string()
        } else if what.contains(".get()") {
            "tasks.named(..).get()".to_string()
        } else {
            format!("{}(..)", what.split('(').next().unwrap_or(what).trim_end())
        };
        out.add_at(
            path,
            code.line_of(m.start()),
            code.column_of(m.start()),
            format!("`{shown}` realizes the task at configuration time and defeats task avoidance; {fix}"),
        );
    }
}

/// A PR that adds a lint baseline or new detekt baseline entries.
fn baselines(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    for change in &diff.files {
        let name = change.path.rsplit('/').next().unwrap_or(&change.path);
        if name == "lint-baseline.xml" && change.status == Status::Added {
            out.add(
                &change.path,
                1,
                "new Android lint baseline hides lint findings; fix them or suppress each one with a reason",
            );
        } else if name == "detekt-baseline.xml" {
            if let Some((line, _)) = change.added.iter().find(|(_, t)| t.contains("<ID>")) {
                let count = change
                    .added
                    .iter()
                    .filter(|(_, t)| t.contains("<ID>"))
                    .count();
                out.add(
                    &change.path,
                    *line,
                    format!("{count} new detekt baseline entr{} hide findings; fix them or `@Suppress` each one with a reason", if count == 1 { "y would" } else { "ies would" }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check, check_unified};

    const KT: &str =
        "components/nimbus/android/src/main/java/org/mozilla/experiments/nimbus/WorkQueue.kt";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(KT, include_str!("../../rules/kotlin-gradle-misc/dont.kt"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 7], "{found:#?}");
        assert!(found[1].message.contains("did you mean `@param limit`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(KT, include_str!("../../rules/kotlin-gradle-misc/do.kt"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn kdoc_on_constructors_generics_and_properties() {
        let src = r#"
/**
 * @param T the item type
 * @param store where items live
 */
class Holder<T>(private val store: Store) {
    /** @param url the URL. */
    @Throws(Exception::class)
    fun <R> get(@RawRes url: String, vararg rest: R): R = TODO()

    /** @param ignored not a function */
    val x = 1

    /**
     * @param The search keyword.
     */
    fun getBookmarkUrlForKeyword(keyword: String): String? = null
}
"#;
        let found = check(&RULE, &[(KT, src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 15);
        assert!(found[0].message.contains("`@param keyword`"));
    }

    #[test]
    fn flags_eager_gradle_task_apis() {
        let gradle = "def p = tasks.register(\"a\", Copy) {\n    dependsOn tasks[\"cargoBuild\"]\n}\ntasks.named(\"b\").configure {}\n// tasks.create(\"c\")\ntasks.create(\"d\")\n";
        let found = check(&RULE, &[("megazords/full/android/build.gradle", gradle)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 6], "{found:#?}");
    }

    #[test]
    fn flags_new_baseline_entries() {
        let diff = "--- a/detekt-baseline.xml\n+++ b/detekt-baseline.xml\n@@ -3 +3,2 @@\n   <CurrentIssues>\n+    <ID>LongMethod:Nimbus.kt$Nimbus$fun x()</ID>\n--- /dev/null\n+++ b/components/places/android/lint-baseline.xml\n@@ -0,0 +1 @@\n+<issues/>\n";
        let found = check_unified(&RULE, &[], diff);
        assert_eq!(found.len(), 2, "{found:#?}");
        let removal = "--- a/detekt-baseline.xml\n+++ b/detekt-baseline.xml\n@@ -4 +3,0 @@\n-    <ID>LongMethod:Nimbus.kt$Nimbus$fun x()</ID>\n";
        assert!(check_unified(&RULE, &[], removal).is_empty());
    }
}
