//! `build.rs` writes only to `OUT_DIR`, doesn't shell out, fails with actionable messages
//!
//! Docs and examples: `rules/build-rs-hygiene/`.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::docs_deps::is_out_of_scope;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "build-rs-hygiene",
    area: Area::Deps,
    title: "`build.rs` writes only to `OUT_DIR`, doesn't shell out, fails with actionable messages",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Functions that write files; the first argument is the destination.
const WRITERS: &[&str] = &[
    "fs::write",
    "File::create",
    "fs::copy",
    "fs::create_dir_all",
    "fs::create_dir",
    "fs::rename",
];

/// Variables Cargo always sets for build scripts.
fn set_by_cargo(name: &str) -> bool {
    name.starts_with("CARGO")
        || matches!(
            name,
            "OUT_DIR"
                | "TARGET"
                | "HOST"
                | "PROFILE"
                | "OPT_LEVEL"
                | "DEBUG"
                | "NUM_JOBS"
                | "RUSTC"
                | "RUSTDOC"
                | "RUSTC_LINKER"
        )
}

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if file.path.rsplit('/').next() != Some("build.rs") || is_out_of_scope(&file.path) {
            continue;
        }
        let mut visitor = BuildScript {
            file,
            out_dir_vars: out_dir_vars(file),
            hits: Vec::new(),
        };
        visitor.visit_file(&file.ast);
        for (line, message) in visitor.hits {
            out.add(&file.path, line, message);
        }
    }
}

/// Local names whose value comes from `OUT_DIR` (directly or through another
/// such name): `let out = env::var("OUT_DIR")?; let dest = Path::new(&out).join(..)`.
fn out_dir_vars(file: &RustFile) -> BTreeSet<String> {
    struct Lets<'f> {
        file: &'f RustFile,
        lets: Vec<(String, String)>,
    }
    impl<'ast> Visit<'ast> for Lets<'_> {
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let (syn::Pat::Ident(p), Some(init)) = (&local.pat, &local.init) {
                let text = self.file.text_of(&*init.expr).to_string();
                self.lets.push((p.ident.to_string(), text));
            }
            syn::visit::visit_local(self, local);
        }
    }
    let mut lets = Lets {
        file,
        lets: Vec::new(),
    };
    lets.visit_file(&file.ast);
    let mut tainted = BTreeSet::new();
    loop {
        let before = tainted.len();
        for (name, init) in &lets.lets {
            if init.contains("OUT_DIR") || mentions_any(init, &tainted) {
                tainted.insert(name.clone());
            }
        }
        if tainted.len() == before {
            return tainted;
        }
    }
}

/// Whether `text` uses one of `names` as a whole identifier.
fn mentions_any(text: &str, names: &BTreeSet<String>) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| names.contains(word))
}

struct BuildScript<'f> {
    file: &'f RustFile,
    out_dir_vars: BTreeSet<String>,
    hits: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for BuildScript<'_> {
    /// Look inside `println!`, `format!`, `assert!`... arguments too.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        use syn::parse::Parser;
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = parser.parse2(mac.tokens.clone()) {
            for arg in &args {
                self.visit_expr(arg);
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*call.func {
            let line = self.file.line_of_node(call);
            if syntax::path_ends_with(&func.path, "Command::new") {
                self.hits.push((
                    line,
                    "shells out from build.rs, which depends on the developer's PATH and platform: do it with a Rust build-dependency (e.g. `uniffi::generate_scaffolding`, `glean_build`)".into(),
                ));
            }
            if let Some(writer) = WRITERS
                .iter()
                .find(|w| syntax::path_ends_with(&func.path, w))
            {
                let dest = call
                    .args
                    .first()
                    .map(|a| self.file.text_of(a).to_string())
                    .unwrap_or_default();
                if !dest.contains("OUT_DIR") && !mentions_any(&dest, &self.out_dir_vars) {
                    self.hits.push((
                        line,
                        format!("`{writer}` outside `OUT_DIR`: build scripts must only write under `env::var(\"OUT_DIR\")`, never into src/ or the crate dir"),
                    ));
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "unwrap" {
            if let Some(var) = env_var_name(&call.receiver) {
                if !set_by_cargo(&var) {
                    self.hits.push((
                        self.file.line_of_node(call),
                        format!("`env::var(\"{var}\").unwrap()` panics with no hint when `{var}` is unset: use `.expect()` saying what to set and how, or fall back"),
                    ));
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// `"X"` for `env::var("X")` / `std::env::var("X")`.
fn env_var_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    let syn::Expr::Path(func) = &*call.func else {
        return None;
    };
    if !syntax::path_ends_with(&func.path, "env::var") && !func.path.is_ident("var") {
        return None;
    }
    match call.args.first()? {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.value()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/notes/build.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/build-rs-hygiene/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 9, 10], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/build-rs-hygiene/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn writes_under_out_dir_are_fine() {
        let text = r#"
use std::{env, fs, path::Path};
fn main() {
    let out = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out).join("version.rs");
    fs::write(&dest, "pub const V: u32 = 1;").unwrap();
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("x"), "").unwrap();
    let profile = env::var("PROFILE").unwrap();
    println!("cargo:rerun-if-changed=build.rs {profile}");
}
"#;
        assert!(check(&RULE, &[(PATH, text)]).is_empty());
    }

    #[test]
    fn only_build_scripts() {
        let text = "fn main() { std::process::Command::new(\"git\").status().unwrap(); }";
        assert!(check(&RULE, &[("components/notes/src/bin/tool.rs", text)]).is_empty());
    }
}
