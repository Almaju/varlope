//! Shell scripts use `set -euo pipefail` and stay portable (shellcheck alone misses both)
//!
//! Docs and examples: `rules/shell-strict-shellcheck/`.

use std::collections::BTreeSet;

use crate::helpers::docs_deps::{hash_comment_start, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "shell-strict-shellcheck",
    area: Area::Scripts,
    title: "Shell scripts use `set -euo pipefail` and stay portable (shellcheck alone misses both)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Commands that rarely fail, or whose failure the next command reports anyway.
const QUIET: &[&str] = &[
    "echo", "printf", "cd", "pushd", "popd", "rm", "mkdir", "shift", "set", "trap", "source", ".",
    "export", "local", "declare", "readonly", "true", ":", "exit", "return", "unset", "read",
    "sleep", "wait", "exec", "eval", "break", "continue", "fail", "die", "abort", "fatal", "error",
    "usage", "bail",
];

/// Shell keywords and syntax that are not commands themselves.
const SYNTAX: &[&str] = &[
    "then", "else", "elif", "fi", "do", "done", "esac", "case", "in", ";;", "{", "}", "(", ")",
    "function", "[[", "[", "test", "!",
];

fn check(cx: &Context, out: &mut Findings) {
    let sourced = sourced_names(cx);
    for file in cx.ws.files_with_ext(&["sh"]) {
        if is_out_of_scope(&file.path) || file.path.split('/').any(|p| p == "examples") {
            continue;
        }
        if is_executed(file, &sourced) && !sets_errexit(&file.text) {
            if let Some((line, command)) = first_unchecked(&file.text) {
                out.add(
                    &file.path,
                    line,
                    format!("no `set -e`: if `{command}` fails the script carries on; add `set -euo pipefail` after the shebang"),
                );
            }
        }
        sed_in_place(file, out);
    }
    exec_bits(cx, out);
}

/// File names that some script sources (`source x.sh`, `. ./x.sh`).
fn sourced_names(cx: &Context) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for file in cx.ws.files_with_ext(&["sh"]) {
        for (_, line) in file.lines() {
            let line = line.trim_start();
            let rest = line
                .strip_prefix("source ")
                .or_else(|| line.strip_prefix(". "));
            if let Some(rest) = rest {
                let target = rest.split_whitespace().next().unwrap_or("");
                let name = target.trim_matches('"').rsplit('/').next().unwrap_or("");
                names.insert(name.trim_matches('"').to_string());
            }
        }
    }
    names
}

