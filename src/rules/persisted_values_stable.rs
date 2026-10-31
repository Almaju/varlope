//! Persisted enum discriminants and hashes are stable
//!
//! Docs and examples: `rules/persisted-values-stable/`.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::sql;
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "persisted-values-stable",
    area: Area::Sql,
    title: "Persisted enum discriminants and hashes are stable",
    kind: Kind::Code,
    // `DefaultHasher` is in clippy's `disallowed-types` (varlope's clippy.toml).
    clippy: &["clippy::disallowed_types"],
    in_tests: false,
    check,
};

const INT_TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "usize", "isize",
];

/// A fieldless enum: `(path, name, variants without an explicit value)`.
struct Enum {
    path: String,
    name: String,
    implicit: Vec<(String, usize)>,
}

fn check(cx: &Context, out: &mut Findings) {
    // Per package: fieldless enums, and the names of types whose values are
    // stored or converted as integers.
    let mut enums: BTreeMap<String, Vec<Enum>> = BTreeMap::new();
    let mut as_int: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in cx.ws.rust_files() {
        if sql::skip_path(&file.path) || file.is_test_file() {
            continue;
        }
        let key = cx
            .ws
            .package_of(&file.path)
            .map(|p| p.manifest.clone())
            .unwrap_or_default();
        let mut v = Collect {
            file_path: &file.path,
            enums: Vec::new(),
            as_int: BTreeSet::new(),
        };
        v.visit_file(&file.ast);
        enums.entry(key.clone()).or_default().extend(
            v.enums
                .into_iter()
                .filter(|e| !file.in_test(e.implicit[0].1)),
        );
        as_int.entry(key).or_default().extend(v.as_int);
    }
    for (key, list) in &enums {
        let Some(stored) = as_int.get(key) else {
            continue;
        };
        for e in list.iter().filter(|e| stored.contains(&e.name)) {
            let (variant, line) = &e.implicit[0];
            out.add(
                &e.path,
                *line,
                format!(
                    "`{}` is stored or converted as an integer, but `{variant}` has no explicit value; give every variant a fixed `= n` so reordering cannot change stored data",
                    e.name
                ),
            );
        }
    }
}

struct Collect<'a> {
    file_path: &'a str,
    enums: Vec<Enum>,
    /// Types converted to integers: `X::A as u8`, or a `ToSql`/`FromSql`
    /// impl that goes through an integer.
    as_int: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for Collect<'_> {
    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        let fieldless = !e.variants.is_empty()
            && e.variants
                .iter()
                .all(|v| matches!(v.fields, syn::Fields::Unit));
        let implicit: Vec<(String, usize)> = e
            .variants
            .iter()
            .filter(|v| v.discriminant.is_none())
            .map(|v| (v.ident.to_string(), v.ident.span().start().line))
            .collect();
        if fieldless && !implicit.is_empty() && !syntax::is_test_gated(&e.attrs) {
            self.enums.push(Enum {
                path: self.file_path.to_string(),
                name: e.ident.to_string(),
                implicit,
            });
        }
        syn::visit::visit_item_enum(self, e);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let sql_trait = i.trait_.as_ref().is_some_and(|(_, p, _)| {
            p.segments
                .last()
                .is_some_and(|s| s.ident == "ToSql" || s.ident == "FromSql")
        });
        if sql_trait {
            let body = i
                .items
                .iter()
                .map(syntax::tokens)
                .collect::<Vec<_>>()
                .join(" ");
            let through_int = INT_TYPES.iter().any(|t| body.contains(&format!("as {t}")))
                || body.contains("as_i64")
                || body.contains("from_u8")
                || body.contains("as_raw");
            if let (true, Some(name)) = (through_int, syntax::type_ident(&i.self_ty)) {
                self.as_int.insert(name);
            }
        }
        syn::visit::visit_item_impl(self, i);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if let Ok(args) = m.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            for e in &args {
                self.visit_expr(e);
            }
        }
    }

    fn visit_expr_cast(&mut self, c: &'ast syn::ExprCast) {
        let to_int = syntax::type_ident(&c.ty).is_some_and(|t| INT_TYPES.contains(&t.as_str()));
        if let (true, syn::Expr::Path(p)) = (to_int, &*c.expr) {
            let segments: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segments.len() >= 2 {
                self.as_int.insert(segments[segments.len() - 2].clone());
            }
        }
        syn::visit::visit_expr_cast(self, c);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/provider.rs";

    #[test]
    fn flags_dont() {
        let code = include_str!("../../rules/persisted-values-stable/dont.rs");
        let found = check(&RULE, &[(PATH, code)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let code = include_str!("../../rules/persisted-values-stable/do.rs");
        assert!(check(&RULE, &[(PATH, code)]).is_empty());
    }

    #[test]
    fn enums_stored_by_name_or_not_at_all_are_fine() {
        let code = r#"
pub enum Kind { A, B }
impl ToSql for Kind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(match self { Kind::A => "a", Kind::B => "b" }))
    }
}
#[repr(u8)]
pub enum Method { Get, Post }
"#;
        assert!(check(&RULE, &[(PATH, code)]).is_empty());
    }

    #[test]
    fn casts_count_as_storage() {
        let code = "pub enum SyncStatus { New, Normal }\nfn f() -> String { format!(\"UPDATE t SET s = {}\", SyncStatus::Normal as u8) }\n";
        assert_eq!(check(&RULE, &[(PATH, code)]).len(), 1);
    }
}
