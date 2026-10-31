//! Descriptive identifiers: no cryptic abbreviations, follow Rust API naming
//!
//! Docs and examples: `rules/naming/`.

use std::collections::BTreeSet;

use syn::visit::Visit;

use crate::helpers::api_mobile::is_production_rust;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::uniffi;

pub static RULE: Rule = Rule {
    id: "naming",
    area: Area::Api,
    title: "Descriptive identifiers: no cryptic abbreviations, follow Rust API naming",
    kind: Kind::Code,
    // `clippy::disallowed_names` is not listed: the native check below
    // reports the same names (plus the ones reviewers added), and listing
    // both would report each binding twice under `--clippy`.
    clippy: &[],
    in_tests: false,
    check,
};

/// Names reviewers called out, with what they stood for.
const CRYPTIC: &[(&str, &str)] = &[
    ("nm", "`name`"),
    ("og", "`original`"),
    ("th", "`targeting_helper`"),
    ("ta", "`targeting_attributes`"),
    ("vt", "what it holds (`value_type`, `variables_type`)"),
    ("fe", "a name that says what it holds"),
    ("ee", "a name that says what it holds"),
    ("tmp", "a name that says what it holds"),
    ("foo", "a name that says what it holds"),
];

fn check(cx: &Context, out: &mut Findings) {
    let exported = exported_type_names(cx);
    for file in cx.ws.rust_files() {
        if !is_production_rust(&file.path) || is_c_binding(&file.path) {
            continue;
        }
        // Acronym casing is checked on new names only (lines a PR adds):
        // renaming existing types is churn, and many mirror external names.
        let added: BTreeSet<usize> = cx
            .diff
            .and_then(|d| d.file(&file.path))
            .map(|c| c.added.iter().map(|(line, _)| *line).collect())
            .unwrap_or_default();
        let mut v = Names {
            exported: &exported,
            added,
            hits: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, column, message) in v.hits {
            out.add_at(&file.path, line, column, message);
        }
    }
}

/// UniFFI-exported type names: renaming one is a breaking change, so they
/// are left to `uniffi-names-read-well`.
fn exported_type_names(cx: &Context) -> BTreeSet<String> {
    uniffi::uniffi_packages(cx.ws)
        .into_iter()
        .flat_map(|p| uniffi::package_api(cx.ws, p))
        .flat_map(|item| std::iter::once(item.name).chain(item.members.into_iter().map(|m| m.name)))
        .collect()
}

struct Names<'a> {
    exported: &'a BTreeSet<String>,
    added: BTreeSet<usize>,
    hits: Vec<(usize, usize, String)>,
}

/// Bindings to C libraries keep the C names (`PK11SlotInfo`, `SECItem`).
fn is_c_binding(path: &str) -> bool {
    path.contains("/nss_sys/")
        || path.contains("/bindings/")
        || path.starts_with("components/support/rc_crypto/")
}

impl Names<'_> {
    fn binding(&mut self, ident: &syn::Ident) {
        let name = ident.to_string();
        if let Some((_, instead)) = CRYPTIC.iter().find(|(n, _)| *n == name) {
            let start = ident.span().start();
            self.hits.push((
                start.line,
                start.column + 1,
                format!("`{name}` is a cryptic name; use {instead}"),
            ));
        }
    }

    fn type_name(&mut self, ident: &syn::Ident) {
        let name = ident.to_string();
        if self.exported.contains(&name) || !self.added.contains(&ident.span().start().line) {
            return;
        }
        if let Some(fixed) = camel_acronyms(&name) {
            let start = ident.span().start();
            self.hits.push((
                start.line,
                start.column + 1,
                format!("`{name}` spells an acronym in capitals; Rust names treat it as a word: `{fixed}`"),
            ));
        }
    }
}

