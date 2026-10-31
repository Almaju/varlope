//! Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`
//!
//! Docs and examples: `rules/borrowed-params/`.

use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::errors;
use crate::helpers::idioms::{self, functions};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::uniffi::{self, ApiKind};
use crate::workspace::Workspace;

pub static RULE: Rule = Rule {
    id: "borrowed-params",
    area: Area::Idioms,
    title: "Take `&str`/`&[T]`/`Option<&T>` instead of `String`/`&Vec<T>`/`&Option<T>`",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::ptr_arg",
        "clippy::ref_option",
        "clippy::needless_pass_by_value",
        "clippy::trivially_copy_pass_by_ref",
    ],
    in_tests: false,
    check,
};

/// Clippy's `ptr_arg` and `ref_option` are the lints for this; the
/// parameter types they flag can be read off the signature, so varlope
/// reports them without a build (`needless_pass_by_value` and
/// `trivially_copy_pass_by_ref` need types and stay with clippy).
fn check(cx: &Context, out: &mut Findings) {
    let exported = exported_signatures(cx);
    for file in idioms::rust_files(cx.ws) {
        // Functions named in strings (`#[serde(skip_serializing_if = "is_none")]`)
        // have a signature someone else chose.
        let named_in_strings: BTreeSet<String> = syntax::string_literals(&file.ast)
            .into_iter()
            .filter_map(|(_, s)| s.rsplit("::").next().map(str::to_string))
            .collect();
        for f in functions(&file.ast) {
            let trait_impl = f.trait_name.is_some() && f.self_ty.is_some();
            // UniFFI decides these signatures (`[ByRef] string?` is `&Option<String>`).
            let exported = syntax::has_attr(f.attrs, "export")
                || f.outer_attrs.iter().any(|a| syntax::has_attr(a, "export"))
                || exported.contains(&(f.self_ty.clone().unwrap_or_default(), f.name.clone()));
            if trait_impl || exported || f.is_test() || named_in_strings.contains(&f.name) {
                continue;
            }
            for input in &f.sig.inputs {
                let syn::FnArg::Typed(arg) = input else {
                    continue;
                };
                let Some(better) = borrowed_form(&arg.ty) else {
                    continue;
                };
                let (line, column) = file.pos(&arg.ty);
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!(
                        "parameter typed `{}`; take `{better}` so callers can pass any borrow without building one",
                        syntax::type_string(&arg.ty)
                    ),
                );
                if let (Some(fix), Some(last)) = (fix(cx.ws, file, &f, arg), out.items.last_mut()) {
                    last.fix = Some(fix);
                }
            }
        }
    }
}

/// Methods the borrowed type lacks, or that return another type on it
/// (`clone` on a `&str` is a `&str`, not a `String`).
const CHANGES_MEANING: &[&str] = &[
    "clone", "as_path", "as_str", "as_slice", "as_ref", "capacity", "into",
];

/// Macros that only format their arguments.
const FORMATTING: &[&str] = &[
    "format",
    "write",
    "writeln",
    "print",
    "println",
    "eprint",
    "eprintln",
    "panic",
    "bail",
    "format_err",
    "anyhow",
    "trace",
    "debug",
    "info",
    "warn",
    "error",
];

