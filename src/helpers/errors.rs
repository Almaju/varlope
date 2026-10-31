//! Helpers shared by the errors rules.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::visit::Visit;

use crate::source::RustFile;
use crate::syntax;
use crate::workspace::{is_component_src, Workspace};

/// Crates under `components/` that never run inside an app: CLIs, build-time
/// tools, dev backends, test helpers, benchmarks, and a crate that crashes on
/// purpose.
const NOT_SHIPPED: &[&str] = &[
    "components/crashtest/",
    "components/example/",
    "components/suggest/suggest-bench/",
    "components/support/find-places-db/",
    "components/support/nimbus-cli/",
    "components/support/nimbus-fml/",
    "components/support/rc_crypto/nss/nss_build_common/",
    "components/support/restmail-client/",
    "components/support/text-table/",
    "components/support/viaduct-dev/",
];

/// Rust code that ships in the apps: component `src/` files, minus test files,
/// generated or vendored code, binaries and the crates in [`NOT_SHIPPED`].
pub fn is_shipped(path: &str) -> bool {
    is_component_src(path)
        && !is_generated(path)
        && !path.contains("/src/bin/")
        && !path.ends_with("/src/main.rs")
        && !NOT_SHIPPED.iter().any(|p| path.starts_with(p))
}

/// Generated or vendored code nobody edits by hand.
pub fn is_generated(path: &str) -> bool {
    path.starts_with("components/external/")
        || path.contains("/generated/")
        || path.contains("/bindings/")
        || path.contains("_generated")
}

/// Every parsed Rust file that ships in the apps.
pub fn shipped_files(ws: &Workspace) -> impl Iterator<Item = &RustFile> {
    ws.rust_files().filter(|f| is_shipped(&f.path))
}

/// Modules and files that only hold test helpers, benchmarks or debug tools,
/// even when they are not `#[cfg(test)]`.
const TEST_SUPPORT_MODS: &[&str] = &[
    "test_utils",
    "test_helpers",
    "test_support",
    "testing",
    "benchmarks",
    "debug_tools",
];

/// Features that only turn on test helpers, benchmarks or debug tools.
const TEST_SUPPORT_FEATURES: &[&str] = &["testing", "benchmark_api", "debug-tools"];

/// Lines of a file that hold test-support code the runner does not already
/// treat as tests: `mod test_utils`, `#[cfg(feature = "testing")]` items, and
/// `#[cfg(test)]` on match arms, statements, fields and expressions.
pub fn test_support_lines(file: &RustFile) -> Vec<std::ops::Range<usize>> {
    let name = file.path.rsplit('/').next().unwrap_or("");
    let stem = name.trim_end_matches(".rs");
    if TEST_SUPPORT_MODS.contains(&stem)
        || TEST_SUPPORT_MODS
            .iter()
            .any(|m| file.path.contains(&format!("/{m}/")))
    {
        return vec![1..usize::MAX];
    }
    struct V(Vec<std::ops::Range<usize>>);
    impl V {
        fn check<T: syn::spanned::Spanned>(&mut self, attrs: &[syn::Attribute], node: &T) -> bool {
            if is_test_support_gated(attrs) {
                let span = node.span();
                self.0.push(span.start().line..span.end().line + 1);
                return true;
            }
            false
        }
    }
    impl<'a> Visit<'a> for V {
        fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
            let named = TEST_SUPPORT_MODS.contains(&m.ident.to_string().as_str());
            if named {
                let span = syn::spanned::Spanned::span(m);
                self.0.push(span.start().line..span.end().line + 1);
            } else if !self.check(&m.attrs, m) {
                syn::visit::visit_item_mod(self, m);
            }
        }
        fn visit_item(&mut self, i: &'a syn::Item) {
            let attrs: &[syn::Attribute] = match i {
                syn::Item::Fn(x) => &x.attrs,
                syn::Item::Impl(x) => &x.attrs,
                syn::Item::Struct(x) => &x.attrs,
                syn::Item::Enum(x) => &x.attrs,
                syn::Item::Static(x) => &x.attrs,
                syn::Item::Const(x) => &x.attrs,
                syn::Item::Use(x) => &x.attrs,
                syn::Item::Trait(x) => &x.attrs,
                _ => &[],
            };
            if !self.check(attrs, i) {
                syn::visit::visit_item(self, i);
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
            if !self.check(&f.attrs, f) {
                syn::visit::visit_impl_item_fn(self, f);
            }
        }
        fn visit_arm(&mut self, a: &'a syn::Arm) {
            if !self.check(&a.attrs, a) {
                syn::visit::visit_arm(self, a);
            }
        }
        fn visit_stmt(&mut self, s: &'a syn::Stmt) {
            let attrs: &[syn::Attribute] = match s {
                syn::Stmt::Local(l) => &l.attrs,
                syn::Stmt::Macro(m) => &m.attrs,
                _ => &[],
            };
            if !self.check(attrs, s) {
                syn::visit::visit_stmt(self, s);
            }
        }
        fn visit_field(&mut self, f: &'a syn::Field) {
            if !self.check(&f.attrs, f) {
                syn::visit::visit_field(self, f);
            }
        }
        fn visit_variant(&mut self, v: &'a syn::Variant) {
            if !self.check(&v.attrs, v) {
                syn::visit::visit_variant(self, v);
            }
        }
        fn visit_field_value(&mut self, f: &'a syn::FieldValue) {
            if !self.check(&f.attrs, f) {
                syn::visit::visit_field_value(self, f);
            }
        }
    }
    let mut v = V(Vec::new());
    v.visit_file(&file.ast);
    v.0
}

