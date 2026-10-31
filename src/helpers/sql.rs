//! Helpers shared by the sql rules: finding SQL in Rust strings and `.sql`
//! files, a small SQL lexer, and a model of the DDL a crate declares.

use std::collections::{BTreeMap, HashSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::source::RustFile;
use crate::syntax;
use crate::workspace::{is_test_path, Workspace};

// ---------------------------------------------------------------------------
// Where to look

/// Generated, vendored or tooling code that the sql rules leave alone.
pub fn skip_path(path: &str) -> bool {
    path.starts_with("components/external/")
        || path.contains("/generated/")
        || path.starts_with("tools/")
        || path.contains("/third_party/")
        || path.contains("/third-party/")
}

/// Methods (rusqlite and `sql_support::ConnExt`) whose first argument is SQL.
pub const SQL_METHODS: &[&str] = &[
    "execute",
    "execute_batch",
    "execute_cached",
    "execute_one",
    "execute_all",
    "prepare",
    "prepare_cached",
    "prepare_maybe_cached",
    "query_row",
    "query_row_and_then",
    "query_row_and_then_cachable",
    "query_rows_and_then",
    "query_rows_and_then_cached",
    "query_rows_into",
    "query_rows_into_cached",
    "query_one",
    "conn_ext_query_one",
    "try_query_one",
    "try_query_row",
    "exists",
];

// ---------------------------------------------------------------------------
// SQL text found in the workspace

/// The function a piece of code sits in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnCtx {
    pub name: String,
    /// Last segment of the trait of the enclosing `impl Trait for T`.
    pub trait_name: Option<String>,
    /// Last segment of the self type of the enclosing `impl`.
    pub self_ty: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
}

/// A string literal (or a whole `.sql` file) that holds SQL.
#[derive(Clone, Debug)]
pub struct SqlText {
    pub path: String,
    /// Line of the first character of `value`.
    pub line: usize,
    pub value: String,
    /// For each byte of `value`, how many lines below `line` it sits.
    line_map: Vec<u32>,
    /// The method this literal is (part of) the first argument of.
    pub method: Option<String>,
    /// Inside a macro such as `format!`: may hold `{}` holes.
    pub in_macro: bool,
    /// The `const`/`static` this literal initializes.
    pub item: Option<String>,
    pub func: Option<FnCtx>,
    pub in_test: bool,
    /// A whole `.sql` file rather than a Rust literal.
    pub from_file: bool,
}

impl SqlText {
    /// The 1-based line of byte `offset` of `value`.
    pub fn line_at(&self, offset: usize) -> usize {
        let delta = self
            .line_map
            .get(offset.min(self.line_map.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        self.line + delta as usize
    }

    /// Whether the enclosing function is called `name`.
    pub fn in_fn(&self, name: &str) -> bool {
        self.func.as_ref().is_some_and(|f| f.name == name)
    }

    /// Whether this text sits in a schema migration (`upgrade_from`, `migrate*`, `upgrade*`).
    pub fn in_migration(&self) -> bool {
        self.func.as_ref().is_some_and(|f| is_migration_fn(&f.name))
    }

    pub fn tokens(&self) -> Vec<Tok<'_>> {
        lex(&self.value)
    }

    pub fn statements(&self) -> Vec<Statement<'_>> {
        statements(&self.value)
    }
}

/// Functions that migrate a schema from an older version.
pub fn is_migration_fn(name: &str) -> bool {
    name == "upgrade_from"
        || name.starts_with("upgrade")
        || name.starts_with("migrate")
        || name.starts_with("migration")
}

/// A reference to a constant passed as the SQL argument of a call:
/// `tx.execute_batch(CREATE_SHARED_SCHEMA_SQL)`.
#[derive(Clone, Debug)]
pub struct SqlConstUse {
    pub path: String,
    pub line: usize,
    pub ident: String,
    pub method: String,
    pub func: Option<FnCtx>,
}

/// Every piece of SQL in the workspace: string literals in Rust files (that
/// look like SQL) and `.sql` files. Skips generated and vendored code.
pub fn sql_texts(ws: &Workspace) -> Vec<SqlText> {
    let mut out = Vec::new();
    for file in ws.rust_files() {
        if skip_path(&file.path) {
            continue;
        }
        out.extend(rust_sql(file).0);
    }
    for file in ws.files_with_ext(&["sql"]) {
        if skip_path(&file.path) {
            continue;
        }
        out.push(sql_file(&file.path, &file.text));
    }
    out
}

