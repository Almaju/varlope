//! Name and explain magic values and non-obvious intent
//!
//! Docs and examples: `rules/explain-magic-values/`.

use syn::visit::Visit;

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "explain-magic-values",
    area: Area::Docs,
    title: "Name and explain magic values and non-obvious intent",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// `Duration` constructors whose argument is a time window.
const DURATIONS: &[&str] = &[
    "Duration::from_secs",
    "Duration::from_millis",
    "Duration::from_mins",
    "Duration::from_hours",
    "Duration::from_days",
];

/// Calls whose first argument is a queue or channel capacity.
const CAPACITIES: &[&str] = &["sync_channel", "mpsc::channel", "bounded", "Semaphore::new"];

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        let files: Vec<&RustFile> = cx
            .ws
            .package_files(package)
            .filter_map(|f| cx.ws.rust(&f.path))
            .filter(|f| !is_out_of_scope(&f.path))
            .collect();
        for file in &files {
            let mut v = Literals {
                file,
                in_const: 0,
                hits: Vec::new(),
            };
            v.visit_file(&file.ast);
            for (line, message) in v.hits {
                if !file.in_test(line) && !commented(file, line) {
                    out.add(&file.path, line, message);
                }
            }
        }
    }
}

/// A comment on the line, or ending on the line above, explains the value
/// (a bare `// flagged: …` marker in an example doesn't).
fn commented(file: &RustFile, line: usize) -> bool {
    file.comments.iter().any(|c| {
        let ends = c.line + c.text.matches('\n').count();
        !c.is_doc()
            && !c.text.trim_start().starts_with("flagged")
            && (c.line == line || ends + 1 == line)
    })
}

struct Literals<'f> {
    file: &'f RustFile,
    /// Depth of `const`/`static` initializers we are in.
    in_const: usize,
    hits: Vec<(usize, String)>,
}

impl Literals<'_> {
    /// The integer value of a literal expression (`86400`, `24 * 60 * 60`).
    fn value(expr: &syn::Expr) -> Option<u128> {
        match expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(i),
                ..
            }) => i.base10_parse().ok(),
            syn::Expr::Binary(b) => {
                let (l, r) = (Self::value(&b.left)?, Self::value(&b.right)?);
                match b.op {
                    syn::BinOp::Mul(_) => l.checked_mul(r),
                    syn::BinOp::Add(_) => l.checked_add(r),
                    _ => None,
                }
            }
            syn::Expr::Paren(p) => Self::value(&p.expr),
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for Literals<'_> {
    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        self.in_const += 1;
        syn::visit::visit_item_const(self, c);
        self.in_const -= 1;
    }
    fn visit_item_static(&mut self, s: &'ast syn::ItemStatic) {
        self.in_const += 1;
        syn::visit::visit_item_static(self, s);
        self.in_const -= 1;
    }
    fn visit_impl_item_const(&mut self, c: &'ast syn::ImplItemConst) {
        self.in_const += 1;
        syn::visit::visit_impl_item_const(self, c);
        self.in_const -= 1;
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.in_const == 0 {
            if let syn::Expr::Path(func) = &*call.func {
                let arg = call.args.first();
                let value = arg.and_then(Self::value);
                let text = arg
                    .map(|a| self.file.text_of(a).to_string())
                    .unwrap_or_default();
                let line = self.file.line_of_node(call);
                if let Some(name) = DURATIONS
                    .iter()
                    .find(|d| syntax::path_ends_with(&func.path, d))
                {
                    if value.is_some_and(|v| v > 1) {
                        self.hits.push((
                            line,
                            format!("`{name}({text})`: where does this window come from? A named `const` with a comment would answer it"),
                        ));
                    }
                }
                if let Some(name) = CAPACITIES
                    .iter()
                    .find(|c| syntax::path_ends_with(&func.path, c))
                {
                    if value.is_some_and(|v| v > 2) {
                        self.hits.push((
                            line,
                            format!("`{name}({text})`: why this capacity, and what happens when it fills up? Worth a named `const` and a comment"),
                        ));
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/suggest/Cargo.toml",
        "[package]\nname = \"suggest\"\n",
    );
    const PATH: &str = "components/suggest/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/explain-magic-values/dont.rs"),
                ),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (PATH, include_str!("../../rules/explain-magic-values/do.rs")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn capacities_comments_and_tests() {
        let text = "fn a() {\n    let (tx, rx) = std::sync::mpsc::sync_channel(1000);\n    // Bounded so a stalled consumer can't grow memory without limit.\n    let (tx2, rx2) = std::sync::mpsc::sync_channel(1000);\n    let d = Duration::from_secs(1);\n}\n#[cfg(test)]\nmod tests {\n    fn t() { let d = Duration::from_secs(300); }\n}\n";
        let found = check(&RULE, &[MANIFEST, (PATH, text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 2);
    }
}
