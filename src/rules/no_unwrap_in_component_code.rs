//! No `unwrap()`/`expect()`/`panic!` in non-test component code
//!
//! Docs and examples: `rules/no-unwrap-in-component-code/`.
//!
//! clippy's `unwrap_used`/`expect_used`/`panic` report every call (hundreds,
//! most on invariants checked just above). The native check keeps the ones
//! reviewers keep asking about: a bare `.unwrap()` on the result of parsing
//! or converting data that comes from outside (JSON, URLs, numbers, UTF-8,
//! HTTP status codes), and `panic!` in a function that could return the
//! error. `.expect("why")` states an invariant and is left to clippy.

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "no-unwrap-in-component-code",
    area: Area::Errors,
    title: "No `unwrap()`/`expect()`/`panic!` in non-test component code",
    kind: Kind::Code,
    clippy: &[
        "clippy::unwrap_used",
        "clippy::expect_used",
        "clippy::panic",
    ],
    in_tests: false,
    check,
};

/// Functions that parse or convert their argument and fail on bad input.
const PARSE_FNS: &[&str] = &[
    "from_str",
    "from_slice",
    "from_value",
    "from_reader",
    "from_utf8",
    "from_str_radix",
    "try_from",
    "parse",
    "decode",
];

/// Methods that parse or convert their receiver and fail on bad input.
const PARSE_METHODS: &[&str] = &["parse", "try_into", "json", "decode"];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        let mut v = Unwraps {
            hits: Vec::new(),
            fns: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, col, msg) in v.hits {
            if !support.contains(line) {
                out.add_at(&file.path, line, col, msg);
            }
        }
    }
}

struct Unwraps {
    hits: Vec<(usize, usize, String)>,
    /// For each enclosing fn or closure: whether it returns a `Result`.
    fns: Vec<bool>,
}

impl<'ast> Visit<'ast> for Unwraps {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.fns.push(errors::returns_result(&f.sig));
        syn::visit::visit_item_fn(self, f);
        self.fns.pop();
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.fns.push(errors::returns_result(&f.sig));
        syn::visit::visit_impl_item_fn(self, f);
        self.fns.pop();
    }
    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        let returns_result = matches!(&c.output, syn::ReturnType::Type(_, ty)
            if syntax::type_ident(ty).is_some_and(|n| n.ends_with("Result")));
        self.fns.push(returns_result);
        syn::visit::visit_expr_closure(self, c);
        self.fns.pop();
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "unwrap" && call.args.is_empty() {
            if let Some(what) = parses_outside_data(&call.receiver) {
                let start = call.method.span().start();
                self.hits.push((
                    start.line,
                    start.column + 1,
                    format!(
                        "`{what}(..).unwrap()` panics on bad input; return an error with `?` instead"
                    ),
                ));
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
    fn visit_expr_if(&mut self, e: &'ast syn::ExprIf) {
        // `if cfg!(debug_assertions) { panic!(..) }` crashes debug builds only, on purpose.
        let debug_only = matches!(&*e.cond, syn::Expr::Macro(m)
            if syntax::macro_name(&m.mac) == "cfg" && m.mac.tokens.to_string().contains("debug_assertions"));
        if debug_only {
            self.fns.push(false);
            syn::visit::visit_expr_if(self, e);
            self.fns.pop();
        } else {
            syn::visit::visit_expr_if(self, e);
        }
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if syntax::macro_name(mac) == "panic" && self.fns.last().copied().unwrap_or(false) {
            let start = mac.path.segments[0].ident.span().start();
            self.hits.push((
                start.line,
                start.column + 1,
                "`panic!` in a function that returns `Result`; return an error variant instead"
                    .to_string(),
            ));
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// When `expr` parses or converts a non-constant value, the name of the call.
fn parses_outside_data(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Call(call) => {
            let syn::Expr::Path(func) = &*call.func else {
                return None;
            };
            let last = func.path.segments.last()?.ident.to_string();
            let input = call.args.first()?;
            (PARSE_FNS.contains(&last.as_str()) && !is_constant(input))
                .then(|| syntax::path_string(&func.path))
        }
        syn::Expr::MethodCall(m) => {
            let name = m.method.to_string();
            (PARSE_METHODS.contains(&name.as_str()) && m.args.is_empty() && !is_constant(&m.receiver))
                .then(|| format!(".{name}"))
        }
        syn::Expr::Paren(p) => parses_outside_data(&p.expr),
        _ => None,
    }
}

/// Literals, consts (`CONTENT_URL`, `config::BASE`) and values derived only
/// from them (`Self::Prod.raw_url()`): input that can't be bad at runtime.
fn is_constant(expr: &syn::Expr) -> bool {
    let mut constant = true;
    errors::scan_tokens(quote::ToTokens::to_token_stream(expr), &mut |tokens, i| {
        let proc_macro2::TokenTree::Ident(ident) = &tokens[i] else {
            return;
        };
        let name = ident.to_string();
        let module = errors::is_punct(tokens.get(i + 1), ':');
        let method = i > 0
            && errors::is_punct(tokens.get(i - 1), '.')
            && errors::is_group(tokens.get(i + 1), proc_macro2::Delimiter::Parenthesis);
        let upper = name.chars().next().is_some_and(|c| c.is_uppercase());
        if !(module || method || upper || name == "as" || name == "str") {
            constant = false;
        }
    });
    constant
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/tabs/src/storage.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!(
            "../../rules/no-unwrap-in-component-code/dont.rs"
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 5);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/no-unwrap-in-component-code/do.rs")).is_empty());
    }

    #[test]
    fn flags_parse_and_try_from_on_input() {
        let found = run("fn f(s: &str, code: u16) {\n    let n: u32 = s.parse().unwrap();\n    let c = StatusCode::try_from(code).unwrap();\n}\n");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn ignores_constant_input() {
        assert!(run("fn f() -> Url {\n    Url::parse(CONTENT_URL).unwrap();\n    Url::parse(Self::Prod.raw_url()).unwrap();\n    \"1\".parse::<u8>().unwrap();\n    Url::parse(config::BASE).expect(\"valid\")\n}\n").is_empty());
    }

    #[test]
    fn ignores_expect_with_a_stated_invariant() {
        assert!(run("fn f(v: Vec<u8>) -> String {\n    String::from_utf8(v).expect(\"guids are ASCII\")\n}\n").is_empty());
    }

    #[test]
    fn ignores_debug_only_panics() {
        assert!(run("fn f() -> Result<()> {\n    if cfg!(debug_assertions) {\n        panic!(\"misuse\");\n    }\n    Ok(())\n}\n").is_empty());
    }

    #[test]
    fn ignores_invariants_and_serialization() {
        assert!(run("fn f(v: Vec<u8>) -> Result<()> {\n    let first = v.first().unwrap();\n    let s = serde_json::to_string(&v).unwrap();\n    Ok(())\n}\n").is_empty());
    }

    #[test]
    fn flags_panic_only_where_an_error_could_be_returned() {
        let found = run("fn f() -> Result<()> {\n    panic!(\"bad record\");\n}\nfn g() {\n    panic!(\"misuse\");\n}\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn ignores_test_support() {
        let src = "#[cfg(feature = \"testing\")]\nfn f(s: &str) { s.parse::<u8>().unwrap(); }\npub mod test_utils { fn g(s: &str) { s.parse::<u8>().unwrap(); } }\n";
        assert!(run(src).is_empty());
    }
}