/// A `.sql` file as one piece of SQL.
pub fn sql_file(path: &str, text: &str) -> SqlText {
    let mut line_map = Vec::with_capacity(text.len());
    let mut line = 0u32;
    for b in text.bytes() {
        line_map.push(line);
        if b == b'\n' {
            line += 1;
        }
    }
    SqlText {
        path: path.to_string(),
        line: 1,
        value: text.to_string(),
        line_map,
        method: None,
        in_macro: false,
        item: None,
        func: None,
        in_test: is_test_path(path),
        from_file: true,
    }
}

/// The SQL literals of one Rust file, and the constants passed as SQL.
pub fn rust_sql(file: &RustFile) -> (Vec<SqlText>, Vec<SqlConstUse>) {
    let mut c = Collector {
        file,
        fns: Vec::new(),
        impls: Vec::new(),
        items: Vec::new(),
        sink: None,
        in_macro: 0,
        seen: HashSet::new(),
        out: Vec::new(),
        consts: Vec::new(),
    };
    c.visit_file(&file.ast);
    (c.out, c.consts)
}

/// Every function of a file with its context.
pub fn functions(file: &RustFile) -> Vec<(FnCtx, &syn::Block, &syn::Signature)> {
    struct V<'a> {
        impls: Vec<(Option<String>, Option<String>)>,
        out: Vec<(FnCtx, &'a syn::Block, &'a syn::Signature)>,
    }
    impl<'a> V<'a> {
        fn push(
            &mut self,
            sig: &'a syn::Signature,
            block: &'a syn::Block,
            whole: proc_macro2::Span,
        ) {
            let (trait_name, self_ty) = self.impls.last().cloned().unwrap_or((None, None));
            self.out.push((
                FnCtx {
                    name: sig.ident.to_string(),
                    trait_name,
                    self_ty,
                    start_line: whole.start().line,
                    end_line: whole.end().line,
                },
                block,
                sig,
            ));
        }
    }
    impl<'a> Visit<'a> for V<'a> {
        fn visit_item_impl(&mut self, i: &'a syn::ItemImpl) {
            self.impls.push(impl_ctx(i));
            syn::visit::visit_item_impl(self, i);
            self.impls.pop();
        }
        fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
            self.push(&f.sig, &f.block, f.span());
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
            self.push(&f.sig, &f.block, f.span());
            syn::visit::visit_impl_item_fn(self, f);
        }
        fn visit_trait_item_fn(&mut self, f: &'a syn::TraitItemFn) {
            if let Some(block) = &f.default {
                self.push(&f.sig, block, f.span());
            }
            syn::visit::visit_trait_item_fn(self, f);
        }
    }
    let mut v = V {
        impls: Vec::new(),
        out: Vec::new(),
    };
    v.visit_file(&file.ast);
    v.out
}

fn impl_ctx(i: &syn::ItemImpl) -> (Option<String>, Option<String>) {
    let trait_name = i
        .trait_
        .as_ref()
        .and_then(|(_, p, _)| p.segments.last())
        .map(|s| s.ident.to_string());
    (trait_name, syntax::type_ident(&i.self_ty))
}

struct Collector<'f> {
    file: &'f RustFile,
    fns: Vec<FnCtx>,
    impls: Vec<(Option<String>, Option<String>)>,
    items: Vec<String>,
    sink: Option<String>,
    in_macro: usize,
    seen: HashSet<(usize, usize)>,
    out: Vec<SqlText>,
    consts: Vec<SqlConstUse>,
}

