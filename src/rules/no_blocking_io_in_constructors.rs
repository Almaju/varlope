//! No disk/network I/O in constructors of exported objects
//!
//! Docs and examples: `rules/no-blocking-io-in-constructors/`.
//!
//! A review hint. Constructors are `#[uniffi::constructor]` methods and the
//! Rust functions behind UDL `constructor(..)`s (`new`, or the `[Name=..]`).
//! The rule points at those that open a database, touch the file system,
//! open RKV or send a request, directly or through a helper of the same
//! package one call deep. `LazyDb::new(..)` only stores its arguments.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::errors;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::udl;

pub static RULE: Rule = Rule {
    id: "no-blocking-io-in-constructors",
    area: Area::Concurrency,
    title: "No disk/network I/O in constructors of exported objects",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        if !package.dir.starts_with("components/") {
            continue;
        }
        // `(type, fn name)` of each UDL constructor.
        let mut udl_ctors: BTreeSet<(String, String)> = BTreeSet::new();
        for file in cx.ws.package_files(package).filter(|f| f.extension() == "udl") {
            for item in udl::parse(&file.text) {
                for m in item.members.iter().filter(|m| m.name == "constructor") {
                    let name = m
                        .attrs
                        .split(',')
                        .find_map(|a| a.trim().strip_prefix("Name="))
                        .map(|n| n.trim().trim_matches('"').to_string())
                        .unwrap_or_else(|| "new".to_string());
                    udl_ctors.insert((item.name.clone(), name));
                }
            }
        }
        let rust: Vec<_> = cx
            .ws
            .package_files(package)
            .filter_map(|f| cx.ws.rust(&f.path))
            .filter(|f| errors::is_shipped(&f.path))
            .collect();
        // Every function of the package, as `Type::name` or `name`, for
        // one-call-deep lookups.
        let mut bodies: BTreeMap<String, Vec<&syn::Block>> = BTreeMap::new();
        for file in &rust {
            for f in errors::functions(&file.ast) {
                let key = match &f.self_ty {
                    Some(t) => format!("{t}::{}", f.sig.ident),
                    None => f.sig.ident.to_string(),
                };
                bodies.entry(key).or_default().push(f.block);
            }
        }
        for file in &rust {
            for f in errors::functions(&file.ast) {
                let name = f.sig.ident.to_string();
                let is_ctor = syntax::has_attr(f.attrs, "uniffi::constructor")
                    || f
                        .self_ty
                        .as_ref()
                        .is_some_and(|t| udl_ctors.contains(&(t.clone(), name.clone())));
                if !is_ctor {
                    continue;
                }
                let ty = f.self_ty.clone().unwrap_or_default();
                for (line, column, what, via) in io_calls(f.block, &bodies) {
                    let how = match via {
                        Some(helper) => format!("through `{helper}`, "),
                        None => String::new(),
                    };
                    out.add_at(
                        &file.path,
                        line,
                        column,
                        format!("the `{ty}::{name}` constructor does I/O ({how}`{what}`), which blocks whichever thread the app constructs it on; look at opening lazily (`LazyDb`) or in a method"),
                    );
                }
            }
        }
    }
}

/// Calls that do disk or network I/O.
fn io_call(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Call(c) => {
            let syn::Expr::Path(p) = &*c.func else {
                return None;
            };
            let path = syntax::path_string(&p.path);
            let last = p.path.segments.last()?.ident.to_string();
            let io = last.starts_with("open_database")
                || path.ends_with("Connection::open")
                || path.ends_with("Connection::open_with_flags")
                || path.starts_with("fs::")
                || path.starts_with("std::fs::")
                || path.ends_with("File::open")
                || path.ends_with("File::create")
                || path.ends_with("Rkv::new")
                || path.contains("rkv::");
            io.then_some(path)
        }
        syn::Expr::MethodCall(m) => errors::is_network_call(m).then(|| format!(".{}()", m.method)),
        _ => None,
    }
}

/// Helpers worth looking into: `open*`, `load*`, `read*`, `from_persisted*`,
/// and the constructor of a database type (`AutofillDb::new`).
fn is_io_helper(path: &str) -> bool {
    let (ty, name) = path.rsplit_once("::").unwrap_or(("", path));
    ["open", "load", "read", "from_persisted"]
        .iter()
        .any(|p| name.starts_with(p))
        || (name == "new" && (ty.ends_with("Db") || ty.ends_with("Database")) && ty != "LazyDb")
}

