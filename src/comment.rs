// Adapted from rabot (https://github.com/Almaju/rabot, MIT).

/// What kind of comment the scanner found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
    Block,
    Doc,
    Line,
}

/// One comment in a source text, with its inner text and location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Byte offset one past the last character of the comment.
    pub end: usize,
    pub kind: CommentKind,
    /// 1-based line on which the comment starts.
    pub line: usize,
    /// Byte offset of the first `/`.
    pub start: usize,
    /// Text without the `//`, `///`, `/*` or `*/` delimiters.
    pub text: String,
}

impl Comment {
    /// Whether this is a later line of a `//` comment that started on the
    /// line above, given the comment just before it.
    pub fn continues(&self, previous: Option<&Comment>) -> bool {
        self.is_line()
            && previous.is_some_and(|previous| previous.is_line() && previous.line + 1 == self.line)
    }

    pub fn is_doc(&self) -> bool {
        self.kind == CommentKind::Doc
    }

    pub fn is_line(&self) -> bool {
        self.kind == CommentKind::Line
    }
}

/// All comments of a file, in source order.
///
/// `syn` throws comments away, so varlope scans them itself. The scanner skips
/// string, raw string, byte string and char literals so that a `//` inside a
/// URL literal is not mistaken for a comment.
#[derive(Clone, Debug, Default)]
pub struct Comments {
    items: Vec<Comment>,
}

impl Comments {
    pub fn scan(text: &str) -> Self {
        Scanner::new(text).run()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Comment> {
        self.items.iter()
    }

    pub fn on_line(&self, line: usize) -> impl Iterator<Item = &Comment> {
        self.items
            .iter()
            .filter(move |comment| comment.line == line)
    }
}

struct Scanner<'a> {
    bytes: &'a [u8],
    items: Vec<Comment>,
    line: usize,
    pos: usize,
    text: &'a str,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            items: Vec::new(),
            line: 1,
            pos: 0,
            text,
        }
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        let line = self.line;
        let kind = match (self.peek(2), self.peek(3)) {
            (Some(b'*'), Some(b'/')) => CommentKind::Block,
            (Some(b'*'), _) | (Some(b'!'), _) => CommentKind::Doc,
            _ => CommentKind::Block,
        };
        self.pos += 2;
        let mut depth = 1;
        while self.pos < self.bytes.len() && depth > 0 {
            match self.bytes[self.pos] {
                b'/' if self.peek(1) == Some(b'*') => {
                    depth += 1;
                    self.pos += 2;
                }
                b'*' if self.peek(1) == Some(b'/') => {
                    depth -= 1;
                    self.pos += 2;
                }
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        let end = self.pos;
        let inner_start = start + if kind == CommentKind::Doc { 3 } else { 2 };
        let inner_end = end.saturating_sub(2).max(inner_start);
        self.items.push(Comment {
            end,
            kind,
            line,
            start,
            text: self.text[inner_start..inner_end].to_string(),
        });
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        let kind = match (self.peek(2), self.peek(3)) {
            (Some(b'/'), Some(b'/')) => CommentKind::Line,
            (Some(b'/'), _) | (Some(b'!'), _) => CommentKind::Doc,
            _ => CommentKind::Line,
        };
        let end = self.text[start..]
            .find('\n')
            .map_or(self.bytes.len(), |offset| start + offset);
        let inner_start = start + if kind == CommentKind::Doc { 3 } else { 2 };
        self.items.push(Comment {
            end,
            kind,
            line: self.line,
            start,
            text: self.text[inner_start..end].to_string(),
        });
        self.pos = end;
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    /// A `'` starts either a char literal or a lifetime.
    fn quote(&mut self) {
        let rest = &self.text[self.pos + 1..];
        let mut chars = rest.chars();
        match chars.next() {
            Some('\\') => {
                let close = rest[1..].find('\'').map_or(rest.len(), |offset| offset + 2);
                self.pos += 1 + close;
            }
            Some(first) if chars.next() == Some('\'') => {
                self.pos += 1 + first.len_utf8() + 1;
            }
            _ => self.pos += 1,
        }
    }

    fn raw_string(&mut self) {
        let start = self.pos;
        let quote = self.text[start..]
            .find('"')
            .map_or(self.bytes.len(), |offset| start + offset);
        let hashes = self.text[start..quote]
            .chars()
            .filter(|c| *c == '#')
            .count();
        let closer = format!("\"{}", "#".repeat(hashes));
        let body_start = quote + 1;
        let end = self.text[body_start..]
            .find(&closer)
            .map_or(self.bytes.len(), |offset| {
                body_start + offset + closer.len()
            });
        self.line += self.text[start..end].matches('\n').count();
        self.pos = end;
    }

    fn raw_string_ahead(&self) -> bool {
        let mut i = self.pos;
        if matches!(self.bytes[i], b'b' | b'c') {
            i += 1;
        }
        if self.bytes.get(i) != Some(&b'r') {
            return false;
        }
        i += 1;
        while self.bytes.get(i) == Some(&b'#') {
            i += 1;
        }
        self.bytes.get(i) == Some(&b'"')
    }

    fn run(mut self) -> Comments {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'/' if self.peek(1) == Some(b'/') => self.line_comment(),
                b'/' if self.peek(1) == Some(b'*') => self.block_comment(),
                b'"' => self.string(),
                b'\'' => self.quote(),
                b'r' | b'b' | b'c' if self.raw_string_ahead() => self.raw_string(),
                b'b' | b'c' if self.peek(1) == Some(b'"') => {
                    self.pos += 1;
                    self.string();
                }
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        Comments { items: self.items }
    }

    fn string(&mut self) {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'\\' => self.pos += 2,
                b'"' => {
                    self.pos += 1;
                    return;
                }
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<String> {
        Comments::scan(source)
            .iter()
            .map(|c| c.text.clone())
            .collect()
    }

    #[test]
    fn finds_line_and_block_comments() {
        let source = "fn a() {} // one\n/* two */ fn b() {}\n/// doc\nfn c() {}";
        assert_eq!(texts(source), vec![" one", " two ", " doc"]);
        let kinds: Vec<_> = Comments::scan(source).iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![CommentKind::Line, CommentKind::Block, CommentKind::Doc]
        );
    }

    #[test]
    fn ignores_slashes_inside_literals() {
        let source =
            r##"let a = "http://x"; let b = r#"// not"#; let c = '"'; let d = '/'; // yes"##;
        assert_eq!(texts(source), vec![" yes"]);
    }

    #[test]
    fn lifetimes_are_not_char_literals() {
        let source = "fn f<'a>(x: &'a str) -> &'a str { x } // c";
        assert_eq!(texts(source), vec![" c"]);
    }

    #[test]
    fn tracks_lines() {
        let source = "\n\n// third\nlet s = \"\n\"; // fifth";
        let lines: Vec<_> = Comments::scan(source).iter().map(|c| c.line).collect();
        assert_eq!(lines, vec![3, 5]);
    }

    #[test]
    fn nested_block_comments() {
        let source = "/* a /* b */ c */ // d";
        assert_eq!(texts(source), vec![" a /* b */ c ", " d"]);
    }
}