impl Collector<'_> {
    fn enter_fn(&mut self, sig: &syn::Signature, span: proc_macro2::Span) {
        let (trait_name, self_ty) = self.impls.last().cloned().unwrap_or((None, None));
        self.fns.push(FnCtx {
            name: sig.ident.to_string(),
            trait_name,
            self_ty,
            start_line: span.start().line,
            end_line: span.end().line,
        });
    }

    fn record(&mut self, span: proc_macro2::Span) {
        let start = span.start();
        if !self.seen.insert((start.line, start.column)) {
            return;
        }
        let range = self.file.range(span);
        let Some(raw) = self.file.text.get(range) else {
            return;
        };
        let Some((value, line_map)) = decode_literal(raw) else {
            return;
        };
        let sink = self.sink.is_some();
        if !looks_like_sql(&value, sink) {
            return;
        }
        self.out.push(SqlText {
            path: self.file.path.clone(),
            line: start.line,
            value,
            line_map,
            method: self.sink.clone(),
            in_macro: self.in_macro > 0,
            item: self.items.last().cloned(),
            func: self.fns.last().cloned(),
            in_test: self.file.in_test(start.line),
            from_file: false,
        });
    }

    fn walk_tokens(&mut self, stream: TokenStream) {
        for tt in stream {
            match tt {
                TokenTree::Group(g) => self.walk_tokens(g.stream()),
                TokenTree::Literal(lit) => {
                    let text = lit.to_string();
                    if text.starts_with('"') || text.starts_with("r\"") || text.starts_with("r#") {
                        self.record(lit.span());
                    }
                }
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        self.impls.push(impl_ctx(i));
        syn::visit::visit_item_impl(self, i);
        self.impls.pop();
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.enter_fn(&f.sig, f.span());
        syn::visit::visit_item_fn(self, f);
        self.fns.pop();
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.enter_fn(&f.sig, f.span());
        syn::visit::visit_impl_item_fn(self, f);
        self.fns.pop();
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.enter_fn(&f.sig, f.span());
        syn::visit::visit_trait_item_fn(self, f);
        self.fns.pop();
    }

    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        self.items.push(c.ident.to_string());
        syn::visit::visit_item_const(self, c);
        self.items.pop();
    }

    fn visit_impl_item_const(&mut self, c: &'ast syn::ImplItemConst) {
        self.items.push(c.ident.to_string());
        syn::visit::visit_impl_item_const(self, c);
        self.items.pop();
    }

    fn visit_item_static(&mut self, s: &'ast syn::ItemStatic) {
        self.items.push(s.ident.to_string());
        syn::visit::visit_item_static(self, s);
        self.items.pop();
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.visit_expr(&m.receiver);
        let method = m.method.to_string();
        let is_sql = SQL_METHODS.contains(&method.as_str());
        for (i, arg) in m.args.iter().enumerate() {
            if i == 0 && is_sql {
                if let Some(ident) = const_ident(arg) {
                    self.consts.push(SqlConstUse {
                        path: self.file.path.clone(),
                        line: arg.span().start().line,
                        ident,
                        method: method.clone(),
                        func: self.fns.last().cloned(),
                    });
                }
                let saved = self.sink.replace(method.clone());
                self.visit_expr(arg);
                self.sink = saved;
            } else {
                let saved = self.sink.take();
                self.visit_expr(arg);
                self.sink = saved;
            }
        }
    }

    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        let saved = self.sink.take();
        syn::visit::visit_expr_closure(self, c);
        self.sink = saved;
    }

    fn visit_lit_str(&mut self, s: &'ast syn::LitStr) {
        self.record(s.span());
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.in_macro += 1;
        let parsed = m.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated);
        match parsed {
            Ok(exprs) => {
                for e in &exprs {
                    self.visit_expr(e);
                }
            }
            Err(_) => self.walk_tokens(m.tokens.clone()),
        }
        self.in_macro -= 1;
    }
}

/// `X`, `&X`, `&*X`, `(X)`: the name of a constant used as an argument.
pub fn const_ident(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(p) => {
            let last = p.path.segments.last()?.ident.to_string();
            let constant = last
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && last.chars().any(|c| c.is_ascii_uppercase());
            constant.then_some(last)
        }
        syn::Expr::Reference(r) => const_ident(&r.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => const_ident(&u.expr),
        syn::Expr::Paren(p) => const_ident(&p.expr),
        _ => None,
    }
}

/// Decode a Rust string literal's source (`"…"`, `r#"…"#`) into its value,
/// and for each byte of the value the number of source lines before it.
pub fn decode_literal(raw: &str) -> Option<(String, Vec<u32>)> {
    let mut value = String::new();
    let mut map = Vec::new();
    let mut line = 0u32;
    if let Some(rest) = raw.strip_prefix('r') {
        let hashes = rest.chars().take_while(|c| *c == '#').count();
        let body = rest.get(hashes + 1..rest.len().checked_sub(hashes + 1)?)?;
        for c in body.chars() {
            push_char(&mut value, &mut map, c, line);
            if c == '\n' {
                line += 1;
            }
        }
        return Some((value, map));
    }
    let body = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            push_char(&mut value, &mut map, c, line);
            if c == '\n' {
                line += 1;
            }
            continue;
        }
        match chars.next()? {
            'n' => push_char(&mut value, &mut map, '\n', line),
            't' => push_char(&mut value, &mut map, '\t', line),
            'r' => push_char(&mut value, &mut map, '\r', line),
            '0' => push_char(&mut value, &mut map, '\0', line),
            '\\' => push_char(&mut value, &mut map, '\\', line),
            '"' => push_char(&mut value, &mut map, '"', line),
            '\'' => push_char(&mut value, &mut map, '\'', line),
            'x' => {
                let hex: String = (0..2).filter_map(|_| chars.next()).collect();
                let c = u8::from_str_radix(&hex, 16).ok()? as char;
                push_char(&mut value, &mut map, c, line);
            }
            'u' => {
                let mut hex = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    if c != '{' {
                        hex.push(c);
                    }
                }
                let c = char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?;
                push_char(&mut value, &mut map, c, line);
            }
            '\n' => {
                line += 1;
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    if chars.next() == Some('\n') {
                        line += 1;
                    }
                }
            }
            '\r' => {}
            _ => return None,
        }
    }
    Some((value, map))
}

