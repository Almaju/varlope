//! Feature gates name declared features and sit on whole items, not scattered statements
//!
//! Docs and examples: `rules/cargo-feature-hygiene/`.

use std::collections::BTreeSet;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::workspace::Package;

pub static RULE: Rule = Rule {
    id: "cargo-feature-hygiene",
    area: Area::Deps,
    title: "Feature gates name declared features and sit on whole items, not scattered statements",
    kind: Kind::Code,
    clippy: &[],
    in_tests: true,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        if is_out_of_scope(&package.manifest) {
            continue;
        }
        let declared = declared_features(package);
        separator_style(cx, package, out);
        for file in cx.ws.package_files(package) {
            if let Some(rust) = cx.ws.rust(&file.path) {
                undeclared_features(rust, &declared, out);
            }
        }
        scattered_cfgs(cx, package, out);
    }
}

/// Explicit features, plus the implicit feature of each optional dependency
/// that no `dep:` entry hides.
fn declared_features(package: &Package) -> BTreeSet<String> {
    let mut features: BTreeSet<String> = package.features().into_iter().collect();
    let table = package.toml.get("features").and_then(|f| f.as_table());
    let uses_dep_syntax = |name: &str| {
        table.is_some_and(|t| {
            t.values()
                .filter_map(|v| v.as_array())
                .flatten()
                .any(|v| v.as_str() == Some(&format!("dep:{name}")))
        })
    };
    for dep in package.dependencies() {
        if dep.optional && !uses_dep_syntax(&dep.name) {
            features.insert(dep.name.clone());
        }
    }
    features
}

/// `feature = "x"` in `#[cfg]`, `#[cfg_attr]` and `cfg!` where `x` isn't a
/// feature of the crate: the code is silently compiled out (rustc's
/// `unexpected_cfgs`).
fn undeclared_features(file: &RustFile, declared: &BTreeSet<String>, out: &mut Findings) {
    let mut names: Vec<(usize, String)> = Vec::new();
    struct Cfgs<'a>(&'a mut Vec<(usize, String)>);
    impl<'ast> Visit<'ast> for Cfgs<'_> {
        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            let name = syntax::attr_name(attr);
            if name == "cfg" || name == "cfg_attr" {
                if let syn::Meta::List(list) = &attr.meta {
                    feature_names(list.tokens.clone(), self.0);
                }
            }
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            let name = syntax::macro_name(mac);
            if name == "cfg" || name == "cfg_if" {
                feature_names(mac.tokens.clone(), self.0);
            }
            syn::visit::visit_macro(self, mac);
        }
    }
    Cfgs(&mut names).visit_file(&file.ast);
    for (line, name) in names {
        if !declared.contains(&name) {
            out.add(
                &file.path,
                line,
                format!("feature `{name}` is not declared in this crate's Cargo.toml, so this code never compiles: fix the name or declare the feature"),
            );
        }
    }
}

