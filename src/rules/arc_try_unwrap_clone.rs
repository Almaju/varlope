//! `Arc::try_unwrap(x.clone())` always fails.
//!
//! Docs and examples: `rules/arc-try-unwrap-clone/`.

use syn::visit::Visit;

use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "arc-try-unwrap-clone",
    area: Area::Concurrency,
    title: "`Arc::try_unwrap(x.clone())` always fails",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        let mut visitor = Calls { hits: Vec::new() };
        visitor.visit_file(&file.ast);
        for (line, column, what) in visitor.hits {
            out.add_at(
                &file.path,
                line,
                column,
                format!("`Arc::{what}` on a fresh clone always fails: the original still holds a reference"),
            );
        }
    }
}

struct Calls {
    hits: Vec<(usize, usize, String)>,
}

impl<'ast> Visit<'ast> for Calls {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*call.func {
            for what in ["try_unwrap", "into_inner"] {
                let is_arc = syntax::path_ends_with(&func.path, &format!("Arc::{what}"));
                let on_clone = matches!(
                    call.args.first(),
                    Some(syn::Expr::MethodCall(m)) if m.method == "clone" && m.args.is_empty()
                );
                if is_arc && on_clone {
                    let start = func.path.segments[0].ident.span().start();
                    self.hits
                        .push((start.line, start.column + 1, what.to_string()));
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

    const PATH: &str = "components/x/src/lib.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/arc-try-unwrap-clone/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        assert!(check(
            &RULE,
            &[(PATH, include_str!("../../rules/arc-try-unwrap-clone/do.rs"))]
        )
        .is_empty());
    }

    #[test]
    fn ignores_owned_arcs() {
        assert!(check(
            &RULE,
            &[(PATH, "fn f(a: Arc<u8>) { let _ = Arc::try_unwrap(a); }")]
        )
        .is_empty());
    }
}