/// The first I/O call in a helper's body, or in the `Self::..` functions it
/// calls (`AutofillDb::new` -> `Self::new_named` -> `open_database`).
fn helper_io(helper: &str, bodies: &BTreeMap<String, Vec<&syn::Block>>) -> Option<String> {
    let (ty, name) = helper.rsplit_once("::").unwrap_or(("", helper));
    // `S::open` on a generic: any function of that name in the package.
    let found: Vec<&syn::Block> = match bodies.get(helper) {
        Some(b) => b.clone(),
        None if name != "new" => bodies
            .iter()
            .filter(|(k, _)| k.rsplit("::").next() == Some(name))
            .flat_map(|(_, b)| b.iter().copied())
            .collect(),
        None => Vec::new(),
    };
    for body in found {
        let mut direct = None;
        let mut selfs = Vec::new();
        struct V<'o>(&'o mut Option<String>, &'o mut Vec<String>);
        impl<'a> Visit<'a> for V<'_> {
            fn visit_expr(&mut self, e: &'a syn::Expr) {
                if self.0.is_none() {
                    *self.0 = io_call(e);
                }
                if let syn::Expr::Call(c) = e {
                    if let syn::Expr::Path(p) = &*c.func {
                        let segs: Vec<String> =
                            p.path.segments.iter().map(|s| s.ident.to_string()).collect();
                        if let [first, name] = segs.as_slice() {
                            if first == "Self" {
                                self.1.push(name.clone());
                            }
                        }
                    }
                }
                syn::visit::visit_expr(self, e);
            }
            fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {}
        }
        V(&mut direct, &mut selfs).visit_block(body);
        if direct.is_some() {
            return direct;
        }
        for inner in selfs {
            for b in bodies.get(&format!("{ty}::{inner}")).into_iter().flatten() {
                let mut direct = None;
                V(&mut direct, &mut Vec::new()).visit_block(b);
                if direct.is_some() {
                    return direct;
                }
            }
        }
    }
    None
}

/// I/O calls in a constructor body, and through its helpers one call deep:
/// `(line, column, call, helper)`.
fn io_calls(
    body: &syn::Block,
    bodies: &BTreeMap<String, Vec<&syn::Block>>,
) -> Vec<(usize, usize, String, Option<String>)> {
    #[derive(Default)]
    struct V {
        direct: Vec<(usize, usize, String)>,
        helpers: Vec<(usize, usize, String)>,
    }
    impl<'a> Visit<'a> for V {
        fn visit_expr(&mut self, e: &'a syn::Expr) {
            let start = syn::spanned::Spanned::span(e).start();
            if let Some(what) = io_call(e) {
                self.direct.push((start.line, start.column + 1, what));
            } else {
                // `Type::helper(..)` or `helper(..)`, as written.
                let callee = match e {
                    syn::Expr::Call(c) => match &*c.func {
                        syn::Expr::Path(p) => {
                            let segs: Vec<String> =
                                p.path.segments.iter().map(|s| s.ident.to_string()).collect();
                            Some(segs[segs.len().saturating_sub(2)..].join("::"))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(callee) = callee.filter(|c| is_io_helper(c)) {
                    self.helpers.push((start.line, start.column + 1, callee));
                }
            }
            syn::visit::visit_expr(self, e);
        }
        fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {
            // Runs later (`LazyDb` initializers, callbacks).
        }
        fn visit_item(&mut self, _: &'a syn::Item) {}
    }
    let mut v = V::default();
    v.visit_block(body);
    let mut out: Vec<_> = v
        .direct
        .into_iter()
        .map(|(l, c, w)| (l, c, w, None))
        .collect();
    for (line, column, helper) in v.helpers {
        if let Some(what) = helper_io(&helper, bodies) {
            out.push((line, column, what, Some(helper)));
        }
    }
    out.sort();
    out.dedup_by_key(|h| h.0);
    out
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = ("components/store/Cargo.toml", "[package]\nname = \"store\"\n");
    const PATH: &str = "components/store/src/store.rs";

    fn run(files: &[(&str, &str)]) -> Vec<crate::Finding> {
        let mut all = vec![MANIFEST];
        all.extend_from_slice(files);
        check(&RULE, &all)
    }

    #[test]
    fn flags_dont() {
        let found = run(&[(PATH, include_str!("../../rules/no-blocking-io-in-constructors/dont.rs"))]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 6);
    }

    #[test]
    fn accepts_do() {
        assert!(run(&[(PATH, include_str!("../../rules/no-blocking-io-in-constructors/do.rs"))]).is_empty());
    }

    #[test]
    fn follows_udl_constructors_and_db_helpers() {
        let udl = ("components/store/src/store.udl", "interface Store {\n    constructor(string path);\n};\n");
        let src = "impl Store {\n    pub fn new(path: String) -> Result<Self> {\n        Ok(Self { db: StoreDb::new(path)? })\n    }\n}\nimpl StoreDb {\n    pub fn new(path: String) -> Result<Self> {\n        Self::open_named(path)\n    }\n    fn open_named(path: String) -> Result<Self> {\n        Ok(Self(open_database_with_flags(path, FLAGS)?))\n    }\n}\n";
        let found = run(&[udl, (PATH, src)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn ignores_plain_constructors_and_methods() {
        let src = "#[uniffi::export]\nimpl Store {\n    #[uniffi::constructor]\n    pub fn new(path: String) -> Self { Self { path } }\n    pub fn open(&self) -> Result<()> { std::fs::create_dir_all(&self.path)?; Ok(()) }\n}\n";
        assert!(run(&[(PATH, src)]).is_empty());
    }
}