/// `#[cfg(test)]`-like gates, plus features that only enable test helpers.
fn is_test_support_gated(attrs: &[syn::Attribute]) -> bool {
    syntax::is_test_gated(attrs)
        || attrs.iter().any(|a| {
            a.path().is_ident("cfg") && {
                let args = syntax::attr_args(a);
                !args.contains("not") && TEST_SUPPORT_FEATURES.iter().any(|f| args.contains(f))
            }
        })
}

/// A file's test-support lines, for filtering findings by line.
pub struct TestSupport(Vec<std::ops::Range<usize>>);

impl TestSupport {
    pub fn of(file: &RustFile) -> Self {
        Self(test_support_lines(file))
    }

    pub fn contains(&self, line: usize) -> bool {
        self.0.iter().any(|r| r.contains(&line))
    }
}

/// The component a path belongs to: `places` for `components/places/src/x.rs`,
/// `sql` for `components/support/sql/src/lib.rs`.
pub fn component_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("components/")?;
    let rest = rest.strip_prefix("support/").unwrap_or(rest);
    rest.split('/').next()
}

/// Logging and error-reporting macros: anything that records an error.
pub const LOG_MACROS: &[&str] = &[
    "trace",
    "debug",
    "info",
    "warn",
    "error",
    "log",
    "event",
    "report_error",
    "breadcrumb",
    "trace_error",
    "eprintln",
];

/// Whether `node` mentions a logging or reporting macro or call anywhere,
/// e.g. a `warn!` in a closure or `report_error_to_app(..)`.
pub fn logs_something<T: quote::ToTokens>(node: &T) -> bool {
    let mut found = false;
    scan_tokens(node.to_token_stream(), &mut |tokens, i| {
        let TokenTree::Ident(ident) = &tokens[i] else {
            return;
        };
        let name = ident.to_string();
        let bang = matches!(tokens.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!');
        if bang && LOG_MACROS.contains(&name.as_str()) {
            found = true;
        }
        if matches!(
            name.as_str(),
            "report_error_to_app" | "report_breadcrumb" | "inspect_err" | "log_error"
        ) {
            found = true;
        }
    });
    found
}

/// Call `f` with every token list in `stream` (recursively into groups) and
/// each index in it.
pub fn scan_tokens(stream: TokenStream, f: &mut dyn FnMut(&[TokenTree], usize)) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for i in 0..tokens.len() {
        f(&tokens, i);
        if let TokenTree::Group(g) = &tokens[i] {
            scan_tokens(g.stream(), f);
        }
    }
}

/// Whether a token is the punctuation `c`.
pub fn is_punct(token: Option<&TokenTree>, c: char) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == c)
}

/// Whether a token is a group with delimiter `d`.
pub fn is_group(token: Option<&TokenTree>, d: Delimiter) -> bool {
    matches!(token, Some(TokenTree::Group(g)) if g.delimiter() == d)
}

/// An expression as compact text: `self.db.lock()`.
pub fn text<T: quote::ToTokens>(node: &T) -> String {
    syntax::compact(&syntax::tokens(node))
}

/// Whether a function returns a `Result` (any alias ending in `Result`).
pub fn returns_result(sig: &syn::Signature) -> bool {
    match &sig.output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => {
            syntax::type_ident(ty).is_some_and(|name| name.ends_with("Result"))
        }
    }
}