/// Every `feature = "x"` in a token stream, with its line.
fn feature_names(tokens: TokenStream, out: &mut Vec<(usize, String)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(g) => feature_names(g.stream(), out),
            TokenTree::Ident(id) if id == "feature" => {
                let eq =
                    matches!(trees.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '=');
                if let (true, Some(TokenTree::Literal(lit))) = (eq, trees.get(i + 2)) {
                    let text = lit.to_string();
                    if let Some(name) = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
                        out.push((lit.span().start().line, name.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
}

/// A crate-private function with two or more `#[cfg(feature = "x")]` on its
/// parameters, statements or expressions, all for the same feature, whose
/// callers are all behind that feature already: the whole function can carry
/// the one attribute instead.
struct Candidate {
    path: String,
    line: usize,
    name: String,
    feature: String,
    count: usize,
}

fn scattered_cfgs(cx: &Context, package: &Package, out: &mut Findings) {
    let files: Vec<&RustFile> = cx
        .ws
        .package_files(package)
        .filter_map(|f| cx.ws.rust(&f.path))
        .filter(|f| !f.is_test_file())
        .collect();
    let gated_modules = gated_modules(&files);
    let mut candidates = Vec::new();
    let mut uses: Vec<(String, usize, String)> = Vec::new();
    let mut gated: Vec<(String, usize, usize, String)> = Vec::new();
    for file in &files {
        let mut v = Scan {
            file,
            candidates: &mut candidates,
            uses: &mut uses,
            gated: &mut gated,
            trait_impl: false,
        };
        v.visit_file(&file.ast);
    }
    for c in candidates {
        let module_gate = |path: &str| {
            gated_modules
                .iter()
                .any(|(dir, f)| *f == c.feature && path.starts_with(dir.as_str()))
        };
        let callers: Vec<&(String, usize, String)> =
            uses.iter().filter(|(_, _, name)| *name == c.name).collect();
        let all_gated = !callers.is_empty()
            && callers.iter().all(|(path, line, _)| {
                module_gate(path)
                    || gated
                        .iter()
                        .any(|(p, a, b, f)| p == path && a <= line && line <= b && *f == c.feature)
            });
        let fn_gated = module_gate(&c.path);
        if all_gated && !fn_gated {
            out.add(
                &c.path,
                c.line,
                format!(
                    "{} `#[cfg(feature = \"{}\")]` inside `{}`, whose callers are all behind that feature: put the one attribute on the fn instead",
                    c.count, c.feature, c.name
                ),
            );
        }
    }
}

/// Directories of modules declared as `#[cfg(feature = "x")] mod name;`,
/// with the feature: `("components/nimbus/src/stateful", "stateful")`.
fn gated_modules(files: &[&RustFile]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for file in files {
        let dir = file.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let name = file.path.rsplit('/').next().unwrap_or("");
        let stem = name.strip_suffix(".rs").unwrap_or(name);
        let base = if matches!(stem, "lib" | "main" | "mod") {
            dir.to_string()
        } else {
            format!("{dir}/{stem}")
        };
        let mut declared: Vec<(String, String)> = Vec::new();
        for item in &file.ast.items {
            match item {
                syn::Item::Mod(m) if m.content.is_none() => {
                    if let Some(feature) = positive_feature(&m.attrs) {
                        declared.push((m.ident.to_string(), feature));
                    }
                }
                syn::Item::Macro(m) if syntax::macro_name(&m.mac) == "cfg_if" => {
                    cfg_if_modules(m.mac.tokens.clone(), &mut declared);
                }
                _ => {}
            }
        }
        for (name, feature) in declared {
            out.push((format!("{base}/{name}/"), feature.clone()));
            out.push((format!("{base}/{name}.rs"), feature));
        }
        if let Some(feature) = positive_feature(&file.ast.attrs) {
            out.push((file.path.clone(), feature));
        }
    }
    out
}

/// `mod name;` declared in the `if #[cfg(feature = "x")] { … }` branch of a
/// `cfg_if!`: `(name, x)`.
fn cfg_if_modules(tokens: TokenStream, out: &mut Vec<(String, String)>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut feature: Option<String> = None;
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(g) if g.delimiter() == proc_macro2::Delimiter::Bracket => {
                let text = g.stream().to_string();
                feature = None;
                if text.starts_with("cfg") && !text.contains("not") && !text.contains("any") {
                    let mut names = Vec::new();
                    feature_names(g.stream(), &mut names);
                    feature = names.into_iter().next().map(|(_, n)| n);
                }
            }
            TokenTree::Group(g) if g.delimiter() == proc_macro2::Delimiter::Brace => {
                if let Some(f) = feature.take() {
                    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                    for w in inner.windows(3) {
                        if let (TokenTree::Ident(m), TokenTree::Ident(name), TokenTree::Punct(p)) =
                            (&w[0], &w[1], &w[2])
                        {
                            if m == "mod" && p.as_char() == ';' {
                                out.push((name.to_string(), f.clone()));
                            }
                        }
                    }
                }
            }
            TokenTree::Ident(id) if id == "else" => {
                // An `else` branch is the opposite of the feature.
                if !matches!(trees.get(i + 1), Some(TokenTree::Ident(n)) if n == "if") {
                    feature = None;
                }
            }
            _ => {}
        }
    }
}

/// The feature of `#[cfg(feature = "x")]` among `attrs` (not `not(..)`).
fn positive_feature(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("cfg") {
            return None;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return None;
        };
        let text = list.tokens.to_string();
        if text.contains("not") || text.contains("any") {
            return None;
        }
        let mut names = Vec::new();
        feature_names(list.tokens.clone(), &mut names);
        names.into_iter().next().map(|(_, n)| n)
    })
}

struct Scan<'a, 'f> {
    file: &'f RustFile,
    candidates: &'a mut Vec<Candidate>,
    /// `(path, line, name)` of each call or mention of a function name.
    uses: &'a mut Vec<(String, usize, String)>,
    /// `(path, first line, last line, feature)` of feature-gated nodes.
    gated: &'a mut Vec<(String, usize, usize, String)>,
    trait_impl: bool,
}

