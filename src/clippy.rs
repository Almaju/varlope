//! Rules that clippy already implements: run `cargo clippy` with their lints
//! and report its findings under varlope's rule ids.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context as _, Result};

use crate::rule::{Finding, Fix, Origin, Rule};
use crate::workspace::Workspace;

/// Settings for the lints that need configuration (`disallowed-*`) and that
/// should stay quiet in tests.
pub const CLIPPY_TOML: &str = r#"allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true
allow-dbg-in-tests = true
allow-print-in-tests = true
disallowed-types = [
  { path = "std::sync::Mutex", reason = "use parking_lot::Mutex" },
  { path = "std::sync::RwLock", reason = "use parking_lot::RwLock" },
  { path = "std::collections::hash_map::DefaultHasher", reason = "not stable across Rust releases; never persist its output" },
]
disallowed-macros = [
  { path = "log::trace", reason = "log through tracing_support" },
  { path = "log::debug", reason = "log through tracing_support" },
  { path = "log::info", reason = "log through tracing_support" },
  { path = "log::warn", reason = "log through tracing_support" },
  { path = "log::error", reason = "log through tracing_support" },
]
disallowed-methods = [
  { path = "alloc::string::String::truncate", reason = "panics off a char boundary; truncate on chars" },
]
disallowed-names = ["foo", "bar", "baz", "tmp", "nm", "og", "th", "ta"]
"#;