fn push_char(value: &mut String, map: &mut Vec<u32>, c: char, line: u32) {
    value.push(c);
    for _ in 0..c.len_utf8() {
        map.push(line);
    }
}

/// Statement keywords that start SQL.
const STARTERS: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "REPLACE",
    "WITH",
    "PRAGMA",
    "VACUUM",
    "ANALYZE",
    "REINDEX",
    "ATTACH",
    "DETACH",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    "RELEASE",
    "EXPLAIN",
];

/// Whether a string is SQL. Strings passed to a SQL method only need to
/// start with a statement keyword; others must start with an uppercase one
/// and read like a statement (`SELECT … FROM`, `INSERT INTO`, `PRAGMA`…).
pub fn looks_like_sql(text: &str, passed_to_sql_method: bool) -> bool {
    let toks = lex(text);
    let mut words = toks.iter().filter(|t| t.kind == TokKind::Word);
    let Some(first) = words.next() else {
        return false;
    };
    let upper = first.text.to_ascii_uppercase();
    if !STARTERS.contains(&upper.as_str()) {
        return false;
    }
    if passed_to_sql_method {
        return true;
    }
    if first.text != upper {
        return false;
    }
    let has = |kw: &str| toks.iter().any(|t| t.kind == TokKind::Word && t.text == kw);
    match upper.as_str() {
        "SELECT" => has("FROM") || toks.len() <= 4,
        "INSERT" | "REPLACE" => has("INTO"),
        "UPDATE" => has("SET"),
        "DELETE" => has("FROM"),
        "CREATE" | "DROP" => has("TABLE") || has("INDEX") || has("TRIGGER") || has("VIEW"),
        "ALTER" => has("TABLE"),
        "WITH" => has("SELECT") || has("AS"),
        "PRAGMA" | "VACUUM" | "ANALYZE" | "REINDEX" | "SAVEPOINT" | "EXPLAIN" => {
            toks.len() >= 2 || upper == "VACUUM"
        }
        _ => {
            toks.len() >= 2
                && toks
                    .iter()
                    .all(|t| t.kind != TokKind::Punct || t.text == ";")
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    /// An unquoted identifier or keyword.
    Word,
    /// `"ident"`, `` `ident` `` or `[ident]`.
    Quoted,
    /// `'text'` or `x'…'`.
    Str,
    Num,
    /// `?`, `?1`, `:name`, `@name`, `$name`.
    Param,
    /// A `format!` hole: `{}` or `{name}`.
    Hole,
    /// Punctuation and operators: `(`, `,`, `||`, `<=`…
    Punct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok<'a> {
    pub kind: TokKind,
    pub text: &'a str,
    /// Byte offset in the lexed text.
    pub start: usize,
}

impl Tok<'_> {
    /// A keyword or identifier, case-insensitively.
    pub fn is(&self, word: &str) -> bool {
        self.kind == TokKind::Word && self.text.eq_ignore_ascii_case(word)
    }

    pub fn is_punct(&self, p: &str) -> bool {
        self.kind == TokKind::Punct && self.text == p
    }

    /// The name of an identifier, without quotes.
    pub fn name(&self) -> String {
        match self.kind {
            TokKind::Quoted => self.text[1..self.text.len() - 1].to_string(),
            _ => self.text.to_string(),
        }
    }
}

/// Split SQL into tokens, dropping whitespace and comments.
pub fn lex(sql: &str) -> Vec<Tok<'_>> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        let kind;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        } else if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        } else if (c == b'x' || c == b'X') && bytes.get(i + 1) == Some(&b'\'') {
            i = skip_quoted(bytes, i + 1, b'\'');
            kind = TokKind::Str;
        } else if c == b'\'' {
            i = skip_quoted(bytes, i, b'\'');
            kind = TokKind::Str;
        } else if c == b'"' || c == b'`' {
            i = skip_quoted(bytes, i, c);
            kind = TokKind::Quoted;
        } else if c == b'[' {
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            kind = TokKind::Quoted;
        } else if c == b'{' {
            while i < bytes.len() && bytes[i] != b'}' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            kind = TokKind::Hole;
        } else if c == b'?'
            || ((c == b':' || c == b'@' || c == b'$')
                && bytes.get(i + 1).is_some_and(|b| is_word(*b)))
        {
            i += 1;
            while i < bytes.len() && is_word(bytes[i]) {
                i += 1;
            }
            kind = TokKind::Param;
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            kind = TokKind::Num;
        } else if is_word(c) || c >= 0x80 {
            while i < bytes.len() && (is_word(bytes[i]) || bytes[i] >= 0x80) {
                i += 1;
            }
            kind = TokKind::Word;
        } else {
            let two = sql.get(i..i + 2).unwrap_or("");
            i += if matches!(
                two,
                "||" | "<=" | ">=" | "<>" | "!=" | "==" | "<<" | ">>" | "->"
            ) {
                2
            } else {
                1
            };
            kind = TokKind::Punct;
        }
        // Only cut on char boundaries (multi-byte chars inside words/strings).
        while i < bytes.len() && !sql.is_char_boundary(i) {
            i += 1;
        }
        out.push(Tok {
            kind,
            text: &sql[start..i],
            start,
        });
    }
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_quoted(bytes: &[u8], open: usize, quote: u8) -> usize {
    let mut i = open + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// One statement: its tokens (without the closing `;`).
#[derive(Clone, Debug)]
pub struct Statement<'a> {
    pub toks: Vec<Tok<'a>>,
}

