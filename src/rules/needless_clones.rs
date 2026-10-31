//! Avoid needless clones and allocations; write `Arc::clone(&x)`
//!
//! Docs and examples: `rules/needless-clones/`.

use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "needless-clones",
    area: Area::Idioms,
    title: "Avoid needless clones and allocations; write `Arc::clone(&x)`",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::redundant_clone",
        "clippy::implicit_clone",
        "clippy::str_to_string",
        "clippy::clone_on_ref_ptr",
        "clippy::inefficient_to_string",
        "clippy::manual_string_new",
    ],
    in_tests: false,
    check,
};

/// Conversions that copy a value that is already an owned `String`.
const COPIES: &[&str] = &["to_string", "to_owned", "clone"];

/// Clippy covers most of this rule. varlope adds what needs no type
/// information: copying a `String` that was just created
/// (`format!(..).to_string()`, `x.to_string().clone()`).
fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws) {
        let mut v = Copies(Vec::new());
        v.visit_file(&file.ast);
        for (m, made_by) in v.0 {
            let (line, column) = file.pos(&m.method);
            out.add_at(
                &file.path,
                line,
                column,
                format!("`{made_by}` already returns a new `String`; drop the `.{}()`", m.method),
            );
        }
    }
}

struct Copies<'a>(Vec<(&'a syn::ExprMethodCall, String)>);

impl<'a> Visit<'a> for Copies<'a> {
    fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
        let method = m.method.to_string();
        if COPIES.contains(&method.as_str()) && m.args.is_empty() {
            let made_by = match &*m.receiver {
                syn::Expr::Macro(mac) if syntax::macro_name(&mac.mac) == "format" => {
                    Some("format!(..)".to_string())
                }
                syn::Expr::MethodCall(inner)
                    if inner.args.is_empty()
                        && matches!(inner.method.to_string().as_str(), "to_string" | "to_owned")
                        // `x.to_owned().to_string()` may be `&&str` -> `String`.
                        && !(inner.method == "to_owned" && method == "to_string") =>
                {
                    Some(format!(".{}()", inner.method))
                }
                _ => None,
            };
            if let Some(made_by) = made_by {
                self.0.push((m, made_by));
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/nimbus/src/enrollment.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/needless-clones/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/needless-clones/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_double_conversions_only() {
        let src = "fn f(s: &str, n: u8) {\n\
            let a = s.to_string().clone();\n\
            let b = format!(\"{n}\")\n    .to_owned();\n\
            let c = s.to_owned();\n\
            let d = format!(\"{n}\").len();\n\
        }\n";
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 4], "{found:#?}");
    }
}
