//! Don't let HashMap/HashSet order leak into results, storage or generated output
//!
//! Docs and examples: `rules/deterministic-iteration/`.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::idioms::{self, functions, FnInfo};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "deterministic-iteration",
    area: Area::Idioms,
    title: "Don't let HashMap/HashSet order leak into results, storage or generated output",
    kind: Kind::Code,
    clippy: &["clippy::iter_over_hash_type"],
    in_tests: false,
    check,
};

/// Crates whose output is generated code, persisted data or ordered results.
const OPTED_IN: &[&str] = &[
    "components/support/nimbus-fml/",
    "components/suggest/",
    "components/search/",
    "components/sync_manager/",
];

const HASH_TYPES: &[&str] = &["HashMap", "HashSet"];

/// Methods that iterate a collection in its own order.
const ITER_METHODS: &[&str] = &[
    "iter",
    "iter_mut",
    "keys",
    "values",
    "values_mut",
    "into_iter",
    "into_keys",
    "into_values",
    "drain",
];

fn check(cx: &Context, out: &mut Findings) {
    let files: Vec<&RustFile> = idioms::rust_files(cx.ws)
        .filter(|f| OPTED_IN.iter().any(|p| f.path.starts_with(p)))
        .collect();
    // Struct name -> its HashMap/HashSet fields, per crate directory.
    let mut fields: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for file in &files {
        let krate = crate_dir(cx, &file.path);
        for item in syntax::all_items(&file.ast) {
            if let syn::Item::Struct(s) = item {
                for field in &s.fields {
                    if let Some(name) = &field.ident {
                        if is_hash_type(&field.ty) {
                            fields
                                .entry((krate.clone(), s.ident.to_string()))
                                .or_default()
                                .insert(name.to_string());
                        }
                    }
                }
            }
        }
    }
    for file in files {
        let krate = crate_dir(cx, &file.path);
        for f in functions(&file.ast) {
            let self_fields = f
                .self_ty
                .as_ref()
                .and_then(|t| fields.get(&(krate.clone(), t.clone())))
                .cloned()
                .unwrap_or_default();
            let hashes = HashNames {
                locals: hash_locals(&f),
                fields: self_fields,
            };
            let mut v = Leaks {
                hashes: &hashes,
                hits: Vec::new(),
                body: f.block,
            };
            v.visit_block(f.block);
            for (span_of, message) in v.hits {
                out.add_at(&file.path, span_of.0, span_of.1, message);
            }
        }
    }
}

fn crate_dir(cx: &Context, path: &str) -> String {
    cx.ws
        .package_of(path)
        .map(|p| p.dir.clone())
        .unwrap_or_default()
}

fn is_hash_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(r) => is_hash_type(&r.elem),
        syn::Type::Paren(p) => is_hash_type(&p.elem),
        _ => syntax::type_ident(ty).is_some_and(|t| HASH_TYPES.contains(&t.as_str())),
    }
}

/// What is known to be a hash collection inside one function.
struct HashNames {
    /// Parameters and `let` bindings.
    locals: BTreeSet<String>,
    /// HashMap/HashSet fields of `self`.
    fields: BTreeSet<String>,
}

impl HashNames {
    /// `x`, `&x`, `self.f`, `&mut self.f` where those are hash collections.
    fn is_hash(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Reference(r) => self.is_hash(&r.expr),
            syn::Expr::Paren(p) => self.is_hash(&p.expr),
            syn::Expr::Path(p) => {
                let ident = p.path.get_ident()?.to_string();
                self.locals.contains(&ident).then_some(ident)
            }
            syn::Expr::Field(f) => {
                let syn::Member::Named(name) = &f.member else {
                    return None;
                };
                let on_self = matches!(&*f.base, syn::Expr::Path(p) if p.path.is_ident("self"));
                (on_self && self.fields.contains(&name.to_string())).then(|| format!("self.{name}"))
            }
            _ => None,
        }
    }

    /// `x.keys()` and friends on a hash collection, or the collection itself.
    fn iterates_hash(&self, expr: &syn::Expr) -> Option<String> {
        if let syn::Expr::MethodCall(m) = expr {
            if ITER_METHODS.contains(&m.method.to_string().as_str()) {
                return self.is_hash(&m.receiver);
            }
            return None;
        }
        self.is_hash(expr)
    }
}

