//! Helpers shared by the docs_deps rules.

use std::collections::BTreeMap;

use crate::comment::Comments;
use crate::workspace::Workspace;

/// Generated, vendored or third-party files that no rule should report on.
pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("components/external/")
        || path.contains("/generated/")
        || path.contains("/Generated/")
        || path.starts_with("generated/")
        || path.contains("/third_party/")
        || path.contains("/vendor/")
        || matches!(
            name,
            "DEPENDENCIES.md" | "dependency-licenses.xml" | "Cargo.lock" | "Package.resolved"
        )
}

/// A file's text: from the workspace, or from disk under the workspace root
/// for files the workspace does not load (`Package.resolved`, `Dockerfile`).
pub fn text_of(ws: &Workspace, path: &str) -> Option<String> {
    if let Some(file) = ws.file(path) {
        return Some(file.text.clone());
    }
    std::fs::read_to_string(ws.root.join(path)).ok()
}

/// The new side of every file in a hand-written unified diff (context and
/// added lines, in order): lets a multi-file Don't/Do example double as a
/// workspace in tests.
pub fn files_from_diff(diff: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("--- ") && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ ")) {
            i += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.strip_prefix("b/").unwrap_or(path).to_string();
            files.push((path, String::new()));
        } else if let Some((_, text)) = files.last_mut() {
            if line.starts_with("@@") || line.starts_with('-') {
                // hunk headers and removed lines are not in the new file
            } else {
                let content = line
                    .strip_prefix('+')
                    .or_else(|| line.strip_prefix(' '))
                    .unwrap_or(line);
                text.push_str(content);
                text.push('\n');
            }
        }
        i += 1;
    }
    files
}

/// In-memory files: `(path, text)`.
pub type Files = Vec<(String, String)>;

/// The old side (context and removed lines) and the new side (context and
/// added lines) of every file in a hand-written diff that starts at line 1.
/// A file only on one side (`--- /dev/null`, `+++ /dev/null`) is left out of
/// the other.
pub fn old_and_new_from_diff(diff: &str) -> (Files, Files) {
    // (path, old text, new text, exists before, exists after)
    let mut files: Vec<(String, String, String, bool, bool)> = Vec::new();
    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let (Some(minus), Some(plus)) = (
            line.strip_prefix("--- "),
            lines.get(i + 1).and_then(|n| n.strip_prefix("+++ ")),
        ) {
            let before = !minus.starts_with("/dev/null");
            let after = !plus.starts_with("/dev/null");
            let path = if after { plus } else { minus };
            let path = path
                .strip_prefix("b/")
                .or_else(|| path.strip_prefix("a/"))
                .unwrap_or(path);
            files.push((
                path.to_string(),
                String::new(),
                String::new(),
                before,
                after,
            ));
            i += 2;
            continue;
        }
        if let Some((_, old, new, _, _)) = files.last_mut() {
            if line.starts_with("@@") {
                // hunk headers carry no content
            } else if let Some(added) = line.strip_prefix('+') {
                new.push_str(added);
                new.push('\n');
            } else if let Some(removed) = line.strip_prefix('-') {
                old.push_str(removed);
                old.push('\n');
            } else {
                let context = line.strip_prefix(' ').unwrap_or(line);
                for side in [old, new] {
                    side.push_str(context);
                    side.push('\n');
                }
            }
        }
        i += 1;
    }
    let old = files
        .iter()
        .filter(|f| f.3)
        .map(|f| (f.0.clone(), f.1.clone()))
        .collect();
    let new = files
        .iter()
        .filter(|f| f.4)
        .map(|f| (f.0.clone(), f.2.clone()))
        .collect();
    (old, new)
}

/// How a file type writes comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentStyle {
    /// `//` and `/* */`: Rust, Kotlin, Swift, UDL, Gradle, C.
    CLike,
    /// `#`: Python, shell, YAML, TOML.
    Hash,
    /// `<!-- -->`: Markdown, XML.
    Html,
    /// `--`: SQL.
    Sql,
}

pub fn comment_style(path: &str) -> Option<CommentStyle> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "rs" | "kt" | "kts" | "swift" | "udl" | "gradle" | "c" | "h" | "js" | "java" => {
            Some(CommentStyle::CLike)
        }
        "py" | "sh" | "yml" | "yaml" | "toml" => Some(CommentStyle::Hash),
        "md" | "xml" => Some(CommentStyle::Html),
        "sql" => Some(CommentStyle::Sql),
        _ if name == "Dockerfile" => Some(CommentStyle::Hash),
        _ => None,
    }
}