/// The signature with `&String` -> `&str`, `&Vec<T>` -> `&[T]` or `&PathBuf`
/// -> `&Path`, when nothing else has to change: the body only calls methods
/// both types have on it, loops over it, compares or formats it (callers'
/// `&String` coerces), and the function is only ever called, never passed as
/// a value. `&Option<T>` -> `Option<&T>` changes every caller, so it gets no
/// fix.
fn fix(ws: &Workspace, file: &RustFile, f: &idioms::FnInfo, arg: &syn::PatType) -> Option<Fix> {
    if f.trait_name.is_some() {
        return None; // a trait's default method: implementors repeat the signature
    }
    let syn::Pat::Ident(pat) = &*arg.pat else {
        return None;
    };
    if pat.by_ref.is_some() || pat.mutability.is_some() || pat.subpat.is_some() {
        return None;
    }
    let syn::Type::Reference(r) = &*arg.ty else {
        return None;
    };
    let name = syntax::type_ident(&r.elem)?;
    let args = syntax::type_args(&r.elem);
    let lifetime = r
        .lifetime
        .as_ref()
        .map_or(String::new(), |l| format!("{l} "));
    let better = match (name.as_str(), args.as_slice()) {
        ("String", []) => format!("&{lifetime}str"),
        ("Vec", [t]) => format!("&{lifetime}[{}]", file.text_of(*t)),
        ("PathBuf", []) if imports_path(file) => format!("&{lifetime}Path"),
        ("PathBuf", []) => format!("&{lifetime}std::path::Path"),
        _ => return None,
    };
    let mut uses = Uses {
        param: pat.ident.to_string(),
        other: false,
    };
    uses.visit_block(f.block);
    if uses.other || used_as_value(ws, &file.path, &f.name, f.is_pub) {
        return None;
    }
    let (start, end) = (arg.ty.span().start(), arg.ty.span().end());
    if start.line != end.line {
        return None;
    }
    let text = file.line(start.line);
    let range = file.range(arg.ty.span());
    let line_start = file.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let (from, to) = (range.start - line_start, range.end - line_start);
    let line = format!("{}{better}{}", text.get(..from)?, text.get(to..)?);
    // A signature over several lines that now fits on one: rustfmt joins it.
    let (first, open) = (
        f.sig.span().start().line,
        f.block.brace_token.span.open().start().line,
    );
    if first < start.line && start.line < open {
        let lines: Vec<String> = (first..=open)
            .map(|n| match n == start.line {
                true => line.clone(),
                false => file.line(n).to_string(),
            })
            .collect();
        let (head, params, tail) = (
            &lines[0],
            &lines[1..lines.len() - 1],
            &lines[lines.len() - 1],
        );
        // Comments keep the parameters one per line.
        if !params.iter().any(|p| p.contains("//")) {
            let tall = head.trim_end().ends_with('(')
                && tail.trim_start().starts_with(')')
                && params.iter().all(|p| p.trim_end().ends_with(','));
            if !tall {
                return None; // a layout this doesn't know how rustfmt redoes
            }
            let params: Vec<&str> = params
                .iter()
                .map(|p| p.trim().trim_end_matches(','))
                .collect();
            let joined = format!(
                "{}{}{}",
                head.trim_end(),
                params.join(", "),
                tail.trim_start()
            );
            if joined.chars().count() <= 100 {
                return Some(Fix::edit(first, open - first + 1, vec![joined]));
            }
        }
    }
    (line.chars().count() <= 100).then(|| Fix::replace_line(start.line, line))
}

/// Whether the file imports `std::path::Path`.
fn imports_path(file: &RustFile) -> bool {
    fn walk(tree: &syn::UseTree, prefix: &mut Vec<String>) -> bool {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                let found = walk(&p.tree, prefix);
                prefix.pop();
                found
            }
            syn::UseTree::Group(g) => g.items.iter().any(|t| walk(t, prefix)),
            syn::UseTree::Name(n) => n.ident == "Path" && prefix.as_slice() == ["std", "path"],
            _ => false,
        }
    }
    file.ast
        .items
        .iter()
        .any(|i| matches!(i, syn::Item::Use(u) if walk(&u.tree, &mut Vec::new())))
}

/// Whether a function named `name` is used other than by calling it
/// (`.map(name)`), so its type must stay as it is: in the package of `path`,
/// or anywhere for a `pub` one.
fn used_as_value(ws: &Workspace, path: &str, name: &str, public: bool) -> bool {
    struct V<'n> {
        name: &'n str,
        called: BTreeSet<(usize, usize)>,
        named: Vec<(usize, usize)>,
    }
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*c.func {
                let start = p.span().start();
                self.called.insert((start.line, start.column));
            }
            syn::visit::visit_expr_call(self, c);
        }
        fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
            if p.path.segments.last().is_some_and(|s| s.ident == self.name) {
                let start = p.span().start();
                self.named.push((start.line, start.column));
            }
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            if idioms::count_ident(&m.tokens, self.name) > 0 {
                self.named.push((0, 0)); // can't tell inside a macro
            }
        }
    }
    let package = ws.package_of(path).filter(|_| !public);
    ws.rust_files()
        .filter(|file| {
            package.is_none_or(|p| ws.package_of(&file.path).is_some_and(|q| q.dir == p.dir))
        })
        .any(|file| {
            let mut v = V {
                name,
                called: BTreeSet::new(),
                named: Vec::new(),
            };
            v.visit_file(&file.ast);
            v.named.iter().any(|n| !v.called.contains(n))
        })
}

