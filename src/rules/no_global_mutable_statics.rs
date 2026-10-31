//! No `static Mutex<…>` caches for state that belongs in the DB or a struct
//!
//! Docs and examples: `rules/no-global-mutable-statics/`.
//!
//! Flags `static` items (and `lazy_static!` refs) whose type holds data
//! behind a `Mutex`, `RwLock` or `RefCell`, directly or inside
//! `LazyLock`/`OnceLock`/`Lazy`. Process-wide registries are left alone: the
//! allowlisted support crates, registries of `Weak` handles, callback
//! objects (`dyn Trait`) and plain `Mutex<()>` locks.

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "no-global-mutable-statics",
    area: Area::Concurrency,
    title: "No `static Mutex<…>` caches for state that belongs in the DB or a struct",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Process-wide registries by design: error reporting, log sinks, viaduct's
/// backend settings and OHTTP config, NSS's token lock.
const ALLOWED: &[&str] = &[
    "components/support/error/",
    "components/support/tracing/",
    "components/viaduct/",
    "components/support/rc_crypto/nss/",
];

const LOCKS: &[&str] = &["Mutex", "RwLock", "RefCell"];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        if ALLOWED.iter().any(|p| file.path.starts_with(p)) {
            continue;
        }
        let support = TestSupport::of(file);
        let mut v = Statics { hits: Vec::new() };
        v.visit_file(&file.ast);
        for (line, column, name, ty) in v.hits {
            if !support.contains(line) {
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!("global `{name}: {ty}` is mutable state shared by every store in the process; keep it in the struct that owns it or in the database"),
                );
            }
        }
    }
}

/// Whether a static of type `ty` (compact text) holds mutable data.
fn holds_mutable_data(ty: &str) -> bool {
    let words: Vec<&str> = ty
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .collect();
    let locked = words.iter().any(|w| LOCKS.contains(w));
    let handles = ty.contains("Weak<") || ty.contains("dyn ") || ty.ends_with("<()>");
    locked && !handles
}

struct Statics {
    hits: Vec<(usize, usize, String, String)>,
}

impl<'ast> Visit<'ast> for Statics {
    fn visit_item_static(&mut self, s: &'ast syn::ItemStatic) {
        let ty = syntax::type_string(&s.ty);
        if holds_mutable_data(&ty) {
            let start = s.ident.span().start();
            self.hits
                .push((start.line, start.column + 1, s.ident.to_string(), ty));
        }
        syn::visit::visit_item_static(self, s);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if syntax::macro_name(m) == "lazy_static" {
            // `static ref NAME: TYPE = EXPR;`, repeated.
            let tokens: Vec<proc_macro2::TokenTree> = m.tokens.clone().into_iter().collect();
            let mut i = 0;
            while i < tokens.len() {
                let is = |k: usize, word: &str| {
                    matches!(tokens.get(k), Some(proc_macro2::TokenTree::Ident(id)) if id == word)
                };
                if is(i, "static") && is(i + 1, "ref") {
                    if let Some(proc_macro2::TokenTree::Ident(name)) = tokens.get(i + 2) {
                        let ty: proc_macro2::TokenStream = tokens[i + 4..]
                            .iter()
                            .take_while(|t| !errors::is_punct(Some(t), '='))
                            .cloned()
                            .collect();
                        let ty = syntax::compact(&ty.to_string());
                        if holds_mutable_data(&ty) {
                            let start = name.span().start();
                            self.hits
                                .push((start.line, start.column + 1, name.to_string(), ty));
                        }
                    }
                }
                i += 1;
            }
        }
        syn::visit::visit_macro(self, m);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/yelp.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/no-global-mutable-statics/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/no-global-mutable-statics/do.rs")).is_empty());
    }

    #[test]
    fn flags_lazy_and_lazy_static_caches() {
        let src = "static CACHE: LazyLock<RwLock<HashMap<String, u64>>> = LazyLock::new(Default::default);\nlazy_static! {\n    static ref COUNTERS: RwLock<HashMap<usize, AtomicI64>> = RwLock::new(HashMap::new());\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 3]);
    }

    #[test]
    fn ignores_registries_callbacks_and_plain_locks() {
        let src = "static CALLBACK: RwLock<Option<Arc<dyn Telemetry>>> = RwLock::new(None);\nstatic LOCK: Mutex<()> = Mutex::new(());\nlazy_static! {\n    static ref STORE_FOR_MANAGER: Mutex<Weak<Store>> = Mutex::new(Weak::new());\n}\nstatic ENDPOINT: Lazy<Url> = Lazy::new(|| url!(\"https://x\"));\n";
        assert!(run(src).is_empty());
    }

    #[test]
    fn ignores_allowlisted_registries() {
        let src = "static GLOBAL_SETTINGS: Lazy<RwLock<Settings>> = Lazy::new(Default::default);\n";
        assert!(check(&RULE, &[("components/viaduct/src/settings.rs", src)]).is_empty());
    }
}
