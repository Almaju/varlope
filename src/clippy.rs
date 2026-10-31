//! Rules that clippy already implements: run `cargo clippy` with their lints
//! and report its findings under varlope's rule ids.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context as _, Result};

use crate::rule::{Finding, Origin, Rule};

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
pub fn run(root: &Path, rules: &[&'static Rule]) -> Result<Vec<Finding>> {
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
    Ok(parse(&stdout, rules))
}

/// Map clippy's JSON messages to findings.
pub fn parse(json_lines: &str, rules: &[&'static Rule]) -> Vec<Finding> {
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
        });
    }
    out
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