impl<'a> Statement<'a> {
    pub fn start(&self) -> usize {
        self.toks.first().map(|t| t.start).unwrap_or(0)
    }

    /// The first keyword, uppercased.
    pub fn verb(&self) -> String {
        self.toks
            .first()
            .map(|t| t.text.to_ascii_uppercase())
            .unwrap_or_default()
    }

    /// Whether the statement is DDL on a persistent object (not `TEMP`).
    pub fn ddl(&self) -> Option<Ddl> {
        ddl_of(&self.toks)
    }
}

/// Split SQL into statements on top-level `;`, keeping trigger bodies whole.
pub fn statements(sql: &str) -> Vec<Statement<'_>> {
    split_statements(lex(sql))
}

pub fn split_statements(toks: Vec<Tok<'_>>) -> Vec<Statement<'_>> {
    let mut out = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut depth = 0i32;
    let mut trigger = false;
    let mut in_body = false;
    let mut case_depth = 0;
    for t in toks {
        if cur.is_empty() && t.is_punct(";") {
            continue;
        }
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        }
        if cur.len() < 4 && t.is("TRIGGER") && cur.first().is_some_and(|f| f.is("CREATE")) {
            trigger = true;
        }
        if trigger && depth == 0 {
            if t.is("BEGIN") {
                in_body = true;
            } else if in_body && t.is("CASE") {
                case_depth += 1;
            } else if in_body && t.is("END") {
                if case_depth > 0 {
                    case_depth -= 1;
                } else {
                    in_body = false;
                }
            }
        }
        if t.is_punct(";") && depth <= 0 && !in_body {
            out.push(Statement {
                toks: std::mem::take(&mut cur),
            });
            trigger = false;
            depth = 0;
            continue;
        }
        cur.push(t);
    }
    if !cur.is_empty() {
        out.push(Statement { toks: cur });
    }
    out
}

/// A `CREATE`/`DROP`/`ALTER` of a persistent schema object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ddl {
    /// `CREATE`, `DROP` or `ALTER`.
    pub verb: String,
    /// `TABLE`, `INDEX`, `TRIGGER` or `VIEW`.
    pub object: String,
    pub name: String,
    pub temp: bool,
    pub virtual_table: bool,
    pub if_exists: bool,
}

fn ddl_of(toks: &[Tok]) -> Option<Ddl> {
    let verb = toks.first()?.text.to_ascii_uppercase();
    if !matches!(verb.as_str(), "CREATE" | "DROP" | "ALTER") {
        return None;
    }
    let mut i = 1;
    let mut temp = false;
    let mut virtual_table = false;
    while let Some(t) = toks.get(i) {
        if t.is("TEMP") || t.is("TEMPORARY") {
            temp = true;
        } else if t.is("VIRTUAL") {
            virtual_table = true;
        } else if !t.is("UNIQUE") {
            break;
        }
        i += 1;
    }
    let object = toks.get(i)?.text.to_ascii_uppercase();
    if !matches!(object.as_str(), "TABLE" | "INDEX" | "TRIGGER" | "VIEW") {
        return None;
    }
    i += 1;
    let mut if_exists = false;
    while toks
        .get(i)
        .is_some_and(|t| t.is("IF") || t.is("NOT") || t.is("EXISTS"))
    {
        if_exists = true;
        i += 1;
    }
    let (name, schema) = qualified_name(toks, i)?;
    if schema
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("temp"))
    {
        temp = true;
    }
    Some(Ddl {
        verb,
        object,
        name,
        temp,
        virtual_table,
        if_exists,
    })
}