/// The comment text on each line of a file: `line -> text` (without the
/// comment markers). A block comment spanning lines yields one entry per line.
pub fn comment_lines(path: &str, text: &str) -> BTreeMap<usize, String> {
    let mut out = BTreeMap::new();
    let Some(style) = comment_style(path) else {
        return out;
    };
    match style {
        CommentStyle::CLike => {
            for c in Comments::scan(text).iter() {
                for (i, part) in c.text.split('\n').enumerate() {
                    let part = part.trim_start();
                    let part = part.strip_prefix('*').unwrap_or(part);
                    push(&mut out, c.line + i, part);
                }
            }
        }
        CommentStyle::Hash => {
            for (index, line) in text.lines().enumerate() {
                if let Some(at) = hash_comment_start(line) {
                    push(&mut out, index + 1, &line[at + 1..]);
                }
            }
        }
        CommentStyle::Sql => {
            for (index, line) in text.lines().enumerate() {
                if let Some(at) = line.find("--") {
                    push(&mut out, index + 1, &line[at + 2..]);
                }
            }
        }
        CommentStyle::Html => {
            let mut rest = text;
            let mut offset = 0;
            while let Some(open) = rest.find("<!--") {
                let body_start = open + 4;
                let close = rest[body_start..]
                    .find("-->")
                    .map(|c| body_start + c)
                    .unwrap_or(rest.len());
                let line = 1 + text[..offset + open].matches('\n').count();
                for (i, part) in rest[body_start..close].split('\n').enumerate() {
                    push(&mut out, line + i, part);
                }
                let next = (close + 3).min(rest.len());
                offset += next;
                rest = &rest[next..];
            }
        }
    }
    out
}

fn push(out: &mut BTreeMap<usize, String>, line: usize, text: &str) {
    let entry = out.entry(line).or_default();
    if !entry.is_empty() {
        entry.push(' ');
    }
    entry.push_str(text);
}

/// Where a `#` comment starts on a line of shell, Python, YAML or TOML: a `#`
/// at the start or after whitespace, outside quotes.
pub fn hash_comment_start(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if c == q && prev != '\\' {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '#' if prev.is_whitespace() || i == 0 => return Some(i),
                _ => {}
            },
        }
        prev = c;
    }
    None
}

/// The first 1-based line whose text matches `pred`.
pub fn find_line(text: &str, pred: impl Fn(&str) -> bool) -> Option<usize> {
    text.lines().position(pred).map(|i| i + 1)
}

/// Package names and versions in a `Cargo.lock`, with the line of each
/// `name = ` entry. Packages without a `source` (in-repo crates) are skipped
/// unless `with_local` is set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockPackage {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub dependencies: Vec<String>,
    pub line: usize,
}

pub fn lock_packages(text: &str) -> Vec<LockPackage> {
    let mut out: Vec<LockPackage> = Vec::new();
    let mut in_deps = false;
    for (index, line) in text.lines().enumerate() {
        let t = line.trim();
        if t == "[[package]]" {
            in_deps = false;
            out.push(LockPackage {
                name: String::new(),
                version: String::new(),
                source: None,
                dependencies: Vec::new(),
                line: index + 1,
            });
            continue;
        }
        let Some(p) = out.last_mut() else { continue };
        if in_deps {
            if t == "]" {
                in_deps = false;
            } else if let Some(dep) = t.trim_end_matches(',').strip_prefix('"') {
                let dep = dep.trim_end_matches('"');
                p.dependencies
                    .push(dep.split(' ').next().unwrap_or(dep).to_string());
            }
            continue;
        }
        if let Some(v) = toml_string(t, "name") {
            p.name = v;
            p.line = index + 1;
        } else if let Some(v) = toml_string(t, "version") {
            p.version = v;
        } else if let Some(v) = toml_string(t, "source") {
            p.source = Some(v);
        } else if t.starts_with("dependencies = [") {
            in_deps = !t.ends_with(']');
        }
    }
    out.retain(|p| !p.name.is_empty());
    out
}

/// `key = "value"` on one line.
pub fn toml_string(line: &str, key: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_from_hand_written_diffs() {
        let diff = "--- a/x.toml\n+++ b/x.toml\n a = 1\n-b = 2\n+b = 3\n+++ b/y.md\n+hello\n";
        let files = files_from_diff(diff);
        assert_eq!(
            files,
            vec![
                ("x.toml".to_string(), "a = 1\nb = 3\n".to_string()),
                ("y.md".to_string(), "hello\n".to_string()),
            ]
        );
    }

    #[test]
    fn comments_by_language() {
        let rs = comment_lines("a.rs", "let s = \"// no\"; // yes\n/* one\n * two */\n");
        assert_eq!(rs.get(&1).map(String::as_str), Some(" yes"));
        assert_eq!(rs.get(&3).map(|s| s.trim()), Some("two"));
        let sh = comment_lines("a.sh", "echo \"#no\" # yes\nx=${#y}\n# top\n");
        assert_eq!(sh.get(&1).map(String::as_str), Some(" yes"));
        assert!(!sh.contains_key(&2));
        assert!(sh.contains_key(&3));
        let md = comment_lines("a.md", "text\n<!-- a\nb -->\n");
        assert_eq!(md.get(&3).map(String::as_str), Some("b "));
    }

    #[test]
    fn lockfile_packages() {
        let lock = "[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\ndependencies = [\n \"b\",\n \"c 2.0.0\",\n]\n\n[[package]]\nname = \"b\"\nversion = \"0.1.0\"\n";
        let p = lock_packages(lock);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].dependencies, vec!["b", "c"]);
        assert_eq!(p[0].line, 2);
        assert_eq!(p[1].source, None);
    }
}