impl Scan<'_, '_> {
    fn gate<T: syn::spanned::Spanned>(&mut self, attrs: &[syn::Attribute], node: &T) {
        if let Some(feature) = positive_feature(attrs) {
            let span = node.span();
            self.gated.push((
                self.file.path.clone(),
                span.start().line,
                span.end().line,
                feature,
            ));
        }
    }

    fn function(
        &mut self,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
        sig: &syn::Signature,
        block: &syn::Block,
    ) {
        // Other crates may call a `pub` fn from either build: only crate-local
        // callers can be checked.
        let public = matches!(vis, syn::Visibility::Public(_));
        if public || self.trait_impl || attrs.iter().any(is_feature_cfg) {
            return;
        }
        let mut features: Vec<Option<String>> = Vec::new();
        for input in &sig.inputs {
            let attrs = match input {
                syn::FnArg::Receiver(r) => &r.attrs,
                syn::FnArg::Typed(t) => &t.attrs,
            };
            features.extend(
                attrs
                    .iter()
                    .filter(|a| is_feature_cfg(a))
                    .map(|a| positive_feature(std::slice::from_ref(a))),
            );
        }
        let mut inner = Inner {
            features: &mut features,
        };
        inner.visit_block(block);
        let Some(Some(first)) = features.first().cloned() else {
            return;
        };
        if features.len() >= 2
            && features
                .iter()
                .all(|f| f.as_deref() == Some(first.as_str()))
        {
            self.candidates.push(Candidate {
                path: self.file.path.clone(),
                line: self.file.line_of_node(&sig.ident),
                name: sig.ident.to_string(),
                feature: first,
                count: features.len(),
            });
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_, '_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            _ => &[],
        };
        self.gate(attrs, item);
        if let syn::Item::Impl(i) = item {
            let outer = std::mem::replace(&mut self.trait_impl, i.trait_.is_some());
            syn::visit::visit_item(self, item);
            self.trait_impl = outer;
            return;
        }
        syn::visit::visit_item(self, item);
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.function(&f.vis, &f.attrs, &f.sig, &f.block);
        syn::visit::visit_item_fn(self, f);
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.gate(&f.attrs, f);
        self.function(&f.vis, &f.attrs, &f.sig, &f.block);
        syn::visit::visit_impl_item_fn(self, f);
    }
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        match stmt {
            syn::Stmt::Local(l) => self.gate(&l.attrs, stmt),
            syn::Stmt::Expr(e, _) => {
                if let Some(attrs) = expr_attrs(e) {
                    self.gate(attrs, stmt);
                }
            }
            syn::Stmt::Macro(m) => self.gate(&m.attrs, stmt),
            syn::Stmt::Item(_) => {}
        }
        syn::visit::visit_stmt(self, stmt);
    }
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.gate(&arm.attrs, arm);
        syn::visit::visit_arm(self, arm);
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let line = call.method.span().start().line;
        self.uses
            .push((self.file.path.clone(), line, call.method.to_string()));
        syn::visit::visit_expr_method_call(self, call);
    }
    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if let Some(last) = path.path.segments.last() {
            let line = last.ident.span().start().line;
            self.uses
                .push((self.file.path.clone(), line, last.ident.to_string()));
        }
        syn::visit::visit_expr_path(self, path);
    }
}

/// The attributes of an expression statement, for the kinds that carry them.
fn expr_attrs(expr: &syn::Expr) -> Option<&[syn::Attribute]> {
    Some(match expr {
        syn::Expr::Call(e) => &e.attrs,
        syn::Expr::MethodCall(e) => &e.attrs,
        syn::Expr::If(e) => &e.attrs,
        syn::Expr::Match(e) => &e.attrs,
        syn::Expr::Block(e) => &e.attrs,
        syn::Expr::Assign(e) => &e.attrs,
        syn::Expr::Macro(e) => &e.attrs,
        syn::Expr::Return(e) => &e.attrs,
        syn::Expr::Path(e) => &e.attrs,
        syn::Expr::ForLoop(e) => &e.attrs,
        _ => return None,
    })
}

/// Feature cfgs in a body, not in nested items.
struct Inner<'a> {
    features: &'a mut Vec<Option<String>>,
}

impl<'ast> Visit<'ast> for Inner<'_> {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if is_feature_cfg(attr) {
            self.features
                .push(positive_feature(std::slice::from_ref(attr)));
        }
    }
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

fn is_feature_cfg(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("cfg") && {
        let mut names = Vec::new();
        if let syn::Meta::List(list) = &attr.meta {
            feature_names(list.tokens.clone(), &mut names);
        }
        !names.is_empty()
    }
}