/// Parameters and locals typed or built as a HashMap/HashSet.
fn hash_locals(f: &FnInfo) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for input in &f.sig.inputs {
        if let syn::FnArg::Typed(t) = input {
            if let syn::Pat::Ident(p) = &*t.pat {
                if is_hash_type(&t.ty) {
                    out.insert(p.ident.to_string());
                }
            }
        }
    }
    struct Lets<'o>(&'o mut BTreeSet<String>);
    impl<'a> Visit<'a> for Lets<'_> {
        fn visit_local(&mut self, local: &'a syn::Local) {
            let (pat, ty) = match &local.pat {
                syn::Pat::Type(t) => (&*t.pat, Some(&*t.ty)),
                pat => (pat, None),
            };
            if let syn::Pat::Ident(p) = pat {
                let typed = ty.is_some_and(is_hash_type);
                let built = local.init.as_ref().is_some_and(|i| builds_hash(&i.expr));
                if typed || (ty.is_none() && built) {
                    self.0.insert(p.ident.to_string());
                }
            }
            syn::visit::visit_local(self, local);
        }
    }
    Lets(&mut out).visit_block(f.block);
    out
}

/// `HashMap::new()`, `HashSet::with_capacity(n)`, `.collect::<HashMap<_, _>>()`.
fn builds_hash(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Call(c) => match &*c.func {
            syn::Expr::Path(p) => {
                let segs: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                segs.len() >= 2 && HASH_TYPES.contains(&segs[segs.len() - 2].as_str())
            }
            _ => false,
        },
        syn::Expr::MethodCall(m) if m.method == "collect" => {
            m.turbofish.as_ref().is_some_and(|t| {
                t.args.iter().any(|a| match a {
                    syn::GenericArgument::Type(ty) => is_hash_type(ty),
                    _ => false,
                })
            })
        }
        _ => false,
    }
}

struct Leaks<'h, 'a> {
    hashes: &'h HashNames,
    hits: Vec<((usize, usize), String)>,
    /// The whole function body, to find a later `.sort()`.
    body: &'a syn::Block,
}

impl Leaks<'_, '_> {
    /// Whether `name` is sorted anywhere in the function (`name.sort()`).
    fn sorts(&self, name: &str) -> bool {
        let mut found = false;
        syntax_method_calls(self.body, |m| {
            if m.method.to_string().starts_with("sort")
                && matches!(&*m.receiver, syn::Expr::Path(p) if p.path.is_ident(name))
            {
                found = true;
            }
        });
        found
    }
}

/// Names bound by `let` inside a block.
fn local_names(block: &syn::Block) -> BTreeSet<String> {
    struct V(BTreeSet<String>);
    impl<'a> Visit<'a> for V {
        fn visit_pat_ident(&mut self, p: &'a syn::PatIdent) {
            self.0.insert(p.ident.to_string());
            syn::visit::visit_pat_ident(self, p);
        }
    }
    let mut v = V(BTreeSet::new());
    v.visit_block(block);
    v.0
}

fn syntax_method_calls<'a>(block: &'a syn::Block, mut f: impl FnMut(&'a syn::ExprMethodCall)) {
    struct V<F>(F);
    impl<'a, F: FnMut(&'a syn::ExprMethodCall)> Visit<'a> for V<F> {
        fn visit_expr_method_call(&mut self, m: &'a syn::ExprMethodCall) {
            (self.0)(m);
            syn::visit::visit_expr_method_call(self, m);
        }
    }
    V(&mut f).visit_block(block);
}

impl<'a> Visit<'a> for Leaks<'_, 'a> {
    fn visit_expr_for_loop(&mut self, l: &'a syn::ExprForLoop) {
        if let Some(what) = self.hashes.iterates_hash(&l.expr) {
            // The loop body appends to an ordered output that is not sorted later.
            let mut sink = None;
            syntax_method_calls(&l.body, |m| {
                let method = m.method.to_string();
                if sink.is_none() && matches!(method.as_str(), "push" | "push_str" | "push_back") {
                    if let syn::Expr::Path(p) = &*m.receiver {
                        if let Some(id) = p.path.get_ident() {
                            sink = Some(id.to_string());
                        }
                    }
                }
            });
            let mut writes = false;
            idioms::macros_in(&l.body, |mac| {
                writes |= matches!(syntax::macro_name(mac).as_str(), "write" | "writeln");
            });
            // A Vec declared inside the loop is rebuilt for every entry.
            let inner = local_names(&l.body);
            let leak = match &sink {
                Some(name) => !inner.contains(name) && !self.sorts(name),
                None => writes,
            };
            if leak {
                let target = sink.map_or("the output".to_string(), |s| format!("`{s}`"));
                let start = l.for_token.span.start();
                self.hits.push((
                    (start.line, start.column + 1),
                    format!("iterating `{what}` (a hash collection) makes the order of {target} change from run to run; use a BTreeMap/BTreeSet or sort first"),
                ));
            }
        }
        syn::visit::visit_expr_for_loop(self, l);
    }