/// Run clippy over the workspace's libraries with every lint the given rules
/// list, and map what it reports back to those rules.
pub fn run(root: &Path, ws: &Workspace, rules: &[&'static Rule]) -> Result<Vec<Finding>> {
    let lints: BTreeSet<&str> = rules
        .iter()
        .flat_map(|r| r.clippy.iter().copied())
        .collect();
    if lints.is_empty() {
        return Ok(Vec::new());
    }
    let conf_dir = std::env::temp_dir().join(format!("varlope-clippy-{}", std::process::id()));
    std::fs::create_dir_all(&conf_dir)?;
    std::fs::write(conf_dir.join("clippy.toml"), CLIPPY_TOML)?;

    let mut cmd = Command::new("cargo");
    cmd.args([
        "clippy",
        "--workspace",
        "--lib",
        "--message-format=json",
        "--quiet",
        "--",
    ]);
    for lint in &lints {
        cmd.args(["-W", lint]);
    }
    let out = cmd
        .current_dir(root)
        .env("CLIPPY_CONF_DIR", &conf_dir)
        .output()
        .context("running cargo clippy")?;
    let _ = std::fs::remove_dir_all(&conf_dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() && !stdout.contains("\"reason\":\"compiler-message\"") {
        bail!(
            "cargo clippy failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .take(30)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(parse(&stdout, rules, Some(ws)))
}

/// Map clippy's JSON messages to findings.
pub fn parse(json_lines: &str, rules: &[&'static Rule], ws: Option<&Workspace>) -> Vec<Finding> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for line in json_lines.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"] != "compiler-message" {
            continue;
        }
        let message = &msg["message"];
        let Some(code) = message["code"]["code"].as_str() else {
            continue;
        };
        let text = message["message"].as_str().unwrap_or("");
        let Some(rule) = rule_for(code, text, rules) else {
            continue;
        };
        let spans = message["spans"].as_array().cloned().unwrap_or_default();
        let Some(span) = spans
            .iter()
            .find(|s| s["is_primary"].as_bool() == Some(true))
            .or(spans.first())
        else {
            continue;
        };
        let path = span["file_name"].as_str().unwrap_or("").to_string();
        let path_for_fix = path.clone();
        let line_no = span["line_start"].as_u64().unwrap_or(0) as usize;
        let column = span["column_start"].as_u64().unwrap_or(1) as usize;
        if !seen.insert((code.to_string(), path.clone(), line_no, column)) {
            continue;
        }
        out.push(Finding {
            rule: rule.id,
            path,
            line: line_no,
            column,
            message: format!("{text} ({code})"),
            origin: Origin::Clippy,
            commit: None,
            fix: ws
                .and_then(|ws| ws.file(&path_for_fix))
                .and_then(|file| suggestion_fix(message, &file.text)),
            snippet: None,
        });
    }
    out
}

/// Clippy's suggested replacements for one message, applied to the file's
/// lines and returned as one line edit.
fn suggestion_fix(message: &serde_json::Value, text: &str) -> Option<Fix> {
    // (line_start, col_start, line_end, col_end, replacement), 1-based, columns in chars.
    let mut subs: Vec<(usize, usize, usize, usize, String)> = Vec::new();
    let mut collect = |spans: &serde_json::Value| {
        for span in spans.as_array().into_iter().flatten() {
            let Some(replacement) = span["suggested_replacement"].as_str() else {
                continue;
            };
            let get = |k: &str| span[k].as_u64().unwrap_or(0) as usize;
            subs.push((
                get("line_start"),
                get("column_start"),
                get("line_end"),
                get("column_end"),
                replacement.to_string(),
            ));
        }
    };
    collect(&message["spans"]);
    for child in message["children"].as_array().into_iter().flatten() {
        collect(&child["spans"]);
    }
    if subs.is_empty() || subs.iter().any(|s| s.0 == 0 || s.1 == 0) {
        return None;
    }
    let first = subs.iter().map(|s| s.0).min()?;
    let last = subs.iter().map(|s| s.2).max()?;
    let lines: Vec<&str> = text
        .lines()
        .skip(first - 1)
        .take(last - first + 1)
        .collect();
    if lines.len() != last - first + 1 {
        return None;
    }
    let region: Vec<char> = lines.join("\n").chars().collect();
    // Char offset of (line, column) inside the region.
    let offset = |line: usize, col: usize| -> usize {
        lines[..line - first]
            .iter()
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            + col
            - 1
    };
    let mut edits: Vec<(usize, usize, String)> = subs
        .iter()
        .map(|(ls, cs, le, ce, r)| (offset(*ls, *cs), offset(*le, *ce), r.clone()))
        .collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = region.clone();
    let mut previous_start = usize::MAX;
    for (start, end, replacement) in edits {
        if start > end || end > out.len() || end > previous_start {
            return None; // overlapping or out of range
        }
        out.splice(start..end, replacement.chars());
        previous_start = start;
    }
    let new: String = out.into_iter().collect();
    if new == lines.join("\n") {
        return None;
    }
    Some(Fix::edit(
        first,
        last - first + 1,
        new.split('\n').map(str::to_string).collect(),
    ))
}

/// The rule a clippy code belongs to. `disallowed_types` serves two rules,
/// told apart by the type named in the message.
fn rule_for(code: &str, text: &str, rules: &[&'static Rule]) -> Option<&'static Rule> {
    let owners: Vec<&'static Rule> = rules
        .iter()
        .copied()
        .filter(|r| r.clippy.contains(&code))
        .collect();
    match owners.as_slice() {
        [] => None,
        [one] => Some(*one),
        many => {
            let wanted = if text.contains("DefaultHasher") {
                "persisted-values-stable"
            } else {
                "parking-lot-locks"
            };
            many.iter()
                .copied()
                .find(|r| r.id == wanted)
                .or(Some(many[0]))
        }
    }
}

#[cfg(test)]
mod suggestion_tests {
    use super::suggestion_fix;

    #[test]
    fn applies_suggested_replacements() {
        let text = "fn f() {\n    let s = format!(\"{}\", x);\n}\n";
        let msg: serde_json::Value = serde_json::from_str(
            r#"{"spans":[],"children":[{"spans":[{"line_start":2,"column_start":21,"line_end":2,"column_end":28,"suggested_replacement":"\"{x}\""}]}]}"#,
        )
        .unwrap();
        let fix = suggestion_fix(&msg, text).unwrap();
        assert_eq!(fix.edits[0].line, 2);
        assert_eq!(
            fix.edits[0].insert,
            vec!["    let s = format!(\"{x}\");".to_string()]
        );
    }
}