/// A script that runs on its own: it has a shebang, and is neither sourced
/// by another script nor marked as meant to be sourced.
fn is_executed(file: &TextFile, sourced: &BTreeSet<String>) -> bool {
    let head: String = file
        .lines()
        .take(15)
        .map(|(_, l)| l.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    file.text.starts_with("#!")
        && !sourced.contains(file.file_name())
        && !head.contains("shellcheck shell=")
        && !head.contains("sourced")
        && !head.contains("not be run directly")
}

/// `set -e` (in any spelling) outside heredocs, or `-e` on the shebang line.
fn sets_errexit(text: &str) -> bool {
    let shebang = text
        .lines()
        .next()
        .is_some_and(|l| l.starts_with("#!") && l.split_whitespace().any(|w| w == "-e"));
    shebang
        || commands(text).iter().any(|(_, command)| {
            command.strip_prefix("set ").is_some_and(|rest| {
                rest.split_whitespace().any(|w| {
                    (w.starts_with('-') && !w.starts_with("--") && w.contains('e'))
                        || w == "errexit"
                })
            })
        })
}

/// The first command whose failure goes unnoticed: not a condition, not
/// followed by `||`, not checked through `$?`, and not the last command
/// (whose status is the script's).
fn first_unchecked(text: &str) -> Option<(usize, String)> {
    let commands = commands(text);
    let mut candidate = None;
    for (i, (line, logical)) in commands.iter().enumerate() {
        let next_checks_status = commands
            .get(i + 1)
            .is_some_and(|(_, next)| next.contains("$?"));
        let words: Vec<&str> = logical.split_whitespace().collect();
        let Some(first) = words.first().copied() else {
            continue;
        };
        let is_condition = matches!(first, "if" | "elif" | "while" | "until")
            || logical.contains("||")
            || logical.ends_with("&&")
            || next_checks_status;
        let is_assignment = first.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
        let is_function = logical.contains("()") || first == "function";
        if is_condition
            || is_assignment
            || is_function
            || QUIET.contains(&first)
            || SYNTAX.contains(&first)
        {
            continue;
        }
        if candidate.is_some() {
            // There is a command after the unchecked one.
            return candidate;
        }
        candidate = Some((
            *line,
            words.iter().take(3).copied().collect::<Vec<_>>().join(" "),
        ));
    }
    None
}

/// Logical command lines `(first line, text)`: comments stripped, `\`
/// continuations joined, heredoc bodies skipped.
fn commands(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut heredoc: Option<String> = None;
    let mut pending: Option<(usize, String)> = None;
    for (index, raw) in text.lines().enumerate() {
        if let Some(end) = &heredoc {
            if raw.trim() == end {
                heredoc = None;
            }
            continue;
        }
        let code = match hash_comment_start(raw) {
            Some(at) => &raw[..at],
            None => raw,
        };
        if let Some(at) = code.find("<<") {
            let tag = code[at + 2..]
                .trim_start_matches(['-', '~'])
                .trim()
                .trim_matches(['\'', '"'])
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !tag.is_empty() && !tag.starts_with('<') {
                heredoc = Some(tag);
            }
        }
        let (start, mut joined) = pending.take().unwrap_or((index + 1, String::new()));
        let trimmed = code.trim();
        if let Some(cont) = trimmed.strip_suffix('\\') {
            joined.push_str(cont);
            joined.push(' ');
            pending = Some((start, joined));
            continue;
        }
        joined.push_str(trimmed);
        if !joined.trim().is_empty() {
            out.push((start, joined.trim().to_string()));
        }
    }
    out
}

/// `sed -i` differs between GNU (no argument) and BSD (`-i ''`) sed.
fn sed_in_place(file: &TextFile, out: &mut Findings) {
    let p = file.path.to_lowercase();
    let mac_only = ["ios", "mac", "osx", "darwin", "xcode"]
        .iter()
        .any(|k| p.contains(k));
    let linux_only = ["linux", "docker"].iter().any(|k| p.contains(k));
    for (line, raw) in file.lines() {
        let code = match hash_comment_start(raw) {
            Some(at) => &raw[..at],
            None => raw,
        };
        let words: Vec<&str> = code.split_whitespace().collect();
        let Some(sed) = words.iter().position(|w| *w == "sed") else {
            continue;
        };
        let Some(flag) = words[sed + 1..]
            .iter()
            .position(|w| *w == "-i" || (w.starts_with("-i") && !w.starts_with("-i.")))
        else {
            continue;
        };
        let bsd = words
            .get(sed + 1 + flag + 1)
            .is_some_and(|w| *w == "''" || *w == "\"\"");
        let (flavour, other) = if bsd { ("BSD", "GNU") } else { ("GNU", "BSD") };
        if (bsd && mac_only) || (!bsd && linux_only) {
            continue;
        }
        out.add(
            &file.path,
            line,
            format!("this `sed -i` form only works with {flavour} sed and fails with {other} sed: write to a temp file and `mv` it, or use `sed -i.bak` and remove the backup"),
        );
    }
}

/// Executable files must be scripts: mozilla-central's `file-perms` lint
/// rejects an exec bit on anything without a shebang.
fn exec_bits(cx: &Context, out: &mut Findings) {
    #[cfg(unix)]
    for file in cx.ws.files() {
        use std::os::unix::fs::PermissionsExt;
        if is_out_of_scope(&file.path) || file.text.starts_with("#!") {
            continue;
        }
        let Ok(meta) = std::fs::metadata(cx.ws.root.join(&file.path)) else {
            continue;
        };
        if meta.permissions().mode() & 0o111 != 0 {
            out.add(
                &file.path,
                1,
                "executable but has no `#!` shebang: drop the exec bit (`chmod -x`) or add a shebang",
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (cx, out);
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "automation/stamp-changelog.sh";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/shell-strict-shellcheck/dont.sh"),
            )],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("no `set -e`"));
        assert_eq!((found[0].line, found[1].line), (4, 4));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/shell-strict-shellcheck/do.sh"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn checked_commands_and_sourced_files_are_fine() {
        let checked = "#!/bin/bash\nif ! cargo build; then\n  exit 1\nfi\nmkdir -p out\ncargo test || fail \"tests\"\nfind . -name x | xargs rm\n";
        let sourced = "#!/bin/bash\nexport A=1\ncargo build\ncargo test\n";
        let user = "#!/bin/bash\nsource ./libs/common.sh\nrun\n";
        let directive = "#!/bin/bash\n# shellcheck shell=bash\ncargo build\ncargo test\n";
        let no_shebang = "cargo build\ncargo test\n";
        let found = check(
            &RULE,
            &[
                ("a/checked.sh", checked),
                ("libs/common.sh", sourced),
                ("a/user.sh", user),
                ("a/directive.sh", directive),
                ("a/no_shebang.sh", no_shebang),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn errexit_spellings() {
        for header in ["set -e", "set -euo pipefail", "set -o errexit", "set -ex"] {
            let text = format!("#!/bin/bash\n{header}\ncargo build\ncargo test\n");
            assert!(check(&RULE, &[("a/x.sh", &text)]).is_empty(), "{header}");
        }
        let text = "#!/bin/bash\ncat <<EOF\nset -e\nEOF\ncargo build\ncargo test\n";
        assert_eq!(check(&RULE, &[("a/x.sh", text)]).len(), 1);
    }

    #[test]
    fn platform_specific_sed() {
        let bsd = "#!/bin/bash\nset -e\nsed -i '' 's/a/b/' x\n";
        let gnu = "#!/bin/bash\nset -e\nsed -i 's/a/b/' x\n";
        let backup = "#!/bin/bash\nset -e\nsed -i.bak 's/a/b/' x\n";
        let found = check(
            &RULE,
            &[
                ("libs/build-nss-ios.sh", bsd),
                ("taskcluster/docker/linux/setup.sh", gnu),
                ("tools/a.sh", backup),
                ("tools/b.sh", gnu),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, "tools/b.sh");
    }
}
