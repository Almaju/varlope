//! No `!!`/force unwraps/force casts in Kotlin & Swift wrappers (incl. FML-generated code)
//!
//! Docs and examples: `rules/no-force-unwrap-wrappers/`.

use crate::helpers::api_mobile::{shipped_mobile_files, Code, Lang};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "no-force-unwrap-wrappers",
    area: Area::Mobile,
    title:
        "No `!!`/force unwraps/force casts in Kotlin & Swift wrappers (incl. FML-generated code)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for (path, lang, code) in shipped_mobile_files(cx.ws) {
        let text = cx.ws.file(&path).map_or("", |f| f.text.as_str());
        let lines: Vec<&str> = text.lines().collect();
        let hits = match lang {
            Lang::Kotlin => kotlin(&code),
            Lang::Swift => swift(&code),
        };
        for (offset, message) in hits {
            let line = code.line_of(offset);
            if lang == Lang::Kotlin && explained(&lines, line) {
                continue;
            }
            out.add_at(&path, line, code.column_of(offset), message);
        }
    }
}

/// `x!!`, outside comments and strings.
fn kotlin(code: &Code) -> Vec<(usize, &'static str)> {
    code.code
        .match_indices("!!")
        .map(|(i, _)| {
            (
                i,
                "`!!` throws if the value is ever null and crashes the app; use a local `val`, `?.let`/`?:`, or `requireNotNull(x) { \"why\" }`",
            )
        })
        .collect()
}

/// `x!`, `as!` and `try!`, outside comments and strings.
fn swift(code: &Code) -> Vec<(usize, &'static str)> {
    let bytes = code.code.as_bytes();
    let mut out = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'!' || i == 0 {
            continue;
        }
        let next = bytes.get(i + 1).copied().unwrap_or(b' ');
        if next == b'=' {
            continue; // `!=`, `!==`
        }
        let prev = bytes[i - 1];
        if !(prev.is_ascii_alphanumeric() || matches!(prev, b'_' | b')' | b']' | b'?')) {
            continue; // prefix `!` is a logical not
        }
        let line_start = code.code[..i].rfind('\n').map_or(0, |n| n + 1);
        let before = &code.code[line_start..i];
        if before.ends_with("try") && !ident_char_before(before, 3) {
            out.push((
                i - 3,
                "`try!` crashes the app on any thrown error; use `try` and handle or rethrow it",
            ));
        } else if before.ends_with("as") && !ident_char_before(before, 2) {
            out.push((
                i - 2,
                "`as!` crashes the app on a type mismatch; use `as?` and handle the failure",
            ));
        } else if !is_type_annotation(before) {
            out.push((
                i,
                "force unwrap `!` crashes the app on nil; use `guard let`/`if let` or `??` with a default",
            ));
        }
    }
    out
}

fn ident_char_before(before: &str, keyword_len: usize) -> bool {
    before[..before.len() - keyword_len]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// `var x: Foo!` or `-> Foo!`: an implicitly unwrapped optional type, not an
/// unwrap.
fn is_type_annotation(before: &str) -> bool {
    let trimmed = before.trim_end_matches(|c: char| {
        c.is_alphanumeric() || matches!(c, '_' | '.' | '<' | '>' | '[' | ']' | ',' | ' ')
    });
    trimmed.ends_with(':') || trimmed.ends_with("->")
}

/// A `// SAFETY:` comment right above the statement, or an
/// `@Suppress("UnsafeCallOnNullableType")` on the enclosing function, says
/// why the value can't be null.
fn explained(lines: &[&str], line: usize) -> bool {
    let mut k = line.saturating_sub(1);
    while k > 0 {
        let above = lines.get(k - 1).map_or("", |l| l.trim());
        let is_comment =
            above.starts_with("//") || above.starts_with('*') || above.starts_with("/*");
        if !is_comment {
            break;
        }
        if above.contains("SAFETY") {
            return true;
        }
        k -= 1;
    }
    // From the annotations of the enclosing `fun` down to the line itself.
    let text = |k: usize| lines.get(k.wrapping_sub(1)).copied().unwrap_or("");
    let mut start = (1..=line)
        .rev()
        .find(|&k| text(k).contains("fun "))
        .unwrap_or(line);
    while start > 1 && text(start - 1).trim_start().starts_with('@') {
        start -= 1;
    }
    (start..=line)
        .any(|k| text(k).contains("@Suppress") && text(k).contains("UnsafeCallOnNullableType"))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const KT: &str =
        "components/nimbus/android/src/main/java/org/mozilla/experiments/nimbus/NimbusBuilder.kt";
    const SWIFT: &str =
        "components/support/nimbus-fml/src/backends/swift/templates/FeatureTemplate.swift";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                KT,
                include_str!("../../rules/no-force-unwrap-wrappers/dont.kt"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                KT,
                include_str!("../../rules/no-force-unwrap-wrappers/do.kt"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_strings_comments_and_tests() {
        let src = "// a!! b\nval s = \"x!!\"\nval t = \"${y!!.z}\"\n";
        assert!(check(&RULE, &[(KT, src)]).is_empty());
        let test = "components/nimbus/android/src/test/java/NimbusTest.kt";
        assert!(check(&RULE, &[(test, "val a = b!!\n")]).is_empty());
    }

    #[test]
    fn honours_safety_comments_and_suppress() {
        let src = "// SAFETY: assigned in the lambda above.\nfoo(events!!)\n";
        assert!(check(&RULE, &[(KT, src)]).is_empty());
        let src = "@Suppress(\"UnsafeCallOnNullableType\")\nfun f() {\n    g(x!!)\n}\n";
        assert!(check(&RULE, &[(KT, src)]).is_empty());
    }

    #[test]
    fn flags_swift_unwraps_casts_and_try() {
        let src = "let a = b!\nlet c = d as! String\nlet e = try! f()\nif !g && h != i {}\nvar j: Int!\nlet k = \"\\(l)!\"\n";
        let found = check(&RULE, &[(SWIFT, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 2, 3], "{found:#?}");
    }

    #[test]
    fn reads_templates_without_askama_tags() {
        let src = "{% if !x.is_empty() %}\nlet a = {{ name }}!\n{# b! #}\n";
        let found = check(&RULE, &[(SWIFT, src)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }
}
