//! Use `collect`/`extend`/entry API; compile regexes once
//!
//! Docs and examples: `rules/collection-idioms/`.

use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "collection-idioms",
    area: Area::Idioms,
    title: "Use `collect`/`extend`/entry API; compile regexes once",
    kind: Kind::Clippy,
    clippy: &[
        "clippy::needless_collect",
        "clippy::explicit_iter_loop",
        "clippy::map_entry",
        "clippy::extend_with_drain",
        "clippy::same_item_push",
        "clippy::regex_creation_in_loops",
    ],
    in_tests: false,
    check,
};

/// Clippy covers the entry API, needless collects and regexes in loops.
/// varlope adds the pattern reviewers rewrote most often and clippy has no
/// lint for: an empty `Vec` filled by a loop that only pushes, which is a
/// `collect()`.
fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws).filter(|f| !idioms::is_tooling(&f.path)) {
        let mut v = PushLoops(Vec::new());
        v.visit_file(&file.ast);
        for (l, message) in v.0 {
            let (line, column) = file.pos(&l.for_token);
            out.add_at(&file.path, line, column, message);
        }
    }
}

struct PushLoops<'a>(Vec<(&'a syn::ExprForLoop, String)>);

impl<'a> Visit<'a> for PushLoops<'a> {
    fn visit_block(&mut self, block: &'a syn::Block) {
        for (i, pair) in block.stmts.windows(2).enumerate() {
            let (Some(name), Some(l)) = (new_vec(&pair[0]), for_loop(&pair[1])) else {
                continue;
            };
            let Some(arg) = only_push(&l.body, &name) else {
                continue;
            };
            // `?`, `return`... inside the loop read better as a loop.
            let mut flow = Flow::default();
            flow.visit_expr(arg);
            if flow.exits || flow.tries {
                continue;
            }
            // Filled again afterwards: the loop is one of several sources.
            if block.stmts[i + 2..].iter().any(|s| grows(s, &name)) {
                continue;
            }
            self.0.push((
                l,
                format!("`{name}` is built by a loop that only pushes; write `let {name}: Vec<_> = ….map(..).collect();`"),
            ));
        }
        syn::visit::visit_block(self, block);
    }
}

/// `let mut name = Vec::new();` (or `vec![]`, `Vec::with_capacity(n)`).
fn new_vec(stmt: &syn::Stmt) -> Option<String> {
    let syn::Stmt::Local(local) = stmt else { return None };
    let pat = match &local.pat {
        syn::Pat::Type(t) => &*t.pat,
        p => p,
    };
    let syn::Pat::Ident(p) = pat else { return None };
    p.mutability?;
    let init = &local.init.as_ref()?.expr;
    let empty = match &**init {
        syn::Expr::Call(c) => matches!(&*c.func, syn::Expr::Path(f)
            if syntax::path_ends_with(&f.path, "Vec::new")
                || syntax::path_ends_with(&f.path, "Vec::with_capacity")),
        syn::Expr::Macro(m) => syntax::macro_name(&m.mac) == "vec" && m.mac.tokens.is_empty(),
        _ => false,
    };
    empty.then(|| p.ident.to_string())
}

/// Whether a statement adds to `name` (`name.push(..)`, `name.extend(..)`).
fn grows(stmt: &syn::Stmt, name: &str) -> bool {
    struct V<'n>(&'n str, bool);
    impl<'a> Visit<'a> for V<'_> {
        fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
            let on_name = matches!(&*m.receiver, syn::Expr::Path(p) if p.path.is_ident(self.0));
            let adds = ["push", "extend", "insert", "append", "extend_from_slice"];
            if on_name && adds.contains(&m.method.to_string().as_str()) {
                self.1 = true;
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }
    let mut v = V(name, false);
    v.visit_stmt(stmt);
    v.1
}

fn for_loop(stmt: &syn::Stmt) -> Option<&syn::ExprForLoop> {
    match stmt {
        syn::Stmt::Expr(syn::Expr::ForLoop(l), _) if l.label.is_none() => Some(l),
        _ => None,
    }
}

/// The argument of the loop body's only statement, `name.push(arg);`.
fn only_push<'a>(body: &'a syn::Block, name: &str) -> Option<&'a syn::Expr> {
    let [stmt] = body.stmts.as_slice() else { return None };
    let syn::Stmt::Expr(syn::Expr::MethodCall(m), _) = stmt else {
        return None;
    };
    let on_name = matches!(&*m.receiver, syn::Expr::Path(p) if p.path.is_ident(name));
    (on_name && m.method == "push" && m.args.len() == 1).then(|| &m.args[0])
}

/// Control flow that makes a loop the clearer form.
#[derive(Default)]
struct Flow {
    exits: bool,
    tries: bool,
}

impl<'a> Visit<'a> for Flow {
    fn visit_expr(&mut self, e: &'a syn::Expr) {
        match e {
            syn::Expr::Return(_)
            | syn::Expr::Break(_)
            | syn::Expr::Continue(_)
            | syn::Expr::Await(_) => self.exits = true,
            syn::Expr::Try(_) => {
                self.tries = true;
                syn::visit::visit_expr(self, e);
            }
            _ => syn::visit::visit_expr(self, e),
        }
    }

    fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {}
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
            &[(PATH, include_str!("../../rules/collection-idioms/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
        assert!(found[0].message.contains("`slugs`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/collection-idioms/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn skips_fallible_conditional_and_multi_source_loops() {
        let src = "fn a(rows: &[Row]) -> Result<Vec<Item>> {\n\
                let mut items = vec![];\n\
                for row in rows {\n\
                    items.push(Item::try_from(row)?);\n\
                }\n\
                for row in extra { items.push(row.into()); }\n\
                Ok(items)\n\
            }\n\
            fn b(rows: &[Row]) -> Vec<u8> {\n\
                let mut out = Vec::new();\n\
                for row in rows { if row.ok { out.push(row.id) } }\n\
                let mut seen = Vec::new();\n\
                for row in rows { seen.push(row.id); log(row); }\n\
                let mut early = Vec::new();\n\
                for row in rows { early.push(if row.ok { row.id } else { return out }); }\n\
                out\n\
            }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