/// Feature names mixing `-` and `_` in one crate: flag the odd ones out.
fn separator_style(cx: &Context, package: &Package, out: &mut Findings) {
    let features = package.features();
    let dashed: Vec<&String> = features.iter().filter(|f| f.contains('-')).collect();
    let underscored: Vec<&String> = features.iter().filter(|f| f.contains('_')).collect();
    let (odd, usual, sep) = match (dashed.len(), underscored.len()) {
        (0, _) | (_, 0) => return,
        (d, u) if d < u => (dashed, "_", '-'),
        (d, u) if u < d => (underscored, "-", '_'),
        _ => return,
    };
    for name in odd {
        let line = package.line_in_manifest(cx.ws, &format!("{name} ="));
        out.add(
            &package.manifest,
            line,
            format!(
                "feature `{name}` uses `{sep}` while this crate's other features use `{usual}`: rename it to `{}`",
                name.replace(sep, usual)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/nimbus/Cargo.toml",
        "[package]\nname = \"nimbus\"\n\n[features]\ndefault = [\"stateful\"]\nstateful = [\"dep:remote_settings\"]\n\n[dependencies]\nremote_settings = { path = \"../remote_settings\", optional = true }\nrkv = { version = \"0.20\", optional = true }\n",
    );
    const PATH: &str = "components/nimbus/src/client.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/cargo-feature-hygiene/dont.rs"),
                ),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/cargo-feature-hygiene/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn undeclared_features() {
        let text = "#[cfg(feature = \"statefull\")]\npub mod stateful;\n#[cfg(feature = \"rkv\")]\nmod rkv_impl;\n#[cfg_attr(feature = \"remote_settings\", derive(Debug))]\nstruct A;\nfn f() -> bool { cfg!(feature = \"stateful\") }\n";
        let found = check(&RULE, &[MANIFEST, (PATH, text)]);
        // `rkv` is an implicit feature; `remote_settings` is hidden by `dep:`.
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 5], "{found:#?}");
    }

    #[test]
    fn mixed_separators() {
        let manifest = "[package]\nname = \"logins\"\n\n[features]\nallow_empty_passwords = []\nfixup_invalid_guids = []\nwipe-on-decryption-error = []\n";
        let found = check(&RULE, &[("components/logins/Cargo.toml", manifest)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 7);
    }

    #[test]
    fn functions_shared_by_both_builds_are_fine() {
        let text = "fn f(\n    #[cfg(feature = \"stateful\")] a: u8,\n) {\n    #[cfg(feature = \"stateful\")]\n    g(a);\n}\npub fn caller() { f(1) }\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, text)]).is_empty());
    }

    #[test]
    fn callers_in_gated_modules() {
        let lib = "#[cfg(feature = \"stateful\")]\npub mod stateful;\nfn f(\n    #[cfg(feature = \"stateful\")] a: u8,\n) {\n    #[cfg(feature = \"stateful\")]\n    g(a);\n}\n";
        let user = "pub fn caller() { crate::f(1) }\n";
        let found = check(
            &RULE,
            &[
                MANIFEST,
                ("components/nimbus/src/lib.rs", lib),
                ("components/nimbus/src/stateful/client.rs", user),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn modules_declared_in_cfg_if() {
        let lib = "cfg_if::cfg_if! {\n    if #[cfg(feature = \"stateful\")] {\n        pub mod stateful;\n    } else {\n        pub mod stateless;\n    }\n}\nfn f(\n    #[cfg(feature = \"stateful\")] a: u8,\n) {\n    #[cfg(feature = \"stateful\")]\n    g(a);\n}\n";
        let user = "pub fn caller() { crate::f(1) }\n";
        let stateful = check(
            &RULE,
            &[
                MANIFEST,
                ("components/nimbus/src/lib.rs", lib),
                ("components/nimbus/src/stateful/client.rs", user),
            ],
        );
        assert_eq!(stateful.len(), 1, "{stateful:#?}");
        let stateless = check(
            &RULE,
            &[
                MANIFEST,
                ("components/nimbus/src/lib.rs", lib),
                ("components/nimbus/src/stateless/client.rs", user),
            ],
        );
        assert!(stateless.is_empty(), "{stateless:#?}");
    }

    #[test]
    fn one_gated_statement_is_fine() {
        let text = "fn f() {\n    #[cfg(feature = \"stateful\")]\n    a();\n    b();\n}\n#[cfg(test)]\nmod tests {}\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, text)]).is_empty());
    }
}
