//! Files as rules see them: text with line lookups, and for Rust, the syntax
//! tree, comments and test regions.

use std::ops::Range;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::allowance::{Allowances, Scope};
use crate::comment::Comments;
use crate::test_regions::TestRegions;

/// Any text file in the workspace.
#[derive(Debug)]
pub struct TextFile {
    /// Relative to the workspace root, `/`-separated.
    pub path: String,
    pub text: String,
    line_starts: Vec<usize>,
    pub allowances: Allowances,
}

impl TextFile {
    pub fn new(path: impl Into<String>, text: String) -> Self {
        let allowances = Allowances::parse(&text, &[]);
        Self {
            path: path.into(),
            line_starts: line_starts(&text),
            text,
            allowances,
        }
    }

    /// `(line number, line)` pairs, 1-based.
    pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.text.lines().enumerate().map(|(i, l)| (i + 1, l))
    }

    /// The 1-based line holding byte `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    /// The text of 1-based `line`, without its newline.
    pub fn line(&self, line: usize) -> &str {
        self.text.lines().nth(line.saturating_sub(1)).unwrap_or("")
    }

    pub fn extension(&self) -> &str {
        self.path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// A Rust file that parses.
#[derive(Debug)]
pub struct RustFile {
    pub path: String,
    pub text: String,
    pub ast: syn::File,
    pub comments: Comments,
    pub allowances: Allowances,
    line_starts: Vec<usize>,
    /// Bytes `syn::parse_file` skipped (BOM, shebang) before its spans start.
    offset: usize,
    test_lines: Vec<Range<usize>>,
}

impl RustFile {
    pub fn parse(path: impl Into<String>, text: String) -> Result<Self, syn::Error> {
        let ast = syn::parse_file(&text)?;
        let mut file = Self {
            path: path.into(),
            offset: skipped_prefix(&text),
            line_starts: line_starts(&text),
            comments: Comments::scan(&text),
            allowances: Allowances::default(),
            ast,
            text,
            test_lines: Vec::new(),
        };
        let scopes = file.scopes();
        file.allowances = Allowances::parse(&file.text, &scopes);
        let whole = crate::workspace::is_test_path(&file.path);
        let regions = TestRegions::collect(&file.ast, whole, |span| file.range(span));
        file.test_lines = regions
            .ranges()
            .iter()
            .map(|r| {
                if r.end == usize::MAX {
                    1..usize::MAX
                } else {
                    file.line_of(r.start)..file.line_of(r.end) + 1
                }
            })
            .collect();
        Ok(file)
    }

    /// Whether 1-based `line` is test code: `#[cfg(test)]`, `#[test]`, or a
    /// file under `tests/`, `benches/`, `examples/`.
    pub fn in_test(&self, line: usize) -> bool {
        self.test_lines.iter().any(|r| r.contains(&line))
    }

    /// Whether the whole file is test code.
    pub fn is_test_file(&self) -> bool {
        crate::workspace::is_test_path(&self.path)
    }

    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    /// Byte range of a span in `text`.
    pub fn range(&self, span: Span) -> Range<usize> {
        let r = span.byte_range();
        r.start + self.offset..r.end + self.offset
    }

    /// 1-based line where `node` starts.
    pub fn line_of_node<T: Spanned>(&self, node: &T) -> usize {
        node.span().start().line
    }

    /// 1-based `(line, column)` where `node` starts.
    pub fn pos<T: Spanned>(&self, node: &T) -> (usize, usize) {
        let start = node.span().start();
        (start.line, start.column + 1)
    }

    pub fn text_of<T: Spanned>(&self, node: &T) -> &str {
        let r = self.range(node.span());
        self.text.get(r).unwrap_or("")
    }

    pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.text.lines().enumerate().map(|(i, l)| (i + 1, l))
    }

    pub fn line(&self, line: usize) -> &str {
        self.text.lines().nth(line.saturating_sub(1)).unwrap_or("")
    }

    fn scopes(&self) -> Vec<Scope> {
        let mut collector = ScopeCollector { scopes: Vec::new() };
        collector.visit_file(&self.ast);
        collector.scopes
    }
}

struct ScopeCollector {
    scopes: Vec<Scope>,
}

impl ScopeCollector {
    fn record<T: Spanned>(&mut self, node: &T) {
        let span = node.span();
        self.scopes.push(Scope {
            start_line: span.start().line,
            end_line: span.end().line,
        });
    }
}

impl<'ast> Visit<'ast> for ScopeCollector {
    fn visit_field(&mut self, node: &'ast syn::Field) {
        self.record(node);
        syn::visit::visit_field(self, node);
    }
    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        self.record(node);
        syn::visit::visit_impl_item(self, node);
    }
    fn visit_item(&mut self, node: &'ast syn::Item) {
        self.record(node);
        syn::visit::visit_item(self, node);
    }
    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        self.record(node);
        syn::visit::visit_stmt(self, node);
    }
    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        self.record(node);
        syn::visit::visit_trait_item(self, node);
    }
    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        self.record(node);
        syn::visit::visit_variant(self, node);
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
    starts
}

/// `syn::parse_file` strips a BOM and a shebang line before parsing.
fn skipped_prefix(text: &str) -> usize {
    let mut offset = 0;
    let mut rest = text;
    if let Some(stripped) = rest.strip_prefix('\u{feff}') {
        offset += '\u{feff}'.len_utf8();
        rest = stripped;
    }
    if rest.starts_with("#!") && !rest.starts_with("#![") {
        offset += rest.find('\n').unwrap_or(rest.len());
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lines_cover_cfg_test_modules() {
        let text = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() {}\n}\n".to_string();
        let file = RustFile::parse("components/x/src/lib.rs", text).unwrap();
        assert!(!file.in_test(1));
        assert!(file.in_test(4));
    }

    #[test]
    fn files_under_tests_are_test_code() {
        let file = RustFile::parse("components/x/tests/it.rs", "fn a() {}".into()).unwrap();
        assert!(file.in_test(1));
    }

    #[test]
    fn line_lookup() {
        let file = TextFile::new("a.txt", "one\ntwo\nthree\n".into());
        assert_eq!(file.line_of(0), 1);
        assert_eq!(file.line_of(4), 2);
        assert_eq!(file.line(3), "three");
    }
}
