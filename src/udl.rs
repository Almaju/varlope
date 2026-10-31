//! Just enough of a UniFFI UDL parser for linting: declarations, their
//! members, attributes, defaults and doc comments, with line numbers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdlKind {
    Namespace,
    Dictionary,
    Enum,
    Interface,
    CallbackInterface,
    Typedef,
    Other,
}

#[derive(Clone, Debug)]
pub struct UdlItem {
    pub kind: UdlKind,
    pub name: String,
    /// Raw attribute list, e.g. `Error` or `Trait, WithForeign`.
    pub attrs: String,
    pub line: usize,
    pub has_doc: bool,
    pub members: Vec<UdlMember>,
}

impl UdlItem {
    pub fn has_attr(&self, name: &str) -> bool {
        attr_names(&self.attrs).any(|a| a == name)
    }
}

#[derive(Clone, Debug)]
pub struct UdlMember {
    /// Field, method, function or variant name (`constructor` for constructors).
    pub name: String,
    /// The member text without attributes, whitespace-normalized:
    /// `string? title = null`, `void delete(string id)`, `"Amp"`...
    pub signature: String,
    pub attrs: String,
    pub line: usize,
    pub has_doc: bool,
}

impl UdlMember {
    pub fn has_attr(&self, name: &str) -> bool {
        attr_names(&self.attrs).any(|a| a == name || a.starts_with(&format!("{name}=")))
    }

    /// Whether this is a dictionary field with a default (`= ...`).
    pub fn has_default(&self) -> bool {
        self.signature.contains('=') && !self.signature.contains('(')
    }

    /// For a field `T name` or `T name = d`, the type `T`.
    pub fn field_type(&self) -> Option<&str> {
        let decl = self.signature.split('=').next()?.trim();
        decl.rsplit_once(' ').map(|(ty, _)| ty.trim())
    }
}

fn attr_names(attrs: &str) -> impl Iterator<Item = &str> {
    attrs
        .split(',')
        .map(|a| a.trim().split('(').next().unwrap_or("").trim())
        .filter(|a| !a.is_empty())
}

/// Parse a UDL file. Unknown syntax is skipped, never an error.
pub fn parse(text: &str) -> Vec<UdlItem> {
    let doc_lines: std::collections::HashSet<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with("///"))
        .map(|(i, _)| i + 1)
        .collect();
    let lines: Vec<&str> = text.lines().collect();
    let has_doc = |line: usize| -> bool {
        // Walk up past attribute-only and blank lines to the previous line.
        let mut l = line;
        while l > 1 {
            l -= 1;
            let t = lines[l - 1].trim();
            if t.starts_with('[') && t.ends_with(']') {
                continue;
            }
            return doc_lines.contains(&l);
        }
        false
    };

    let code = strip_comments(text);
    let chars: Vec<(usize, char)> = code.chars().map(|c| (0, c)).collect();
    let mut line_of = Vec::with_capacity(chars.len());
    let mut line = 1;
    for (_, c) in &chars {
        line_of.push(line);
        if *c == '\n' {
            line += 1;
        }
    }
    let code_chars: Vec<char> = code.chars().collect();

    let mut items = Vec::new();
    for (start, end) in split_top(&code_chars, 0, code_chars.len()) {
        let stmt: String = code_chars[start..end].iter().collect();
        let Some(first) = first_non_space(&code_chars, start, end) else {
            continue;
        };
        let stmt_line = line_of[first];
        let (attrs, rest) = take_attrs(stmt.trim());
        let rest = rest.trim();
        let (kind, after) = if let Some(r) = rest.strip_prefix("namespace ") {
            (UdlKind::Namespace, r)
        } else if let Some(r) = rest.strip_prefix("dictionary ") {
            (UdlKind::Dictionary, r)
        } else if let Some(r) = rest.strip_prefix("enum ") {
            (UdlKind::Enum, r)
        } else if let Some(r) = rest.strip_prefix("callback interface ") {
            (UdlKind::CallbackInterface, r)
        } else if let Some(r) = rest.strip_prefix("interface ") {
            (UdlKind::Interface, r)
        } else if let Some(r) = rest.strip_prefix("typedef ") {
            (UdlKind::Typedef, r)
        } else {
            (UdlKind::Other, rest)
        };
        let name = if kind == UdlKind::Typedef {
            after.split_whitespace().last().unwrap_or("").to_string()
        } else {
            after
                .split(|c: char| c.is_whitespace() || c == '{')
                .next()
                .unwrap_or("")
                .to_string()
        };
        let mut members = Vec::new();
        if let Some(open_rel) = stmt.find('{') {
            let open = start + stmt[..open_rel].chars().count();
            if let Some(close) = matching(&code_chars, open) {
                for (ms, me) in split_members(&code_chars, open + 1, close, kind) {
                    let text: String = code_chars[ms..me].iter().collect();
                    let Some(mfirst) = first_non_space(&code_chars, ms, me) else {
                        continue;
                    };
                    let mline = line_of[mfirst];
                    let (mattrs, mrest) = take_attrs(text.trim());
                    let sig = normalize(mrest);
                    if sig.is_empty() {
                        continue;
                    }
                    members.push(UdlMember {
                        name: member_name(&sig),
                        signature: sig,
                        attrs: mattrs,
                        line: mline,
                        has_doc: has_doc(mline),
                    });
                }
            }
        }
        items.push(UdlItem {
            kind,
            name,
            attrs,
            line: stmt_line,
            has_doc: has_doc(stmt_line),
            members,
        });
    }
    items
}