/// How a function body uses a parameter: `other` is any use the borrowed
/// type might not serve.
struct Uses {
    param: String,
    other: bool,
}

impl Uses {
    fn is_param(&self, e: &syn::Expr) -> bool {
        let mut e = e;
        while let syn::Expr::Paren(p) = e {
            e = &p.expr;
        }
        matches!(e, syn::Expr::Path(p) if p.path.is_ident(&self.param))
    }
}

impl<'ast> Visit<'ast> for Uses {
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if self.is_param(&m.receiver) {
            if CHANGES_MEANING.contains(&m.method.to_string().as_str()) {
                self.other = true;
            }
            for arg in &m.args {
                self.visit_expr(arg);
            }
            return;
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_for_loop(&mut self, f: &'ast syn::ExprForLoop) {
        if self.is_param(&f.expr) {
            self.visit_block(&f.body);
            return;
        }
        syn::visit::visit_expr_for_loop(self, f);
    }
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        if matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            for side in [&b.left, &b.right] {
                if !self.is_param(side) {
                    self.visit_expr(side);
                }
            }
            return;
        }
        syn::visit::visit_expr_binary(self, b);
    }
    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if p.path.is_ident(&self.param) {
            self.other = true;
        }
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if idioms::count_ident(&m.tokens, &self.param) == 0 {
            return;
        }
        match errors::macro_args(m) {
            Some(args) if FORMATTING.contains(&syntax::macro_name(m).as_str()) => {
                for arg in &args {
                    if !self.is_param(arg) {
                        self.visit_expr(arg);
                    }
                }
            }
            _ => self.other = true,
        }
    }
}

/// `(type, method)` and `("", function)` pairs that UDL files export.
fn exported_signatures(cx: &Context) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for package in cx.ws.packages() {
        for item in uniffi::package_api(cx.ws, package) {
            match item.kind {
                ApiKind::Function => {
                    out.insert((String::new(), item.name.clone()));
                }
                _ => {
                    for m in &item.members {
                        let name = if m.name == "constructor" {
                            "new"
                        } else {
                            m.name.as_str()
                        };
                        out.insert((item.name.clone(), name.to_string()));
                    }
                }
            }
        }
    }
    out
}