/// `name` or `schema.name` at `i`: `(name, schema)`.
fn qualified_name(toks: &[Tok], i: usize) -> Option<(String, Option<String>)> {
    let first = toks.get(i)?;
    if !matches!(first.kind, TokKind::Word | TokKind::Quoted | TokKind::Hole) {
        return None;
    }
    if toks.get(i + 1).is_some_and(|t| t.is_punct(".")) {
        let second = toks.get(i + 2)?;
        return Some((second.name(), Some(first.name())));
    }
    Some((first.name(), None))
}

// ---------------------------------------------------------------------------
// Schema model

#[derive(Clone, Debug, Default)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub not_null: bool,
    pub primary_key: bool,
    /// Byte offset of the definition in the SQL text.
    pub start: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub parent: String,
    /// `CASCADE`, `SET NULL`, `RESTRICT`... `None` when not declared.
    pub on_delete: Option<String>,
    pub start: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub name: String,
    pub temp: bool,
    pub virtual_table: bool,
    pub columns: Vec<Column>,
    /// Columns of the primary key, in order.
    pub primary_key: Vec<String>,
    /// Column lists of `UNIQUE` constraints.
    pub unique: Vec<Vec<String>>,
    pub foreign_keys: Vec<ForeignKey>,
    pub start: usize,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Index {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub start: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Trigger {
    pub name: String,
    pub table: String,
    /// `DELETE`, `INSERT` or `UPDATE`.
    pub event: String,
    pub temp: bool,
    /// Tables the trigger body deletes from.
    pub deletes_from: Vec<String>,
    pub start: usize,
}

/// Parse a `CREATE TABLE` statement.
pub fn parse_table(stmt: &Statement) -> Option<Table> {
    let ddl = stmt.ddl()?;
    if ddl.verb != "CREATE" || ddl.object != "TABLE" {
        return None;
    }
    let toks = &stmt.toks;
    let mut table = Table {
        name: ddl.name,
        temp: ddl.temp,
        virtual_table: ddl.virtual_table,
        start: stmt.start(),
        ..Default::default()
    };
    if ddl.virtual_table {
        return Some(table);
    }
    let open = toks.iter().position(|t| t.is_punct("("))?;
    let close = matching(toks, open)?;
    for def in split_top(&toks[open + 1..close]) {
        parse_def(def, &mut table);
    }
    Some(table)
}

fn parse_def(def: &[Tok], table: &mut Table) {
    let Some(first) = def.first() else { return };
    let mut def = def;
    if first.is("CONSTRAINT") {
        def = &def[2.min(def.len())..];
    }
    let Some(first) = def.first() else { return };
    if first.is("PRIMARY") {
        table.primary_key = paren_names(def, 0);
        return;
    }
    if first.is("UNIQUE") {
        table.unique.push(paren_names(def, 0));
        return;
    }
    if first.is("CHECK") {
        return;
    }
    if first.is("FOREIGN") {
        let columns = paren_names(def, 0);
        if let Some(r) = def.iter().position(|t| t.is("REFERENCES")) {
            table.foreign_keys.push(ForeignKey {
                columns,
                parent: def.get(r + 1).map(|t| t.name()).unwrap_or_default(),
                on_delete: on_delete(&def[r..]),
                start: first.start,
            });
        }
        return;
    }
    if first.kind == TokKind::Hole {
        return;
    }
    let name = first.name();
    let mut ty = Vec::new();
    let mut col = Column {
        name: name.clone(),
        start: first.start,
        ..Default::default()
    };
    let mut i = 1;
    while let Some(t) = def.get(i) {
        if t.kind == TokKind::Word
            && ![
                "CONSTRAINT",
                "PRIMARY",
                "NOT",
                "NULL",
                "UNIQUE",
                "CHECK",
                "DEFAULT",
                "COLLATE",
                "REFERENCES",
                "GENERATED",
                "AS",
            ]
            .iter()
            .any(|k| t.is(k))
        {
            ty.push(t.text);
            i += 1;
        } else if t.is_punct("(") {
            i = matching(def, i).map(|c| c + 1).unwrap_or(def.len());
        } else {
            break;
        }
    }
    col.ty = ty.join(" ").to_ascii_uppercase();
    while i < def.len() {
        let t = def[i];
        if t.is("NOT") && def.get(i + 1).is_some_and(|n| n.is("NULL")) {
            col.not_null = true;
        } else if t.is("PRIMARY") {
            col.primary_key = true;
            table.primary_key = vec![name.clone()];
        } else if t.is("UNIQUE") {
            table.unique.push(vec![name.clone()]);
        } else if t.is("REFERENCES") {
            table.foreign_keys.push(ForeignKey {
                columns: vec![name.clone()],
                parent: def.get(i + 1).map(|t| t.name()).unwrap_or_default(),
                on_delete: on_delete(&def[i..]),
                start: first.start,
            });
        } else if t.is_punct("(") {
            i = matching(def, i).unwrap_or(def.len());
        }
        i += 1;
    }
    table.columns.push(col);
}

/// The `ON DELETE` action in a `REFERENCES …` clause.
fn on_delete(toks: &[Tok]) -> Option<String> {
    let at = toks
        .windows(2)
        .position(|w| w[0].is("ON") && w[1].is("DELETE"))?;
    let action: Vec<String> = toks[at + 2..]
        .iter()
        .take_while(|t| {
            [
                "CASCADE", "SET", "NULL", "DEFAULT", "RESTRICT", "NO", "ACTION",
            ]
            .iter()
            .any(|k| t.is(k))
        })
        .map(|t| t.text.to_ascii_uppercase())
        .collect();
    Some(action.join(" "))
}

/// Names in the first `( … )` after `from`.
fn paren_names(toks: &[Tok], from: usize) -> Vec<String> {
    let Some(open) = toks[from..]
        .iter()
        .position(|t| t.is_punct("("))
        .map(|p| p + from)
    else {
        return Vec::new();
    };
    let Some(close) = matching(toks, open) else {
        return Vec::new();
    };
    split_top(&toks[open + 1..close])
        .into_iter()
        .filter_map(|part| part.first().map(|t| t.name()))
        .collect()
}

/// The index of the `)` that closes the `(` at `open`.
pub fn matching(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in toks.iter().enumerate().skip(open) {
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Split tokens on top-level commas.
pub fn split_top<'t, 'a>(toks: &'t [Tok<'a>]) -> Vec<&'t [Tok<'a>]> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, t) in toks.iter().enumerate() {
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        } else if t.is_punct(",") && depth == 0 {
            out.push(&toks[start..i]);
            start = i + 1;
        }
    }
    if start < toks.len() {
        out.push(&toks[start..]);
    }
    out
}