/// Replace comments with spaces (keeping newlines so lines still line up).
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Split `[start, end)` on `;` at nesting depth 0.
fn split_top(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut s = start;
    for (i, c) in chars.iter().enumerate().take(end).skip(start) {
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' => depth -= 1,
            ';' if depth == 0 => {
                out.push((s, i));
                s = i + 1;
            }
            _ => {}
        }
    }
    if chars[s..end].iter().any(|c| !c.is_whitespace()) {
        out.push((s, end));
    }
    out
}

/// Members of a block: `;`-separated, except flat enums which use `,`.
fn split_members(chars: &[char], start: usize, end: usize, kind: UdlKind) -> Vec<(usize, usize)> {
    let body: String = chars[start..end].iter().collect();
    let flat_enum = kind == UdlKind::Enum && !body.contains('(') && !body.contains(';');
    if !flat_enum {
        return split_top(chars, start, end);
    }
    let mut out = Vec::new();
    let mut s = start;
    for (i, c) in chars.iter().enumerate().take(end).skip(start) {
        if *c == ',' {
            out.push((s, i));
            s = i + 1;
        }
    }
    if chars[s..end].iter().any(|c| !c.is_whitespace()) {
        out.push((s, end));
    }
    out
}

fn matching(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in chars.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_non_space(chars: &[char], start: usize, end: usize) -> Option<usize> {
    (start..end).find(|&i| !chars[i].is_whitespace())
}

/// Leading `[...]` attribute lists, joined; and the rest.
fn take_attrs(text: &str) -> (String, &str) {
    let mut rest = text.trim_start();
    let mut attrs = Vec::new();
    while let Some(after) = rest.strip_prefix('[') {
        let mut depth = 1;
        let mut end = None;
        for (i, c) in after.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        attrs.push(after[..end].trim().to_string());
        rest = after[end + 1..].trim_start();
    }
    (attrs.join(", "), rest)
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn member_name(sig: &str) -> String {
    if let Some(open) = sig.find('(') {
        let before = sig[..open].trim();
        return before.rsplit(' ').next().unwrap_or(before).to_string();
    }
    let decl = sig.split('=').next().unwrap_or(sig).trim();
    decl.rsplit(' ')
        .next()
        .unwrap_or(decl)
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const UDL: &str = r#"
namespace places {
    /// Open the db.
    [Throws=PlacesApiError]
    PlacesApi places_api_new(string db_path);
};

/// A visit.
dictionary VisitObservation {
    Url url;
    string? title = null;
    i64? at;
};

enum FrecencyThresholdOption { "None", "SkipOneTimePages" };

[Error]
interface PlacesApiError {
    UnexpectedPlacesException(string reason);
};

interface PlacesConnection {
    constructor(string path);
    // not a doc
    [Throws=PlacesApiError]
    void delete_everything();
};
"#;

    #[test]
    fn parses_declarations_and_members() {
        let items = parse(UDL);
        let names: Vec<_> = items.iter().map(|i| (i.kind, i.name.as_str())).collect();
        assert_eq!(
            names,
            vec![
                (UdlKind::Namespace, "places"),
                (UdlKind::Dictionary, "VisitObservation"),
                (UdlKind::Enum, "FrecencyThresholdOption"),
                (UdlKind::Interface, "PlacesApiError"),
                (UdlKind::Interface, "PlacesConnection"),
            ]
        );
        let ns = &items[0];
        assert_eq!(ns.members[0].name, "places_api_new");
        assert!(ns.members[0].has_doc);
        assert!(ns.members[0].has_attr("Throws"));
        let dict = &items[1];
        assert!(dict.has_doc);
        assert_eq!(dict.members[1].name, "title");
        assert!(dict.members[1].has_default());
        assert_eq!(dict.members[2].field_type(), Some("i64?"));
        assert!(!dict.members[2].has_default());
        assert_eq!(items[2].members.len(), 2);
        assert_eq!(items[2].members[1].name, "SkipOneTimePages");
        assert!(items[3].has_attr("Error"));
        let conn = &items[4];
        assert_eq!(conn.members[0].name, "constructor");
        assert_eq!(conn.members[1].name, "delete_everything");
        assert!(!conn.members[1].has_doc);
        assert_eq!(conn.members[1].line, 25);
    }
}
