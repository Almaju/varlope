//! Doc comments reference things that exist (intra-doc links, stale names)
//!
//! Docs and examples: `rules/docs-in-sync/`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use syn::visit::Visit;

use crate::helpers::docs_deps::{comment_lines, is_out_of_scope, text_of};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::workspace::{is_test_path, Package, Workspace};

pub static RULE: Rule = Rule {
    id: "docs-in-sync",
    area: Area::Docs,
    title: "Doc comments reference things that exist (intra-doc links, repo paths)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// `[`Self::wipe`]`, `[crate::db::Store]`, `[text](Self::wipe)`.
static INTRA_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\[`?|\]\(|\]\[)((?:Self|crate|super)(?:::[A-Za-z_][\w]*)+)!?(?:\(\))?`?[\])]")
        .expect("valid regex")
});

/// A repo-relative path: `components/places/src/db.rs`.
static REPO_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[\s`'(\[<])((?:components|megazords|docs|tools|automation|taskcluster|examples|testing|libs|build-scripts)/[\w./-]*[\w/])",
    )
    .expect("valid regex")
});

/// `](relative/path.md)` in Markdown.
static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\]\(([^)\s]+)\)").expect("valid regex"));

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        intra_doc_links(cx, package, out);
    }
    let ignored = Gitignore::load(cx.ws);
    repo_paths(cx, &ignored, out);
    markdown_links(cx, &ignored, out);
}

/// Names a crate defines or imports, and the associated items of each type.
#[derive(Default)]
struct Names {
    all: BTreeSet<String>,
    /// type (or trait) name -> its methods, consts, types, fields, variants
    members: BTreeMap<String, BTreeSet<String>>,
}

impl<'ast> Visit<'ast> for Names {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let ident = match item {
            syn::Item::Fn(i) => Some(&i.sig.ident),
            syn::Item::Struct(i) => {
                let members = self.members.entry(i.ident.to_string()).or_default();
                members.extend(
                    i.fields
                        .iter()
                        .filter_map(|f| f.ident.as_ref().map(|x| x.to_string())),
                );
                Some(&i.ident)
            }
            syn::Item::Enum(i) => {
                let members = self.members.entry(i.ident.to_string()).or_default();
                members.extend(i.variants.iter().map(|v| v.ident.to_string()));
                Some(&i.ident)
            }
            syn::Item::Trait(i) => {
                let members = self.members.entry(i.ident.to_string()).or_default();
                for t in &i.items {
                    match t {
                        syn::TraitItem::Fn(f) => members.insert(f.sig.ident.to_string()),
                        syn::TraitItem::Const(c) => members.insert(c.ident.to_string()),
                        syn::TraitItem::Type(t) => members.insert(t.ident.to_string()),
                        _ => false,
                    };
                }
                Some(&i.ident)
            }
            syn::Item::Type(i) => Some(&i.ident),
            syn::Item::Const(i) => Some(&i.ident),
            syn::Item::Static(i) => Some(&i.ident),
            syn::Item::Mod(i) => Some(&i.ident),
            syn::Item::Union(i) => Some(&i.ident),
            syn::Item::Macro(i) => i.ident.as_ref(),
            syn::Item::Impl(i) => {
                if let Some(ty) = syntax::type_ident(&i.self_ty) {
                    let members = self.members.entry(ty).or_default();
                    for it in &i.items {
                        match it {
                            syn::ImplItem::Fn(f) => members.insert(f.sig.ident.to_string()),
                            syn::ImplItem::Const(c) => members.insert(c.ident.to_string()),
                            syn::ImplItem::Type(t) => members.insert(t.ident.to_string()),
                            _ => false,
                        };
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(ident) = ident {
            self.all.insert(ident.to_string());
        }
        syn::visit::visit_item(self, item);
    }
    fn visit_use_name(&mut self, name: &'ast syn::UseName) {
        self.all.insert(name.ident.to_string());
    }
    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        self.all.insert(rename.rename.to_string());
    }
    fn visit_use_glob(&mut self, _: &'ast syn::UseGlob) {
        // A glob import can bring in anything: names can't be checked.
        self.all.insert("*".into());
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.all.insert(f.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, f);
    }
    fn visit_field(&mut self, f: &'ast syn::Field) {
        if let Some(i) = &f.ident {
            self.all.insert(i.to_string());
        }
        syn::visit::visit_field(self, f);
    }
    fn visit_variant(&mut self, v: &'ast syn::Variant) {
        self.all.insert(v.ident.to_string());
        syn::visit::visit_variant(self, v);
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.all.insert(f.sig.ident.to_string());
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// Intra-doc links to `Self::x`, `crate::…::x` and `super::…::x` that name
/// nothing in the crate: rustdoc's `broken_intra_doc_links`, which CI does
/// not run.
fn intra_doc_links(cx: &Context, package: &Package, out: &mut Findings) {
    let files: Vec<&RustFile> = cx
        .ws
        .package_files(package)
        .filter_map(|f| cx.ws.rust(&f.path))
        .filter(|f| !is_out_of_scope(&f.path))
        .collect();
    let mut names = Names::default();
    for file in &files {
        names.visit_file(&file.ast);
    }
    for file in &files {
        let mut docs = Docs {
            file,
            self_type: None,
            hits: Vec::new(),
        };
        docs.visit_file(&file.ast);
        for (line, self_type, link) in docs.hits {
            let mut segments: Vec<&str> = link.split("::").collect();
            let last = segments.pop().unwrap_or("");
            let first = segments.first().copied().unwrap_or("");
            let known = match (first, &self_type) {
                ("Self", Some(ty)) if segments.len() == 1 => {
                    names.members.get(ty).is_some_and(|m| m.contains(last))
                }
                ("Self", _) => true,
                _ => names.all.contains("*") || names.all.contains(last),
            };
            if !known {
                out.add(
                    &file.path,
                    line,
                    format!("doc link `{link}` points at nothing in this crate (renamed or removed?): fix the link"),
                );
            }
        }
    }
}

/// Doc comments with their line and the `impl`/trait they sit in.
struct Docs<'f> {
    file: &'f RustFile,
    self_type: Option<String>,
    hits: Vec<(usize, Option<String>, String)>,
}

impl Docs<'_> {
    fn attrs(&mut self, attrs: &[syn::Attribute]) {
        for attr in attrs.iter().filter(|a| a.path().is_ident("doc")) {
            let syn::Meta::NameValue(nv) = &attr.meta else {
                continue;
            };
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            else {
                continue;
            };
            let line = self.file.line_of_node(attr);
            if self.file.in_test(line) {
                continue;
            }
            for link in INTRA_LINK.captures_iter(&s.value()) {
                self.hits
                    .push((line, self.self_type.clone(), link[1].to_string()));
            }
        }
    }
}

impl<'ast> Visit<'ast> for Docs<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let (attrs, scope): (&[syn::Attribute], Option<String>) = match item {
            syn::Item::Impl(i) => (&i.attrs, syntax::type_ident(&i.self_ty)),
            syn::Item::Trait(i) => (&i.attrs, Some(i.ident.to_string())),
            syn::Item::Struct(i) => (&i.attrs, Some(i.ident.to_string())),
            syn::Item::Enum(i) => (&i.attrs, Some(i.ident.to_string())),
            syn::Item::Fn(i) => (&i.attrs, None),
            syn::Item::Mod(i) => (&i.attrs, None),
            syn::Item::Const(i) => (&i.attrs, None),
            syn::Item::Static(i) => (&i.attrs, None),
            syn::Item::Type(i) => (&i.attrs, None),
            _ => (&[], None),
        };
        let outer = std::mem::replace(&mut self.self_type, scope.clone());
        self.attrs(attrs);
        syn::visit::visit_item(self, item);
        self.self_type = outer;
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.attrs(&f.attrs);
        syn::visit::visit_impl_item_fn(self, f);
    }
    fn visit_impl_item_const(&mut self, c: &'ast syn::ImplItemConst) {
        self.attrs(&c.attrs);
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.attrs(&f.attrs);
        syn::visit::visit_trait_item_fn(self, f);
    }
    fn visit_field(&mut self, f: &'ast syn::Field) {
        self.attrs(&f.attrs);
    }
    fn visit_variant(&mut self, v: &'ast syn::Variant) {
        self.attrs(&v.attrs);
        syn::visit::visit_variant(self, v);
    }
}

/// Simple `.gitignore` matching, enough to skip build outputs.
struct Gitignore {
    patterns: Vec<String>,
}

impl Gitignore {
    fn load(ws: &Workspace) -> Self {
        let patterns = text_of(ws, ".gitignore")
            .unwrap_or_default()
            .lines()
            .map(|l| l.split(" #").next().unwrap_or("").trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
            .collect();
        Self { patterns }
    }

    fn ignores(&self, path: &str) -> bool {
        let segments: Vec<&str> = path.split('/').collect();
        self.patterns.iter().any(|p| {
            let p = p.trim_matches('/');
            if !p.contains('/') {
                return segments.iter().any(|s| glob(p, s));
            }
            let parts: Vec<&str> = p.split('/').collect();
            matches_from(&parts, &segments)
        })
    }
}

/// Whether the pattern segments match a prefix of the path segments.
fn matches_from(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.first() {
        None => true,
        Some(&"**") => (0..=path.len()).any(|skip| matches_from(&pattern[1..], &path[skip..])),
        Some(p) => path
            .first()
            .is_some_and(|s| glob(p, s) && matches_from(&pattern[1..], &path[1..])),
    }
}

/// `*` within one segment. Case-insensitive: a doc that writes `sources/`
/// for `Sources/` still means the ignored directory.
fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern.eq_ignore_ascii_case(text),
        Some((head, tail)) => {
            text.len() >= head.len()
                && text.is_char_boundary(head.len())
                && text[..head.len()].eq_ignore_ascii_case(head)
                && (0..=text.len() - head.len()).any(|i| {
                    text.is_char_boundary(head.len() + i) && glob(tail, &text[head.len() + i..])
                })
        }
    }
}

/// Whether a repo path exists: a file, a directory of files, or something
/// on disk the workspace doesn't load (`.c`, `.png`).
fn exists(cx: &Context, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    cx.ws.file(path).is_some()
        || cx
            .ws
            .files()
            .any(|f| f.path.starts_with(&format!("{path}/")))
        || cx.ws.root.join(path).exists()
}

/// Placeholder names in examples (`components/mycrate`), and build outputs
/// that only exist after a build (`libs/android`, `*.xcframework.zip`).
fn is_placeholder_or_output(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    let placeholder = segments.iter().any(|s| {
        [
            "my", "your", "foo", "bar", "some", "new-", "new_", "example-",
        ]
        .iter()
        .any(|p| s.starts_with(p))
    });
    let last = segments.last().copied().unwrap_or("");
    let artifact = [
        ".xcframework",
        ".zip",
        ".tar.gz",
        ".tar.xz",
        ".aar",
        ".so",
        ".a",
        ".dylib",
    ]
    .iter()
    .any(|e| last.ends_with(e));
    let libs_output = segments.len() >= 2 && segments[0] == "libs" && !segments[1].contains('.');
    placeholder || artifact || libs_output
}

/// Repo paths named in comments (and in Markdown prose) that don't exist.
fn repo_paths(cx: &Context, ignored: &Gitignore, out: &mut Findings) {
    for file in cx.ws.files() {
        let p = file.path.as_str();
        if is_out_of_scope(p) || skip_doc(p) || is_test_path(p) {
            continue;
        }
        let texts: Vec<(usize, String)> = if file.extension() == "md" {
            // Link targets are checked by `markdown_links`.
            prose_lines(&file.text)
                .into_iter()
                .map(|(n, l)| (n, MD_LINK.replace_all(&l, "]()").into_owned()))
                .collect()
        } else {
            comment_lines(p, &file.text).into_iter().collect()
        };
        for (line, text) in texts {
            for m in REPO_PATH.captures_iter(&text) {
                let found = m.get(1).expect("group 1");
                let after = text[found.end()..].chars().next();
                if matches!(after, Some('*' | '{' | '<' | '$' | '-')) {
                    continue; // a glob or a template
                }
                let path = found.as_str().trim_end_matches(['.', '/']);
                if is_placeholder_or_output(path) || ignored.ignores(path) || exists(cx, path) {
                    continue;
                }
                out.add(
                    p,
                    line,
                    format!("`{path}` does not exist (moved or renamed?): update the reference"),
                );
            }
        }
    }
}

/// Relative Markdown links to files that don't exist.
fn markdown_links(cx: &Context, ignored: &Gitignore, out: &mut Findings) {
    for file in cx.ws.files_with_ext(&["md"]) {
        let p = file.path.as_str();
        if is_out_of_scope(p) || skip_doc(p) || is_test_path(p) || p == "docs/SUMMARY.md" {
            continue;
        }
        let dir = p.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for (line, text) in prose_lines(&file.text) {
            for m in MD_LINK.captures_iter(&text) {
                let target = m[1].split('#').next().unwrap_or("");
                let target = target.trim_matches(['<', '>']);
                if target.is_empty()
                    || target.contains("://")
                    || target.starts_with("mailto:")
                    || target.starts_with('/')
                    || target.ends_with(".html")
                    || target.contains("rust-docs")
                {
                    continue;
                }
                let resolved = normalize(&format!("{dir}/{target}"));
                if resolved.starts_with("..")
                    || is_placeholder_or_output(&resolved)
                    || ignored.ignores(&resolved)
                    || exists(cx, &resolved)
                {
                    continue;
                }
                out.add(
                    p,
                    line,
                    format!("link target `{target}` does not exist: fix the link"),
                );
            }
        }
    }
}

/// History that is meant to stay as written (the CHANGELOG, the decision
/// records but not their index), and templates full of placeholders.
fn skip_doc(path: &str) -> bool {
    let adr_record = path
        .strip_prefix("docs/adr/")
        .is_some_and(|name| name.starts_with(|c: char| c.is_ascii_digit()));
    let template = path
        .rsplit('/')
        .next()
        .is_some_and(|n| n.starts_with("template"));
    path == "CHANGELOG.md" || adr_record || template || path.ends_with("DEPENDENCIES.md")
}

/// Markdown lines outside fenced code blocks.
fn prose_lines(text: &str) -> Vec<(usize, String)> {
    let mut fenced = false;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push((i + 1, line.to_string()));
        }
    }
    out
}

/// `a/b/../c` → `a/c`; a leading `..` past the root is kept.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/logins/Cargo.toml",
        "[package]\nname = \"logins\"\n",
    );
    const PATH: &str = "components/logins/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (PATH, include_str!("../../rules/docs-in-sync/dont.rs")),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (PATH, include_str!("../../rules/docs-in-sync/do.rs")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn crate_links_and_fields() {
        let text = "/// See [`crate::db::LoginDb`] and [`crate::Gone`].\npub struct Store { db: u8 }\nimpl Store {\n    /// Uses [`Self::db`] and [Self::open].\n    pub fn open() {}\n}\nmod db { pub struct LoginDb; }\n";
        let found = check(&RULE, &[MANIFEST, (PATH, text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("crate::Gone"));
    }

    #[test]
    fn repo_paths_in_comments_and_markdown() {
        let files = [
            MANIFEST,
            (PATH, "// See components/logins/src/db.rs and components/logins/src/store.rs.\n// Run libs/verify-*-environment.sh first.\nfn a() { let p = \"components/gone/x.rs\"; }\n"),
            ("docs/howto.md", "Edit [the store](../components/logins/src/store.rs) or [old](../components/logins/src/old.rs).\nWork in `components/mycrate`.\n```\ncd components/nope\n```\n"),
            (".gitignore", "build\ncomponents/**/ios/Generated\n"),
            ("README.md", "Bindings land in components/logins/ios/Generated and components/logins/build/out.\n"),
        ];
        let found = check(&RULE, &files);
        let messages: Vec<(String, usize)> =
            found.iter().map(|f| (f.path.clone(), f.line)).collect();
        assert_eq!(found.len(), 2, "{messages:#?}");
        assert_eq!(messages[0], (PATH.to_string(), 1));
        assert_eq!(messages[1], ("docs/howto.md".to_string(), 1));
    }
}