/// `JSONError` -> `JsonError`, `RelayAPI` -> `RelayApi`; `None` if fine.
fn camel_acronyms(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_uppercase() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_uppercase() {
                j += 1;
            }
            // In `JSONError` the run is `JSONE`: its last capital starts `Error`.
            let next_is_lower = chars.get(j).is_some_and(|c| c.is_ascii_lowercase());
            let acronym_end = if next_is_lower { j - 1 } else { j };
            if acronym_end - i >= 2 {
                out.push(chars[i]);
                out.extend(
                    chars[i + 1..acronym_end]
                        .iter()
                        .map(|c| c.to_ascii_lowercase()),
                );
                out.extend(&chars[acronym_end..j]);
                changed = true;
            } else {
                out.extend(&chars[i..j]);
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    (changed && name.chars().any(|c| c.is_ascii_lowercase())).then_some(out)
}

fn derives_serde(attrs: &[syn::Attribute]) -> bool {
    syntax::has_derive(attrs, "Serialize") || syntax::has_derive(attrs, "Deserialize")
}

impl<'ast> Visit<'ast> for Names<'_> {
    fn visit_pat_ident(&mut self, p: &'ast syn::PatIdent) {
        self.binding(&p.ident);
        syn::visit::visit_pat_ident(self, p);
    }

    fn visit_field(&mut self, f: &'ast syn::Field) {
        if let Some(ident) = &f.ident {
            self.binding(ident);
        }
        syn::visit::visit_field(self, f);
    }

    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        self.type_name(&s.ident);
        syn::visit::visit_item_struct(self, s);
    }

    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        self.type_name(&e.ident);
        // Without `rename`, serde writes variant names to the wire.
        if !derives_serde(&e.attrs) {
            for v in &e.variants {
                self.type_name(&v.ident);
            }
        }
        syn::visit::visit_item_enum(self, e);
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        self.type_name(&t.ident);
        syn::visit::visit_item_trait(self, t);
    }

    fn visit_item_type(&mut self, t: &'ast syn::ItemType) {
        self.type_name(&t.ident);
        syn::visit::visit_item_type(self, t);
    }
}

#[cfg(test)]
mod tests {
    use super::{camel_acronyms, RULE};
    use crate::testing::{check, check_diff};

    const PATH: &str = "components/support/nimbus-fml/src/defaults/validator.rs";

    #[test]
    fn flags_dont() {
        let found = check(&RULE, &[(PATH, include_str!("../../rules/naming/dont.rs"))]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 6], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(&RULE, &[(PATH, include_str!("../../rules/naming/do.rs"))]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_new_acronyms_in_a_diff_only() {
        let old = "pub enum Error { Io }\n";
        let new = "pub enum Error { Io, JSONDecode }\npub struct MARSClient;\n";
        let found = check_diff(&RULE, &[(PATH, old)], &[(PATH, new)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 2], "{found:#?}");
        assert!(found[0].message.contains("`JsonDecode`"));
        assert!(check(&RULE, &[(PATH, new)]).is_empty());
    }

    #[test]
    fn rewrites_acronyms() {
        assert_eq!(camel_acronyms("JSONError").as_deref(), Some("JsonError"));
        assert_eq!(camel_acronyms("RelayAPI").as_deref(), Some("RelayApi"));
        assert_eq!(camel_acronyms("IOError").as_deref(), Some("IoError"));
        assert_eq!(camel_acronyms("JsonError"), None);
        assert_eq!(camel_acronyms("AError"), None);
        assert_eq!(camel_acronyms("UTF8"), None); // all caps: a constant-like name
    }

    #[test]
    fn spares_exported_and_serde_names_and_tests() {
        let src = r#"
#[derive(uniffi::Error, Debug)]
pub enum FMLError { IOError }
#[derive(Serialize)]
enum Kind { JSON }
#[cfg(test)]
mod tests { fn t() { let nm = 1; } }
fn ok(db: &Db, e: Error, name: &str) {}
"#;
        let manifest = (
            "components/support/nimbus-fml/Cargo.toml",
            "[package]\nname = \"nimbus-fml\"\n[dependencies]\nuniffi = \"0.29\"\n",
        );
        let found = check(&RULE, &[manifest, (PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
