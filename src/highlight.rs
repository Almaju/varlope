//! A small syntax highlighter for the docs site: comments, strings, numbers,
//! keywords, attributes, macros and type names, as HTML `<span>`s. No
//! dependencies; good enough for short examples.

use std::fmt::Write as _;

use crate::site::esc;

struct Lang {
    keywords: &'static [&'static str],
    /// Line comment openers.
    line_comments: &'static [&'static str],
    block_comments: bool,
    /// `#[...]` attributes (Rust) or `@Annotation` (Kotlin/Swift).
    attributes: bool,
    /// `name!` macros.
    macros: bool,
    /// Single quotes delimit strings (not lifetimes/chars).
    single_quote_strings: bool,
    /// A `"` string can run over several lines.
    multiline_strings: bool,
}

/// What a line leaves open for the next one: a `/* */` comment or a string.
#[derive(Clone, Copy, Debug, Default)]
pub struct Carry {
    block: bool,
    string: bool,
}

const RUST: Lang = Lang {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while", "Some", "None", "Ok", "Err",
    ],
    line_comments: &["//"],
    block_comments: true,
    attributes: true,
    macros: true,
    single_quote_strings: false,
    multiline_strings: true,
};

const UDL: Lang = Lang {
    keywords: &[
        "namespace",
        "dictionary",
        "interface",
        "enum",
        "callback",
        "constructor",
        "void",
        "string",
        "boolean",
        "sequence",
        "record",
        "typedef",
        "i32",
        "i64",
        "u32",
        "u64",
        "f64",
        "bytes",
        "null",
        "true",
        "false",
    ],
    line_comments: &["//"],
    block_comments: true,
    attributes: false,
    macros: false,
    single_quote_strings: false,
    multiline_strings: false,
};

const KOTLIN: Lang = Lang {
    keywords: &[
        "fun",
        "val",
        "var",
        "class",
        "object",
        "interface",
        "if",
        "else",
        "when",
        "return",
        "null",
        "true",
        "false",
        "import",
        "package",
        "private",
        "public",
        "internal",
        "override",
        "suspend",
        "is",
        "as",
        "in",
        "for",
        "while",
        "try",
        "catch",
        "throw",
        "data",
        "enum",
        "companion",
        "lateinit",
        "func",
        "let",
        "struct",
        "guard",
        "self",
        "nil",
        "init",
        "static",
        "extension",
        "protocol",
        "fileprivate",
        "open",
        "final",
        "throws",
        "rethrows",
        "do",
        "case",
        "switch",
        "default",
        "where",
        "async",
        "await",
    ],
    line_comments: &["//"],
    block_comments: true,
    attributes: true,
    macros: false,
    single_quote_strings: false,
    multiline_strings: true,
};

const SQL: Lang = Lang {
    keywords: &[
        "SELECT",
        "FROM",
        "WHERE",
        "AND",
        "OR",
        "NOT",
        "NULL",
        "INSERT",
        "INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE",
        "CREATE",
        "TABLE",
        "INDEX",
        "IF",
        "EXISTS",
        "PRIMARY",
        "KEY",
        "REFERENCES",
        "ON",
        "CASCADE",
        "INTEGER",
        "TEXT",
        "REAL",
        "BLOB",
        "JOIN",
        "LEFT",
        "ORDER",
        "BY",
        "GROUP",
        "LIMIT",
        "AS",
        "IN",
        "LIKE",
        "BETWEEN",
        "REPLACE",
        "CONFLICT",
        "DO",
        "NOTHING",
        "ALTER",
        "ADD",
        "COLUMN",
        "DROP",
        "PRAGMA",
        "DEFAULT",
        "UNIQUE",
        "WITHOUT",
        "ROWID",
        "COUNT",
        "FOREIGN",
    ],
    line_comments: &["--"],
    block_comments: true,
    attributes: false,
    macros: false,
    single_quote_strings: true,
    multiline_strings: false,
};

const SHELL: Lang = Lang {
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
        "function", "return", "set", "export", "local", "echo", "exit", "def", "import", "from",
        "class", "return", "None", "True", "False", "with", "as", "not", "and", "or", "try",
        "except", "raise",
    ],
    line_comments: &["#"],
    block_comments: false,
    attributes: false,
    macros: false,
    single_quote_strings: true,
    multiline_strings: true,
};

const CONFIG: Lang = Lang {
    keywords: &["true", "false", "null"],
    line_comments: &["#"],
    block_comments: false,
    attributes: false,
    macros: false,
    single_quote_strings: true,
    multiline_strings: false,
};

fn lang(ext: &str) -> Option<&'static Lang> {
    match ext {
        "rs" => Some(&RUST),
        "udl" => Some(&UDL),
        "kt" | "kts" | "swift" | "gradle" => Some(&KOTLIN),
        "sql" => Some(&SQL),
        "sh" | "py" => Some(&SHELL),
        "toml" | "yaml" | "yml" => Some(&CONFIG),
        _ => None,
    }
}

/// The extension a path's contents should be highlighted as.
pub fn ext_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