/// Every function with a body in a file (free functions, methods, default
/// trait methods), including nested ones, skipping test-gated items.
pub fn functions(file: &syn::File) -> Vec<Function<'_>> {
    struct V<'a> {
        out: Vec<Function<'a>>,
        self_ty: Vec<Option<String>>,
    }
    impl<'a> Visit<'a> for V<'a> {
        fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
            if syntax::is_test_gated(&f.attrs) {
                return;
            }
            self.out.push(Function {
                attrs: &f.attrs,
                sig: &f.sig,
                block: &f.block,
                self_ty: None,
            });
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_item_impl(&mut self, i: &'a syn::ItemImpl) {
            if syntax::is_test_gated(&i.attrs) {
                return;
            }
            self.self_ty.push(syntax::type_ident(&i.self_ty));
            syn::visit::visit_item_impl(self, i);
            self.self_ty.pop();
        }
        fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
            if syntax::is_test_gated(&f.attrs) {
                return;
            }
            self.out.push(Function {
                attrs: &f.attrs,
                sig: &f.sig,
                block: &f.block,
                self_ty: self.self_ty.last().cloned().flatten(),
            });
            syn::visit::visit_impl_item_fn(self, f);
        }
        fn visit_trait_item_fn(&mut self, f: &'a syn::TraitItemFn) {
            if let Some(block) = &f.default {
                if !syntax::is_test_gated(&f.attrs) {
                    self.out.push(Function {
                        attrs: &f.attrs,
                        sig: &f.sig,
                        block,
                        self_ty: None,
                    });
                }
            }
            syn::visit::visit_trait_item_fn(self, f);
        }
        fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
            if !syntax::is_test_gated(&m.attrs) {
                syn::visit::visit_item_mod(self, m);
            }
        }
    }
    let mut v = V {
        out: Vec::new(),
        self_ty: Vec::new(),
    };
    v.visit_file(file);
    v.out
}

/// A function with a body.
pub struct Function<'a> {
    pub attrs: &'a [syn::Attribute],
    pub sig: &'a syn::Signature,
    pub block: &'a syn::Block,
    /// The `Self` type, for methods.
    pub self_ty: Option<String>,
}

/// Whether an expression is a literal, or a reference to one.
pub fn is_literal(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(_) => true,
        syn::Expr::Reference(r) => is_literal(&r.expr),
        syn::Expr::Group(g) => is_literal(&g.expr),
        syn::Expr::Paren(p) => is_literal(&p.expr),
        _ => false,
    }
}

/// The macro name and its arguments, parsed as comma-separated expressions
/// when they parse (`format!`-style macros).
pub fn macro_args(mac: &syn::Macro) -> Option<Vec<syn::Expr>> {
    use syn::punctuated::Punctuated;
    mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .ok()
        .map(|p| p.into_iter().collect())
}

/// Visit the expressions inside a `format!`-style macro (`format!`, `warn!`,
/// `vec!`, `assert!`...), which `syn` leaves as tokens.
/// The visitor must not borrow from the tree (`for<'a> Visit<'a>`).
pub fn visit_macro_args<V>(v: &mut V, mac: &syn::Macro)
where
    V: for<'a> Visit<'a>,
{
    if let Some(args) = macro_args(mac) {
        for arg in &args {
            v.visit_expr(arg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_paths() {
        assert!(is_shipped("components/places/src/db.rs"));
        assert!(is_shipped("components/support/sql/src/conn.rs"));
        assert!(!is_shipped("components/places/src/tests.rs"));
        assert!(!is_shipped("components/support/nimbus-fml/src/parser.rs"));
        assert!(!is_shipped("components/relevancy/src/bin/gen.rs"));
        assert!(!is_shipped("components/external/x/src/lib.rs"));
        assert!(!is_shipped("tools/x/src/lib.rs"));
    }

    #[test]
    fn components() {
        assert_eq!(component_of("components/places/src/db.rs"), Some("places"));
        assert_eq!(
            component_of("components/support/sql/src/lib.rs"),
            Some("sql")
        );
    }

    #[test]
    fn finds_logging() {
        let e: syn::Expr = syn::parse_quote!(r.inspect_err(|e| warn!("x {e}")).ok());
        assert!(logs_something(&e));
        let e: syn::Expr = syn::parse_quote!(r.ok());
        assert!(!logs_something(&e));
    }
}