/// The parameter type to suggest for `&String`, `&Vec<T>`, `&PathBuf` and
/// `&Option<T>`.
fn borrowed_form(ty: &syn::Type) -> Option<String> {
    let syn::Type::Reference(r) = ty else {
        return None;
    };
    if r.mutability.is_some() {
        return None; // `&mut Vec<T>` can push; `&mut Option<T>` can set
    }
    let name = syntax::type_ident(&r.elem)?;
    let args = syntax::type_args(&r.elem);
    match (name.as_str(), args.as_slice()) {
        ("String", []) => Some("&str".into()),
        ("PathBuf", []) => Some("&Path".into()),
        ("Vec", [t]) => Some(format!("&[{}]", syntax::type_string(t))),
        ("Option", [t]) => Some(match t {
            syn::Type::Reference(_) => format!("Option<{}>", syntax::type_string(t)),
            _ => {
                let inner = syn::Type::Reference(syn::TypeReference {
                    and_token: Default::default(),
                    lifetime: None,
                    mutability: None,
                    elem: Box::new((*t).clone()),
                });
                // `&Option<String>` reads best as `Option<&str>` (`.as_deref()`).
                let target =
                    borrowed_form(&inner).unwrap_or_else(|| format!("&{}", syntax::type_string(t)));
                format!("Option<{target}>")
            }
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/stateful/gecko_prefs.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/borrowed-params/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 5, 6], "{found:#?}");
        assert!(found[0].message.contains("take `&[GeckoPrefState]`"));
        assert!(found[1]
            .message
            .contains("take `Option<&Arc<GeckoPrefStore>>`"));
        let opt = check(
            &RULE,
            &[(PATH, "fn f(a: &Option<String>, b: &Option<Vec<u8>>) {}")],
        );
        assert!(opt[0].message.contains("take `Option<&str>`"), "{opt:#?}");
        assert!(opt[1].message.contains("take `Option<&[u8]>`"), "{opt:#?}");
        assert!(found[2].message.contains("take `&str`"));
    }

    /// Each finding's edits: (line, removed, inserted).
    fn edits(found: &[crate::Finding]) -> Vec<Vec<(usize, usize, Vec<String>)>> {
        found
            .iter()
            .map(|f| {
                f.fix
                    .iter()
                    .flat_map(|fix| &fix.edits)
                    .map(|e| (e.line, e.remove, e.insert.clone()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn fixes_signatures_the_body_still_fits() {
        let src = "fn a(names: &Vec<String>, root: &PathBuf, label: &String) -> usize {\n    for n in names {\n        println!(\"{n}\");\n    }\n    if root.exists() && label == \"x\" {\n        warn!(\"{}\", label);\n    }\n    names.len()\n}\n";
        let one = |l: &str| vec![(1, 1, vec![l.to_string()])];
        assert_eq!(
            edits(&check(&RULE, &[(PATH, src)])),
            vec![
                one("fn a(names: &[String], root: &PathBuf, label: &String) -> usize {"),
                // No `use std::path::Path` in the file.
                one("fn a(names: &Vec<String>, root: &std::path::Path, label: &String) -> usize {"),
                one("fn a(names: &Vec<String>, root: &PathBuf, label: &str) -> usize {"),
            ]
        );
        // rustfmt joins a signature that now fits on one line.
        let src =
            "fn long_function_name(\n    first: &Vec<u8>,\n) -> usize {\n    first.len()\n}\n";
        assert_eq!(
            edits(&check(&RULE, &[(PATH, src)])),
            vec![vec![(
                1,
                3,
                vec!["fn long_function_name(first: &[u8]) -> usize {".to_string()]
            )]]
        );
    }

    #[test]
    fn leaves_signatures_whose_uses_may_change() {
        let src = "fn b(v: &Vec<u8>) -> Vec<u8> {\n    v.clone()\n}\n\
                   fn c(s: &String) {\n    takes(s);\n}\n\
                   fn d(s: &String) {}\n\
                   fn e(o: &Option<String>) {}\n\
                   fn g() {\n    let f = d;\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 4, "{found:#?}");
        assert!(edits(&found).iter().all(Vec::is_empty), "{found:#?}");
        // The Don't example passes them on to `write_states`.
        let dont = include_str!("../../rules/borrowed-params/dont.rs");
        assert!(edits(&check(&RULE, &[(PATH, dont)]))
            .iter()
            .all(Vec::is_empty));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/borrowed-params/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_trait_impls_exports_mut_refs_and_serde_helpers() {
        let src = "impl Display for A { fn fmt(&self, s: &String) {} }\n\
            #[uniffi::export]\nimpl Store { pub fn put(&self, v: &Vec<u8>) {} }\n\
            fn push(v: &mut Vec<u8>, o: &mut Option<u8>) {}\n\
            #[derive(Serialize)]\nstruct T { #[serde(skip_serializing_if = \"T::is_none\")] x: Option<u8> }\n\
            impl T { fn is_none(x: &Option<u8>) -> bool { x.is_none() } }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
        let udl = "namespace push {};\ninterface PushManager {\n  void subscribe([ByRef] string? key);\n};\n";
        let rust = "impl PushManager { pub fn subscribe(&self, key: &Option<String>) {} }\n";
        let found = check(
            &RULE,
            &[
                ("components/push/Cargo.toml", "[package]\nname = \"push\"\n"),
                ("components/push/src/push.udl", udl),
                ("components/push/src/lib.rs", rust),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