    fn visit_local(&mut self, local: &'a syn::Local) {
        // `let v: Vec<_> = map.keys().cloned().collect();` never sorted.
        if let (syn::Pat::Type(t), Some(init)) = (&local.pat, &local.init) {
            if let (syn::Pat::Ident(name), Some("Vec")) =
                (&*t.pat, syntax::type_ident(&t.ty).as_deref())
            {
                self.collected(&name.ident.to_string(), &init.expr);
            }
        }
        if let (syn::Pat::Ident(name), Some(init)) = (&local.pat, &local.init) {
            if let syn::Expr::MethodCall(m) = &*init.expr {
                let to_vec = m.method == "collect"
                    && m.turbofish.as_ref().is_some_and(|t| {
                        t.args.iter().any(|a| {
                            matches!(a, syn::GenericArgument::Type(ty)
                                if syntax::type_ident(ty).as_deref() == Some("Vec"))
                        })
                    });
                if to_vec {
                    self.collected(&name.ident.to_string(), &init.expr);
                }
            }
        }
        syn::visit::visit_local(self, local);
    }
}

impl Leaks<'_, '_> {
    /// A `Vec` named `name` collected from a chain that starts at a hash collection.
    fn collected(&mut self, name: &str, expr: &syn::Expr) {
        let mut cur = expr;
        let mut root = None;
        while let syn::Expr::MethodCall(m) = cur {
            if let Some(what) = self.hashes.iterates_hash(cur) {
                root = Some((what, m));
                break;
            }
            cur = &m.receiver;
        }
        let Some((what, call)) = root else {
            return;
        };
        if self.sorts(name) {
            return;
        }
        let (line, column) = (
            call.method.span().start().line,
            call.method.span().start().column + 1,
        );
        self.hits.push((
            (line, column),
            format!("`{name}` is collected from the hash collection `{what}`, so its order changes from run to run; use a BTreeMap/BTreeSet or sort it"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/sync_manager/src/manager.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/deterministic-iteration/dont.rs"),
            )],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/deterministic-iteration/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_fields_and_collected_vecs() {
        let src = "struct Gen { types: HashMap<String, Type> }\n\
            impl Gen {\n\
                fn render(&self, out: &mut String) {\n\
                    for (name, t) in &self.types {\n\
                        writeln!(out, \"{name}: {t}\").unwrap();\n\
                    }\n\
                }\n\
                fn names(&self) -> Vec<String> {\n\
                    let names: Vec<String> = self.types.keys().cloned().collect();\n\
                    names\n\
                }\n\
            }\n";
        let found = check(&RULE, &[("components/support/nimbus-fml/src/gen.rs", src)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 4);
        assert_eq!(found[1].line, 9);
    }

    #[test]
    fn accepts_sorted_or_order_free_uses_and_other_crates() {
        let src = "fn f(m: &HashMap<String, u32>) -> Vec<String> {\n\
                let mut v = Vec::new();\n\
                for k in m.keys() { v.push(k.clone()); }\n\
                v.sort();\n\
                let mut total = 0;\n\
                for x in m.values() { total += x; }\n\
                let mut other = HashMap::new();\n\
                for (k, x) in m { other.insert(k, x); }\n\
                for (k, x) in m { let mut per_key = Vec::new(); per_key.push(x); other.insert(k, per_key); }\n\
                v\n\
            }\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
        let leak = "fn f(m: &HashSet<String>) -> Vec<String> { let mut v = Vec::new(); for k in m { v.push(k.clone()); } v }";
        assert!(check(&RULE, &[("components/places/src/a.rs", leak)]).is_empty());
        assert_eq!(check(&RULE, &[(PATH, leak)]).len(), 1);
    }
}