/// Highlight one line of `ext` source. `carry` holds what the previous line
/// left open.
pub fn line(ext: &str, text: &str, carry: &mut Carry) -> String {
    let Some(lang) = lang(ext) else {
        return esc(text);
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let rest = |i: usize| chars[i..].iter().collect::<String>();
    if carry.string && !lang.multiline_strings {
        carry.string = false;
    }
    while i < chars.len() {
        if carry.string {
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1;
                carry.string = false;
            }
            span(&mut out, "st", &chars[start..i.min(chars.len())]);
            continue;
        }
        if carry.block {
            let start = i;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            if i < chars.len() {
                i += 2;
                carry.block = false;
            }
            span(&mut out, "cm", &chars[start..i.min(chars.len())]);
            continue;
        }
        let c = chars[i];
        if lang.line_comments.iter().any(|o| rest(i).starts_with(o))
            && (c != '#' || i == 0 || chars[i - 1].is_whitespace())
        {
            span(&mut out, "cm", &chars[i..]);
            break;
        }
        if lang.block_comments && c == '/' && chars.get(i + 1) == Some(&'*') {
            carry.block = true;
            continue;
        }
        if lang.attributes && c == '#' && matches!(chars.get(i + 1), Some('[') | Some('!')) {
            let start = i;
            let mut depth = 0;
            while i < chars.len() {
                match chars[i] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            span(&mut out, "at", &chars[start..i]);
            continue;
        }
        if lang.attributes && c == '@' && chars.get(i + 1).is_some_and(|n| n.is_alphabetic()) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            span(&mut out, "at", &chars[start..i]);
            continue;
        }
        if c == '"' || (c == '\'' && lang.single_quote_strings) {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= chars.len() && c == '"' && lang.multiline_strings {
                carry.string = true;
            }
            i = (i + 1).min(chars.len());
            span(&mut out, "st", &chars[start..i]);
            continue;
        }
        if c == '\'' && !lang.single_quote_strings {
            // A char literal ('x', '\n', '"'), not a lifetime.
            let end = if chars.get(i + 1) == Some(&'\\') {
                chars[i + 2..]
                    .iter()
                    .take(8)
                    .position(|&x| x == '\'')
                    .map(|p| i + 3 + p)
            } else if chars.get(i + 2) == Some(&'\'') {
                Some(i + 3)
            } else {
                None
            };
            if let Some(end) = end {
                span(&mut out, "st", &chars[i..end]);
                i = end;
                continue;
            }
        }
        if c == 'r' && chars.get(i + 1) == Some(&'#') && chars.get(i + 2) == Some(&'"') {
            let start = i;
            i += 3;
            while i < chars.len() && !(chars[i] == '"' && chars.get(i + 1) == Some(&'#')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            span(&mut out, "st", &chars[start..i]);
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(chars[i - 1])) {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            span(&mut out, "nu", &chars[start..i]);
            continue;
        }
        if is_ident(c) && !c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let class = if lang.macros
                && chars.get(i) == Some(&'!')
                && chars.get(i + 1) != Some(&'=')
            {
                i += 1;
                Some("ma")
            } else if lang.keywords.contains(&word.as_str())
                || (lang.keywords == SQL.keywords
                    && lang.keywords.contains(&word.to_uppercase().as_str()))
            {
                Some("kw")
            } else if word.starts_with(char::is_uppercase) && word.chars().any(char::is_lowercase) {
                Some("ty")
            } else {
                None
            };
            match class {
                Some(class) => span(&mut out, class, &chars[start..i]),
                None => out.push_str(&esc(&word)),
            }
            continue;
        }
        out.push_str(&esc(&c.to_string()));
        i += 1;
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn span(out: &mut String, class: &str, chars: &[char]) {
    let text: String = chars.iter().collect();
    let _ = write!(out, r#"<span class="{class}">{}</span>"#, esc(&text));
}

/// The language a `.diff` example's file is in, from its `+++ b/path` lines.
pub fn diff_file_ext(header: &str) -> String {
    header
        .trim_start_matches("+++ ")
        .trim_start_matches("--- ")
        .trim_start_matches("b/")
        .trim_start_matches("a/")
        .rsplit_once('.')
        .map(|(_, e)| e.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tokens() {
        let mut b = Carry::default();
        let html = line(
            "rs",
            r#"#[uniffi::export] pub fn get(s: &str) -> Option<String> { log::info!("x {}", 1); } // why"#,
            &mut b,
        );
        assert!(html.contains(r#"<span class="at">#[uniffi::export]</span>"#));
        assert!(html.contains(r#"<span class="kw">pub</span>"#));
        assert!(html.contains(r#"<span class="ty">Option</span>"#));
        assert!(html.contains(r#"<span class="ma">info!</span>"#));
        assert!(html.contains(r#"<span class="st">&quot;x {}&quot;</span>"#));
        assert!(html.contains(r#"<span class="cm">// why</span>"#));
    }

    #[test]
    fn block_comments_span_lines() {
        let mut b = Carry::default();
        line("rs", "let a = 1; /* start", &mut b);
        assert!(b.block);
        let html = line("rs", "still */ let b = 2;", &mut b);
        assert!(!b.block);
        assert!(html.starts_with(r#"<span class="cm">still */</span>"#));
    }

    #[test]
    fn strings_span_lines_and_chars_do_not() {
        let mut b = Carry::default();
        line("rs", r#"query("SELECT 1"#, &mut b);
        assert!(b.string);
        let html = line("rs", r#"  FROM t", x); // c"#, &mut b);
        assert!(!b.string);
        assert!(html.contains(r#"<span class="cm">// c</span>"#));
        let html = line("rs", r#"if c == '"' { f::<'a>() } // c"#, &mut b);
        assert!(!b.string);
        assert!(html.contains(r#"<span class="st">'&quot;'</span>"#));
        assert!(html.contains(r#"<span class="cm">// c</span>"#));
    }

    #[test]
    fn sql_and_hash_comments() {
        let mut b = Carry::default();
        assert!(line("sql", "select * from t -- c", &mut b)
            .contains(r#"<span class="kw">select</span>"#));
        assert!(line("toml", "a = \"b\" # c", &mut b).contains(r#"<span class="cm"># c</span>"#));
    }
}