/// Parse a `CREATE INDEX` statement.
pub fn parse_index(stmt: &Statement) -> Option<Index> {
    let ddl = stmt.ddl()?;
    if ddl.verb != "CREATE" || ddl.object != "INDEX" {
        return None;
    }
    let on = stmt.toks.iter().position(|t| t.is("ON"))?;
    let (table, _) = qualified_name(&stmt.toks, on + 1)?;
    Some(Index {
        name: ddl.name,
        table,
        columns: paren_names(&stmt.toks, on),
        start: stmt.start(),
    })
}

/// Parse a `CREATE TRIGGER` statement.
pub fn parse_trigger(stmt: &Statement) -> Option<Trigger> {
    let ddl = stmt.ddl()?;
    if ddl.verb != "CREATE" || ddl.object != "TRIGGER" {
        return None;
    }
    let toks = &stmt.toks;
    let on = toks.iter().position(|t| t.is("ON"))?;
    let event = toks[..on]
        .iter()
        .find(|t| t.is("DELETE") || t.is("INSERT") || t.is("UPDATE"))?
        .text
        .to_ascii_uppercase();
    let (table, _) = qualified_name(toks, on + 1)?;
    let deletes_from = toks
        .windows(3)
        .filter(|w| w[0].is("DELETE") && w[1].is("FROM"))
        .map(|w| w[2].name())
        .collect();
    Some(Trigger {
        name: ddl.name,
        table,
        event,
        temp: ddl.temp,
        deletes_from,
        start: stmt.start(),
    })
}

/// The persistent schema a crate declares outside of tests and migrations.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    /// `(table, where it is declared)`.
    pub tables: Vec<(Table, SqlText)>,
    pub indexes: Vec<(Index, SqlText)>,
    pub triggers: Vec<(Trigger, SqlText)>,
}

impl Schema {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .map(|(t, _)| t)
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Foreign keys (with their table) that reference `parent`.
    pub fn children_of<'s>(
        &'s self,
        parent: &'s str,
    ) -> impl Iterator<Item = (&'s Table, &'s ForeignKey)> + 's {
        self.tables.iter().flat_map(move |(t, _)| {
            t.foreign_keys
                .iter()
                .filter(move |fk| fk.parent.eq_ignore_ascii_case(parent))
                .map(move |fk| (t, fk))
        })
    }
}

