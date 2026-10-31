//! Helpers shared by the api_mobile rules: a small Kotlin/Swift scanner, the
//! scope of hand-written mobile code, and a detailed view of the UniFFI
//! surface of Rust files (fields, arguments and variants with their types).

use std::collections::BTreeSet;

use syn::spanned::Spanned;

use crate::source::RustFile;
use crate::syntax;
use crate::workspace::{is_test_path, Workspace};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Vendored or generated code no rule should report on.
pub fn is_generated_or_vendored(path: &str) -> bool {
    path.starts_with("components/external/")
        || path.contains("/generated/")
        || path.contains("/Generated/")
        || path.contains("/target/")
}

/// A nimbus-fml Askama template (`backends/{kotlin,swift}/templates/*`):
/// its output ships inside apps.
pub fn is_fml_template(path: &str) -> bool {
    path.contains("nimbus-fml/src/backends/") && path.contains("/templates/")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Kotlin,
    Swift,
}

/// Hand-written Kotlin or Swift that ships to apps: the component wrappers
/// (`components/*/android/src/main`, `components/*/ios`) and the nimbus-fml
/// templates. Tests, fixtures and generated bindings are left out.
pub fn shipped_mobile_lang(path: &str) -> Option<Lang> {
    if is_generated_or_vendored(path) || !path.starts_with("components/") {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let testy = path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/androidTest/")
        || path.contains("/fixtures/")
        || path.contains("Tests/")
        || name.ends_with("Tests.swift")
        || name.ends_with("Test.kt")
        || name.ends_with("Tests.kt");
    if testy {
        return None;
    }
    if path.ends_with(".kt") && (path.contains("/android/src/main/") || is_fml_template(path)) {
        return Some(Lang::Kotlin);
    }
    if path.ends_with(".swift") && (path.contains("/ios/") || is_fml_template(path)) {
        return Some(Lang::Swift);
    }
    None
}

// ---------------------------------------------------------------------------
// A Kotlin/Swift scanner
// ---------------------------------------------------------------------------

/// A comment found by [`Code::scan`].
#[derive(Clone, Debug)]
pub struct MobileComment {
    /// Byte range in the original text.
    pub start: usize,
    pub end: usize,
    /// The comment as written, delimiters included.
    pub text: String,
}

/// Kotlin or Swift source with comments and string contents blanked out.
/// Byte offsets and line numbers match the original text.
#[derive(Clone, Debug)]
pub struct Code {
    /// The code: comments and the inside of string literals are spaces
    /// (newlines kept); Askama tags in templates are blanked too, except
    /// `{{ expr }}` which becomes a run of `x` (it stands for a name).
    pub code: String,
    pub comments: Vec<MobileComment>,
    line_starts: Vec<usize>,
}

impl Code {
    pub fn scan(text: &str, lang: Lang, template: bool) -> Self {
        let mut bytes = text.as_bytes().to_vec();
        if template {
            blank_askama(&mut bytes);
        }
        let mut comments = Vec::new();
        let src = bytes.clone();
        let n = src.len();
        let mut i = 0;
        while i < n {
            let rest = &src[i..];
            if rest.starts_with(b"//") {
                let end = src[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(n, |p| i + p);
                comments.push(comment(text, i, end));
                blank(&mut bytes, i, end);
                i = end;
            } else if rest.starts_with(b"/*") {
                let mut depth = 0;
                let mut j = i;
                while j < n {
                    if src[j..].starts_with(b"/*") {
                        depth += 1;
                        j += 2;
                    } else if src[j..].starts_with(b"*/") {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                comments.push(comment(text, i, j));
                blank(&mut bytes, i, j);
                i = j;
            } else if rest.starts_with(b"\"\"\"") {
                let end = find(&src, i + 3, b"\"\"\"").unwrap_or(n);
                blank(&mut bytes, i + 3, end);
                i = (end + 3).min(n);
            } else if src[i] == b'"' {
                let end = string_end(&src, i + 1, lang);
                blank(&mut bytes, i + 1, end);
                i = (end + 1).min(n);
            } else if lang == Lang::Kotlin && src[i] == b'\'' {
                // A char literal: 'a', '\n', '"'.
                let mut j = i + 1;
                if j < n && src[j] == b'\\' {
                    j += 2;
                } else {
                    j += utf8_len(src.get(j).copied().unwrap_or(b' '));
                }
                if j < n && src[j] == b'\'' {
                    blank(&mut bytes, i + 1, j);
                    i = j + 1;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        let code = String::from_utf8(bytes).unwrap_or_else(|_| text.to_string());
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            code,
            comments,
            line_starts,
        }
    }

    /// The 1-based line holding byte `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    /// 1-based column of byte `offset`.
    pub fn column_of(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        offset - self.line_starts[line - 1] + 1
    }

    /// `(line number, code line)` pairs, 1-based.
    pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.code.lines().enumerate().map(|(i, l)| (i + 1, l))
    }

    /// Byte offset where 1-based `line` starts.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(self.code.len())
    }
}

fn comment(text: &str, start: usize, end: usize) -> MobileComment {
    MobileComment {
        start,
        end,
        text: text.get(start..end).unwrap_or("").to_string(),
    }
}

fn blank(bytes: &mut [u8], start: usize, end: usize) {
    let end = end.min(bytes.len());
    for b in bytes.iter_mut().take(end).skip(start) {
        if *b != b'\n' {
            *b = b' ';
        }
    }
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..bytes.len().saturating_sub(needle.len() - 1)).find(|&i| bytes[i..].starts_with(needle))
}

fn utf8_len(first: u8) -> usize {
    match first {
        b if b < 0x80 => 1,
        b if b >= 0xF0 => 4,
        b if b >= 0xE0 => 3,
        _ => 2,
    }
}

/// The index of the `"` closing a string that starts at `from`, skipping
/// escapes and interpolations (`${..}` in Kotlin, `\(..)` in Swift).
fn string_end(src: &[u8], from: usize, lang: Lang) -> usize {
    let n = src.len();
    let mut j = from;
    while j < n {
        match src[j] {
            b'\\' if lang == Lang::Swift && src.get(j + 1) == Some(&b'(') => {
                j = matching(src, j + 1, b'(', b')');
            }
            b'\\' => j += 2,
            b'$' if lang == Lang::Kotlin && src.get(j + 1) == Some(&b'{') => {
                j = matching(src, j + 1, b'{', b'}');
            }
            b'"' | b'\n' => return j,
            _ => j += 1,
        }
    }
    n
}

/// Index just past the bracket closing the one at `open`, skipping nested
/// simple strings.
fn matching(src: &[u8], open: usize, left: u8, right: u8) -> usize {
    let mut depth = 0;
    let mut j = open;
    while j < src.len() {
        let b = src[j];
        if b == b'"' {
            j += 1;
            while j < src.len() && src[j] != b'"' && src[j] != b'\n' {
                j += if src[j] == b'\\' { 2 } else { 1 };
            }
        } else if b == left {
            depth += 1;
        } else if b == right {
            depth -= 1;
            if depth == 0 {
                return j + 1;
            }
        }
        j += 1;
    }
    src.len()
}

/// Blank Askama `{% .. %}` and `{# .. #}` tags; turn `{{ .. }}` into `xxxx`.
fn blank_askama(bytes: &mut [u8]) {
    let n = bytes.len();
    let mut i = 0;
    while i + 1 < n {
        let close: Option<&[u8]> = match (bytes[i], bytes[i + 1]) {
            (b'{', b'%') => Some(b"%}"),
            (b'{', b'#') => Some(b"#}"),
            (b'{', b'{') => Some(b"}}"),
            _ => None,
        };
        let Some(close) = close else {
            i += 1;
            continue;
        };
        let end = find(bytes, i + 2, close).map_or(n, |e| e + 2);
        let fill = if bytes[i + 1] == b'{' { b'x' } else { b' ' };
        for b in bytes.iter_mut().take(end).skip(i) {
            if *b != b'\n' {
                *b = fill;
            }
        }
        i = end;
    }
}

/// Each loaded file that is shipped Kotlin/Swift, scanned.
pub fn shipped_mobile_files(ws: &Workspace) -> Vec<(String, Lang, Code)> {
    ws.files()
        .filter_map(|f| {
            let lang = shipped_mobile_lang(&f.path)?;
            Some((
                f.path.clone(),
                lang,
                Code::scan(&f.text, lang, is_fml_template(&f.path)),
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rust: which files count, and the exported surface in detail
// ---------------------------------------------------------------------------

/// Production Rust code of a crate: not tests, examples, generated or vendored.
pub fn is_production_rust(path: &str) -> bool {
    path.ends_with(".rs")
        && !is_test_path(path)
        && !is_generated_or_vendored(path)
        && !path.starts_with("testing/")
        && !path.starts_with("examples/")
        && !path.contains("/examples/")
        && !path.ends_with("/build.rs")
        && path != "build.rs"
}

/// Production Rust files, parsed.
pub fn production_rust(ws: &Workspace) -> impl Iterator<Item = &RustFile> {
    ws.rust_files().filter(|f| is_production_rust(&f.path))
}

/// `#[cfg(feature = "...")]` gates that only turn on test or dev helpers.
pub fn is_testing_feature_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        let name = syntax::attr_name(a);
        if name != "cfg" && name != "cfg_attr" {
            return false;
        }
        let args = syntax::attr_args(a);
        let gate = if name == "cfg_attr" {
            args.split(',').next().unwrap_or("").to_string()
        } else {
            args
        };
        gate.contains("feature")
            && [
                "\"testing\"",
                "\"test-utils\"",
                "\"test_utils\"",
                "\"testing-utils\"",
            ]
            .iter()
            .any(|f| gate.contains(f))
    })
}

/// What kind of exported thing a [`Slot`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A Record field.
    Field,
    /// A field of an enum (or error) variant.
    VariantField,
    /// An argument of an exported fn, method or constructor.
    Arg,
    /// The return type of an exported fn or method.
    Return,
}

/// One typed name that crosses the FFI: a Record field, a variant field, an
/// argument or a return type.
#[derive(Clone, Debug)]
pub struct Slot {
    pub kind: SlotKind,
    pub path: String,
    pub line: usize,
    /// The type or function it belongs to (`Suggestion::Yelp`, `Store::get`).
    pub owner: String,
    /// The field or argument name (`""` for a return type).
    pub name: String,
    /// The type as Rust text (`Option<String>`) or as UDL (`string?`).
    pub ty: String,
    pub from_udl: bool,
    pub doc: Option<String>,
}

impl Slot {
    /// The type with `Option<..>`/`?`, `&` and `Vec<..>`/`sequence<..>`
    /// stripped, in a common vocabulary: `String`, `i64`, `Url`, `Value`...
    pub fn base_type(&self) -> String {
        if self.from_udl {
            udl_base_type(&self.ty)
        } else {
            rust_base_type(&self.ty)
        }
    }

    pub fn is_optional(&self) -> bool {
        if self.from_udl {
            self.ty.trim_end().ends_with('?')
        } else {
            self.ty.starts_with("Option<")
        }
    }
}

/// `Option<Vec<&str>>` -> `str`, `Option<i64>` -> `i64`.
pub fn rust_base_type(ty: &str) -> String {
    let mut t = ty.trim().trim_start_matches('&').trim();
    loop {
        let next = ["Option<", "Vec<", "Box<", "Arc<"]
            .iter()
            .find_map(|w| t.strip_prefix(w).and_then(|r| r.strip_suffix('>')));
        match next {
            Some(inner) => t = inner.trim().trim_start_matches('&').trim(),
            None => break,
        }
    }
    let last = t.rsplit("::").next().unwrap_or(t);
    match last {
        "str" => "String".to_string(),
        other => other.to_string(),
    }
}

/// UDL `string?` -> `String`, `sequence<i64>` -> `i64`, `[ByRef] string` -> `String`.
pub fn udl_base_type(ty: &str) -> String {
    let mut t = ty.trim();
    if let Some(close) = t.strip_prefix('[').and_then(|r| r.find(']')) {
        t = t[close + 2..].trim();
    }
    t = t.trim_end_matches('?');
    if let Some(inner) = t
        .strip_prefix("sequence<")
        .and_then(|r| r.strip_suffix('>'))
    {
        t = inner.trim().trim_end_matches('?');
    }
    match t {
        "string" => "String".to_string(),
        "boolean" => "bool".to_string(),
        other => other.to_string(),
    }
}

/// Every [`Slot`] of a Rust file's proc-macro exports (test code excluded).
pub fn rust_slots(file: &RustFile) -> Vec<Slot> {
    let mut out = Vec::new();
    for item in syntax::all_items(&file.ast) {
        match item {
            syn::Item::Struct(s) if syntax::has_derive(&s.attrs, "uniffi::Record") => {
                if file.in_test(file.line_of_node(s)) {
                    continue;
                }
                for f in &s.fields {
                    let Some(ident) = &f.ident else { continue };
                    out.push(Slot {
                        kind: SlotKind::Field,
                        path: file.path.clone(),
                        line: ident.span().start().line,
                        owner: s.ident.to_string(),
                        name: unraw(ident),
                        ty: syntax::type_string(&f.ty),
                        from_udl: false,
                        doc: syntax::doc(&f.attrs),
                    });
                }
            }
            syn::Item::Enum(e)
                if syntax::has_derive(&e.attrs, "uniffi::Enum")
                    || syntax::has_derive(&e.attrs, "uniffi::Error") =>
            {
                if file.in_test(file.line_of_node(e)) {
                    continue;
                }
                for v in &e.variants {
                    for f in &v.fields {
                        let Some(ident) = &f.ident else { continue };
                        out.push(Slot {
                            kind: SlotKind::VariantField,
                            path: file.path.clone(),
                            line: ident.span().start().line,
                            owner: format!("{}::{}", e.ident, v.ident),
                            name: unraw(ident),
                            ty: syntax::type_string(&f.ty),
                            from_udl: false,
                            doc: syntax::doc(&f.attrs),
                        });
                    }
                }
            }
            syn::Item::Fn(f) if syntax::has_attr(&f.attrs, "uniffi::export") => {
                if !file.in_test(file.line_of_node(&f.sig)) {
                    sig_slots(file, &f.sig.ident.to_string(), &f.sig, &f.attrs, &mut out);
                }
            }
            syn::Item::Impl(i) if syntax::has_attr(&i.attrs, "uniffi::export") => {
                if file.in_test(file.line_of_node(&i.self_ty)) {
                    continue;
                }
                let ty = syntax::type_ident(&i.self_ty).unwrap_or_default();
                for it in &i.items {
                    let syn::ImplItem::Fn(m) = it else { continue };
                    let public = matches!(m.vis, syn::Visibility::Public(_)) || i.trait_.is_some();
                    if public && !syntax::is_test_gated(&m.attrs) {
                        let owner = format!("{ty}::{}", m.sig.ident);
                        sig_slots(file, &owner, &m.sig, &m.attrs, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn sig_slots(
    file: &RustFile,
    owner: &str,
    sig: &syn::Signature,
    attrs: &[syn::Attribute],
    out: &mut Vec<Slot>,
) {
    let doc = syntax::doc(attrs);
    for input in &sig.inputs {
        let syn::FnArg::Typed(t) = input else {
            continue;
        };
        let syn::Pat::Ident(p) = &*t.pat else {
            continue;
        };
        out.push(Slot {
            kind: SlotKind::Arg,
            path: file.path.clone(),
            line: p.ident.span().start().line,
            owner: owner.to_string(),
            name: unraw(&p.ident),
            ty: syntax::type_string(&t.ty),
            from_udl: false,
            doc: doc.clone(),
        });
    }
    if let syn::ReturnType::Type(_, ty) = &sig.output {
        out.push(Slot {
            kind: SlotKind::Return,
            path: file.path.clone(),
            line: ty.span().start().line,
            owner: owner.to_string(),
            name: String::new(),
            ty: syntax::type_string(ty),
            from_udl: false,
            doc,
        });
    }
}

fn unraw(ident: &syn::Ident) -> String {
    ident.to_string().trim_start_matches("r#").to_string()
}

/// Every [`Slot`] declared in a UDL file.
pub fn udl_slots(path: &str, text: &str) -> Vec<Slot> {
    use crate::udl::UdlKind;
    let mut out = Vec::new();
    let docs = udl_docs(text);
    for item in crate::udl::parse(text) {
        for member in &item.members {
            let doc = docs.get(&member.line).cloned();
            let mut m = member.clone();
            m.line = udl_name_line(text, member.line, &member.name);
            match item.kind {
                UdlKind::Dictionary => {
                    if let Some(ty) = m.field_type() {
                        out.push(Slot {
                            kind: SlotKind::Field,
                            path: path.to_string(),
                            line: m.line,
                            owner: item.name.clone(),
                            name: m.name.clone(),
                            ty: ty.to_string(),
                            from_udl: true,
                            doc,
                        });
                    }
                }
                UdlKind::Namespace | UdlKind::Interface | UdlKind::CallbackInterface => {
                    let Some((ret, name, args)) = udl_signature(&m.signature) else {
                        continue;
                    };
                    let owner = if item.kind == UdlKind::Namespace {
                        name.clone()
                    } else {
                        format!("{}::{name}", item.name)
                    };
                    let is_error = item.has_attr("Error") || item.has_attr("Enum");
                    for (ty, arg) in args {
                        out.push(Slot {
                            kind: if is_error {
                                SlotKind::VariantField
                            } else {
                                SlotKind::Arg
                            },
                            path: path.to_string(),
                            line: m.line,
                            owner: owner.clone(),
                            name: arg,
                            ty,
                            from_udl: true,
                            doc: doc.clone(),
                        });
                    }
                    if !ret.is_empty() && ret != "void" && name != "constructor" && !is_error {
                        out.push(Slot {
                            kind: SlotKind::Return,
                            path: path.to_string(),
                            line: m.line,
                            owner,
                            name: String::new(),
                            ty: ret,
                            from_udl: true,
                            doc,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// A UDL signature: return type, name, and `(type, name)` arguments.
pub type UdlSignature = (String, String, Vec<(String, String)>);

/// `[Throws=E] sequence<Foo>? get_all(string a, optional i64 b = 1)` ->
/// `("sequence<Foo>?", "get_all", [("string", "a"), ("i64", "b")])`.
pub fn udl_signature(sig: &str) -> Option<UdlSignature> {
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    let head = sig[..open].trim();
    let (ret, name) = match head.rsplit_once(' ') {
        Some((r, n)) => (r.trim().to_string(), n.trim().to_string()),
        None => (String::new(), head.to_string()),
    };
    let mut args = Vec::new();
    for raw in split_top_commas(&sig[open + 1..close]) {
        let mut a = raw.trim();
        while let Some(rest) = a.strip_prefix('[') {
            a = rest.split_once(']').map_or("", |(_, r)| r).trim();
        }
        let a = a.trim_start_matches("optional ").trim();
        let decl = a.split('=').next().unwrap_or(a).trim();
        if let Some((ty, n)) = decl.rsplit_once(' ') {
            args.push((ty.trim().to_string(), n.trim().to_string()));
        }
    }
    Some((ret, name, args))
}

fn split_top_commas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(&text[start..]);
    }
    out
}

/// The line, at or after `from`, where `name` appears as a word: UDL member
/// lines point at their first attribute, this points at the name.
pub fn udl_name_line(text: &str, from: usize, name: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    (from..from + 6)
        .find(|&l| {
            lines
                .get(l.wrapping_sub(1))
                .is_some_and(|t| has_word(t, name))
        })
        .unwrap_or(from)
}

/// Whether `word` occurs in `text` with no identifier character around it.
pub fn has_word(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(ident)
            && !text[i + word.len()..].chars().next().is_some_and(ident)
    })
}

/// For each line of a UDL file, the `///` doc block right above it (skipping
/// attribute-only lines).
pub fn udl_docs(text: &str) -> std::collections::HashMap<usize, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = std::collections::HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let mut doc = Vec::new();
        let mut k = index;
        while k > 0 {
            k -= 1;
            let above = lines[k].trim();
            if let Some(d) = above.strip_prefix("///") {
                doc.push(d.trim().to_string());
            } else if above.starts_with('[') && above.ends_with(']') && doc.is_empty() {
                continue;
            } else {
                break;
            }
        }
        if !doc.is_empty() {
            doc.reverse();
            out.insert(index + 1, doc.join("\n"));
        }
    }
    out
}

/// The exported surface of the workspace, as slots: every UDL file and every
/// production Rust file of a crate that depends on `uniffi`.
pub fn all_slots(ws: &Workspace) -> Vec<Slot> {
    let mut out = Vec::new();
    for file in ws.files() {
        if !is_uniffi_crate_file(ws, &file.path) {
            continue;
        }
        if file.extension() == "udl" && !is_test_path(&file.path) {
            out.extend(udl_slots(&file.path, &file.text));
        } else if is_production_rust(&file.path) {
            if let Some(rust) = ws.rust(&file.path) {
                out.extend(rust_slots(rust));
            }
        }
    }
    out
}

/// Whether `path` belongs to a crate that depends on `uniffi`, outside
/// vendored and generated code.
pub fn is_uniffi_crate_file(ws: &Workspace, path: &str) -> bool {
    !is_generated_or_vendored(path)
        && ws
            .package_of(path)
            .is_some_and(|p| p.dependencies().iter().any(|d| d.name == "uniffi"))
}

/// Split an identifier into lowercase words: `subjectType` and
/// `subject_type` -> `["subject", "type"]`, `JSONValue` -> `["json", "value"]`.
pub fn words(ident: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = ident.chars().collect();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        let boundary = c.is_uppercase()
            && !current.is_empty()
            && (prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit())
                || next.is_some_and(|n| n.is_lowercase()) && prev.is_some_and(char::is_uppercase));
        if boundary {
            out.push(std::mem::take(&mut current));
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Names of crates (with `_`) that some megazord links: their API ships.
pub fn megazord_crates(ws: &Workspace) -> BTreeSet<String> {
    ws.ios_megazord_crates()
        .into_iter()
        .chain(ws.android_megazord_crates())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_kotlin() {
        let text = "val a = \"x!!\" // b!!\nval c = d!! /* e!! */\nval f = '\"'!!\n";
        let code = Code::scan(text, Lang::Kotlin, false);
        let bangs: Vec<usize> = code
            .code
            .match_indices("!!")
            .map(|(i, _)| code.line_of(i))
            .collect();
        assert_eq!(bangs, vec![2, 3]);
        assert_eq!(code.comments.len(), 2);
        assert_eq!(code.comments[0].text, "// b!!");
    }

    #[test]
    fn scans_interpolation_and_templates() {
        let text = "let s = \"\\(a!) \\(\"q\")\"\n{% if !x %}public class {{ name }} {}\n";
        let code = Code::scan(text, Lang::Swift, true);
        assert!(!code.code.contains('!'));
        assert!(code.code.contains("public class xxxxxxxxxx {}"));
    }

    #[test]
    fn splits_words() {
        assert_eq!(words("subjectType"), vec!["subject", "type"]);
        assert_eq!(words("JSONValue"), vec!["json", "value"]);
        assert_eq!(words("mime_type"), vec!["mime", "type"]);
        assert_eq!(words("RustBuffer2"), vec!["rust", "buffer2"]);
    }

    #[test]
    fn udl_signatures() {
        let (ret, name, args) =
            udl_signature("sequence<Foo>? get_all([ByRef] string a, optional i64 b = 1)").unwrap();
        assert_eq!(ret, "sequence<Foo>?");
        assert_eq!(name, "get_all");
        assert_eq!(
            args,
            vec![
                ("string".to_string(), "a".to_string()),
                ("i64".to_string(), "b".to_string())
            ]
        );
        assert_eq!(udl_base_type("sequence<string>?"), "String");
        assert_eq!(rust_base_type("Option<Vec<&str>>"), "String");
    }
}