/// The schema declared by `texts` that are not test code or migrations.
pub fn schema_from(texts: &[&SqlText]) -> Schema {
    let mut schema = Schema::default();
    for text in texts {
        if text.in_test || text.in_migration() {
            continue;
        }
        for stmt in text.statements() {
            if let Some(t) = parse_table(&stmt) {
                if !schema
                    .tables
                    .iter()
                    .any(|(o, _)| o.name.eq_ignore_ascii_case(&t.name))
                {
                    schema.tables.push((t, (*text).clone()));
                }
            } else if let Some(i) = parse_index(&stmt) {
                schema.indexes.push((i, (*text).clone()));
            } else if let Some(t) = parse_trigger(&stmt) {
                schema.triggers.push((t, (*text).clone()));
            }
        }
    }
    schema
}

/// SQL texts grouped by the package (crate) they belong to: each crate has
/// its own database, so schemas and foreign keys only matter within one.
pub fn by_package<'t>(ws: &Workspace, texts: &'t [SqlText]) -> BTreeMap<String, Vec<&'t SqlText>> {
    let mut out: BTreeMap<String, Vec<&SqlText>> = BTreeMap::new();
    for text in texts {
        let key = ws
            .package_of(&text.path)
            .map(|p| p.manifest.clone())
            .unwrap_or_default();
        out.entry(key).or_default().push(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_literals_with_lines() {
        let (v, map) = decode_literal("\"a\\\n   b\nc\"").unwrap();
        assert_eq!(v, "ab\nc");
        assert_eq!(map, vec![0, 1, 1, 2]);
        let (v, _) = decode_literal("r#\"x \"y\"\"#").unwrap();
        assert_eq!(v, "x \"y\"");
    }

    #[test]
    fn lexes_and_splits() {
        let sql = "CREATE TABLE t(a INTEGER NOT NULL -- c;\n, b TEXT); \
                   CREATE TEMP TRIGGER x AFTER DELETE ON t BEGIN DELETE FROM u; SELECT CASE WHEN 1 THEN 2 END; END;\
                   SELECT ':a', :b || '%'";
        let stmts = statements(sql);
        assert_eq!(stmts.len(), 3);
        let table = parse_table(&stmts[0]).unwrap();
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[0].not_null);
        let trigger = parse_trigger(&stmts[1]).unwrap();
        assert!(trigger.temp);
        assert_eq!(trigger.event, "DELETE");
        assert_eq!(trigger.deletes_from, vec!["u"]);
        let params: Vec<_> = stmts[2]
            .toks
            .iter()
            .filter(|t| t.kind == TokKind::Param)
            .collect();
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn parses_foreign_keys() {
        let sql = "CREATE TABLE c(id INTEGER PRIMARY KEY, p INTEGER NOT NULL REFERENCES p(id) ON DELETE CASCADE, \
                   q INTEGER, FOREIGN KEY(q) REFERENCES p(id))";
        let t = parse_table(&statements(sql)[0]).unwrap();
        assert_eq!(t.foreign_keys.len(), 2);
        assert_eq!(t.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));
        assert_eq!(t.foreign_keys[1].on_delete, None);
        assert_eq!(t.primary_key, vec!["id"]);
    }

    #[test]
    fn recognizes_sql() {
        assert!(looks_like_sql("SELECT a FROM t", false));
        assert!(looks_like_sql("select a from t", true));
        assert!(!looks_like_sql("select a from t", false));
        assert!(!looks_like_sql("Select the login to delete", false));
        assert!(!looks_like_sql("DELETE failed", false));
    }

    #[test]
    fn finds_sql_in_rust() {
        let text = "const SQL: &str = \"CREATE TABLE t(a)\";\n\
                    impl ConnectionInitializer for I {\n\
                    fn upgrade_from(&self, tx: &T, v: u32) {\n\
                    tx.execute_batch(&format!(\"DROP TABLE {}\", x));\n\
                    tx.execute_batch(SQL);\n\
                    }\n}\n";
        let file = RustFile::parse("components/a/src/schema.rs", text.to_string()).unwrap();
        let (sql, consts) = rust_sql(&file);
        assert_eq!(sql.len(), 2);
        assert_eq!(sql[0].item.as_deref(), Some("SQL"));
        assert_eq!(sql[1].method.as_deref(), Some("execute_batch"));
        assert!(sql[1].in_macro);
        assert!(sql[1].in_migration());
        assert_eq!(
            sql[1].func.as_ref().unwrap().trait_name.as_deref(),
            Some("ConnectionInitializer")
        );
        assert_eq!(consts.len(), 1);
        assert_eq!(consts[0].ident, "SQL");
    }
}
