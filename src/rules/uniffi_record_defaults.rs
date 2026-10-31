//! Optional/new Record fields get a UniFFI default that matches Rust's `Default`; avoid `Option<Vec<_>>`
//!
//! Docs and examples: `rules/uniffi-record-defaults/`.
//!
//! Only records the foreign side builds are checked: those reachable from an
//! argument of an exported function, method or constructor (or returned by a
//! foreign-implemented trait). Records Rust only returns are left alone. A
//! missing default on an `Option` field is only reported when the record
//! already defaults other fields: a record without any defaults may want
//! callers to decide every value.
//! Fields added since `main` without a default are `uniffi-api-break-gate`'s
//! job.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::uniffi::{self as api, Field, Item, Shape};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;
use crate::uniffi::ApiKind;

pub static RULE: Rule = Rule {
    id: "uniffi-record-defaults",
    area: Area::Uniffi,
    title: "Optional/new Record fields get a UniFFI default that matches Rust's `Default`; avoid `Option<Vec<_>>`",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let mut items: Vec<Item> = Vec::new();
    for file in cx.ws.files() {
        if !api::is_shipped_api_file(&file.path) {
            continue;
        }
        if file.extension() == "udl" {
            items.extend(api::udl_items(&file.path, &file.text));
        } else if let Some(rust) = cx.ws.rust(&file.path) {
            items.extend(api::rust_items(rust));
        }
    }
    let defaults = rust_defaults(cx);
    let items = api::merge(items);
    for name in inputs(&items) {
        let Some(record) = items.get(&name) else {
            continue;
        };
        if record.kind != ApiKind::Record {
            continue;
        }
        let Some(file) = cx.ws.file(&record.path) else {
            continue;
        };
        let lines: Vec<&str> = file.text.lines().collect();
        // A record that defaults some fields is meant to be built without
        // spelling everything out; one that defaults none may want callers
        // to decide every value.
        let uses_defaults = record
            .members
            .iter()
            .any(|m| matches!(&m.shape, Shape::Field(f) if f.default.is_some()));
        for member in &record.members {
            let Shape::Field(field) = &member.shape else {
                continue;
            };
            let at = |out: &mut Findings, message: String| {
                out.add(&member.path, member.line, message);
            };
            if let Some(inner) = optional_collection(&field.ty) {
                if !explains_none(&doc_above(&lines, member.line)) {
                    at(
                        out,
                        format!(
                            "`{}.{}` is an optional {inner}, so callers must tell `None` from empty; use a plain {inner} that is empty when there is nothing, or document what `None` means",
                            record.name, field.name
                        ),
                    );
                    continue;
                }
            }
            if uses_defaults && field.ty.starts_with("Option<") && field.default.is_none() {
                at(
                    out,
                    format!(
                        "optional field `{}.{}` has no UniFFI default while other fields of the record do, so Kotlin/Swift callers must still pass `null`/`nil` for it; add `#[uniffi(default)]` (`= null` in UDL)",
                        record.name, field.name
                    ),
                );
                continue;
            }
            if let Some(mismatch) = default_mismatch(&defaults, &record.name, field) {
                at(out, mismatch);
            }
        }
    }
}

/// Names of the types the foreign side constructs: argument types of
/// exported functions, return types of foreign-implemented traits, and
/// everything reachable from them through record fields and variants.
fn inputs(items: &BTreeMap<String, Item>) -> BTreeSet<String> {
    let mut todo: Vec<String> = Vec::new();
    for item in items.values() {
        for member in &item.members {
            let Shape::Fn(sig) = &member.shape else {
                continue;
            };
            if item.foreign {
                todo.extend(api::type_names(&sig.ret).map(str::to_string));
            } else {
                for arg in &sig.args {
                    todo.extend(api::type_names(&arg.ty).map(str::to_string));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(item) = items.get(&name) else {
            continue;
        };
        for member in &item.members {
            match &member.shape {
                Shape::Field(f) => todo.extend(api::type_names(&f.ty).map(str::to_string)),
                Shape::Variant(fields) => {
                    for f in fields {
                        todo.extend(api::type_names(&f.ty).map(str::to_string));
                    }
                }
                Shape::Fn(_) => {}
            }
        }
    }
    seen
}

/// `Vec` or `map` for `Option<Vec<..>>` / `Option<Map<..>>`.
fn optional_collection(ty: &str) -> Option<&'static str> {
    let inner = ty.strip_prefix("Option<")?;
    if inner.starts_with("Vec<") && !inner.starts_with("Vec<u8>") {
        Some("`Vec`")
    } else if inner.starts_with("Map<") {
        Some("map")
    } else {
        None
    }
}

/// The `///` lines right above `line` (past attributes), joined.
fn doc_above(lines: &[&str], line: usize) -> String {
    let mut doc = Vec::new();
    let mut l = line.saturating_sub(1);
    while l >= 1 {
        let text = lines.get(l - 1).map(|t| t.trim()).unwrap_or("");
        if let Some(d) = text.strip_prefix("///") {
            doc.push(d.trim().to_lowercase());
        } else if !(text.starts_with("#[") || (text.starts_with('[') && text.ends_with(']'))) {
            break;
        }
        l -= 1;
    }
    doc.join(" ")
}

/// Whether a docstring says what an absent value means.
fn explains_none(doc: &str) -> bool {
    [
        "none", "null", "nil", "empty", "absent", "missing", "unset", "not set", "optional",
        "if not", "default",
    ]
    .iter()
    .any(|w| doc.contains(w))
}

/// Field values of each struct's `Default`: struct name -> field -> value.
/// A derived `Default` gives `None` for the fields it can't name.
fn rust_defaults(cx: &Context) -> BTreeMap<String, BTreeMap<String, Option<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, Option<String>>> = BTreeMap::new();
    for file in cx.ws.rust_files() {
        if !api::is_shipped_api_file(&file.path) {
            continue;
        }
        for item in syntax::all_items(&file.ast) {
            match item {
                syn::Item::Struct(s) if syntax::has_derive(&s.attrs, "Default") => {
                    let fields = s
                        .fields
                        .iter()
                        .filter_map(|f| {
                            let name = f.ident.as_ref()?.to_string();
                            Some((name, derived_default(&api::rust_type(&f.ty))))
                        })
                        .collect();
                    out.insert(s.ident.to_string(), fields);
                }
                syn::Item::Impl(i) => {
                    let is_default = i.trait_.as_ref().is_some_and(|(_, p, _)| {
                        p.segments.last().is_some_and(|s| s.ident == "Default")
                    });
                    if !is_default {
                        continue;
                    }
                    let Some(name) = syntax::type_ident(&i.self_ty) else {
                        continue;
                    };
                    if let Some(fields) = manual_default(i) {
                        out.insert(name, fields);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// What `#[derive(Default)]` gives a field of normalized type `ty`, when known.
fn derived_default(ty: &str) -> Option<String> {
    let value = match ty {
        _ if ty.starts_with("Option<") => "None",
        "bool" => "false",
        "String" => "\"\"",
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" => "0",
        "f32" | "f64" => "0.0",
        _ if ty.starts_with("Vec<") => "[]",
        _ if ty.starts_with("Map<") => "{}",
        _ => return None,
    };
    Some(value.to_string())
}

/// The field values of `fn default() -> Self { Self { a: x, .. } }`.
fn manual_default(i: &syn::ItemImpl) -> Option<BTreeMap<String, Option<String>>> {
    let f = i.items.iter().find_map(|it| match it {
        syn::ImplItem::Fn(f) if f.sig.ident == "default" => Some(f),
        _ => None,
    })?;
    let expr = match f.block.stmts.last()? {
        syn::Stmt::Expr(e, None) => e,
        _ => return None,
    };
    let syn::Expr::Struct(s) = expr else {
        return None;
    };
    if s.rest.is_some() {
        return None; // `..Default::default()` hides some values
    }
    Some(
        s.fields
            .iter()
            .filter_map(|fv| match &fv.member {
                syn::Member::Named(name) => Some((name.to_string(), normalize(&fv.expr))),
                syn::Member::Unnamed(_) => None,
            })
            .collect(),
    )
}

/// A default value in a comparable form, when it is a plain constant.
fn normalize(expr: &syn::Expr) -> Option<String> {
    let text = syntax::compact(&syntax::tokens(expr));
    normalize_text(&text)
}

fn normalize_text(text: &str) -> Option<String> {
    let t = text.trim();
    let empty_string = [
        "\"\"",
        "String::new()",
        "\"\".to_string()",
        "\"\".into()",
        "String::from(\"\")",
        "\"\".to_owned()",
    ];
    if empty_string.contains(&t) {
        return Some("\"\"".into());
    }
    if ["Vec::new()", "vec![]", "[]"].contains(&t) {
        return Some("[]".into());
    }
    if ["HashMap::new()", "BTreeMap::new()", "{}"].contains(&t) {
        return Some("{}".into());
    }
    if t == "None" || t == "null" {
        return Some("None".into());
    }
    if t == "true" || t == "false" {
        return Some(t.into());
    }
    if let Some(inner) = t.strip_prefix("Some(").and_then(|r| r.strip_suffix(')')) {
        return normalize_text(inner).map(|v| format!("Some({v})"));
    }
    if let Some(s) = t
        .strip_suffix(".to_string()")
        .or_else(|| t.strip_suffix(".into()"))
    {
        if s.starts_with('"') {
            return Some(s.to_string());
        }
    }
    if t.starts_with('"') && t.ends_with('"') {
        return Some(t.to_string());
    }
    let number: String = t.chars().filter(|c| *c != '_').collect();
    let number = number.trim_end_matches(|c: char| c.is_alphabetic() && c != 'e');
    if !number.is_empty() && number.parse::<f64>().is_ok() {
        return Some(number.to_string());
    }
    None
}

/// A UniFFI default that differs from the record's Rust `Default`.
fn default_mismatch(
    defaults: &BTreeMap<String, BTreeMap<String, Option<String>>>,
    record: &str,
    field: &Field,
) -> Option<String> {
    let uniffi = field.default.as_deref()?;
    if uniffi.is_empty() {
        return None; // bare `#[uniffi(default)]` is the type's own default
    }
    let rust = defaults.get(record)?.get(&field.name)?.as_ref()?;
    let foreign = normalize_text(uniffi)?;
    // A UDL `= null` for an `Option` is `None`.
    if &foreign == rust {
        return None;
    }
    Some(format!(
        "`{record}.{}` defaults to `{uniffi}` for Kotlin/Swift but `{rust}` in Rust's `Default`; use one value so both sides agree",
        field.name
    ))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/query.rs";
    const STORE: (&str, &str) = (
        "components/suggest/src/store.rs",
        "#[uniffi::export]\nimpl SuggestStore {\n    pub fn query(&self, query: SuggestionQuery) -> Vec<Suggestion> { todo!() }\n}\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                STORE,
                (
                    PATH,
                    include_str!("../../rules/uniffi-record-defaults/dont.rs"),
                ),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![7, 8, 10], "{found:#?}");
        assert!(found[0].message.contains("no UniFFI default"));
        assert!(found[1].message.contains("optional `Vec`"));
        assert!(found[2]
            .message
            .contains("`true` for Kotlin/Swift but `false`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                STORE,
                (
                    PATH,
                    include_str!("../../rules/uniffi-record-defaults/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_records_without_any_defaults() {
        let src = "#[derive(uniffi::Record)]\npub struct SuggestionQuery { pub keyword: String, pub limit: Option<i32> }\n";
        assert!(check(&RULE, &[STORE, (PATH, src)]).is_empty());
    }

    #[test]
    fn ignores_records_only_returned() {
        let src = "#[derive(uniffi::Record)]\npub struct Suggestion { pub title: Option<String>, pub icons: Option<Vec<String>> }\n";
        assert!(check(&RULE, &[STORE, (PATH, src)]).is_empty());
    }

    #[test]
    fn accepts_documented_none_and_matching_manual_defaults() {
        let src = "#[derive(uniffi::Record)]\npub struct SuggestionQuery {\n    /// `None` means every provider.\n    #[uniffi(default = None)]\n    pub providers: Option<Vec<String>>,\n    #[uniffi(default = 60_000)]\n    pub timeout: u32,\n}\nimpl Default for SuggestionQuery {\n    fn default() -> Self { Self { providers: None, timeout: 60000 } }\n}\n";
        let found = check(&RULE, &[STORE, (PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn checks_udl_dictionaries_reached_through_other_records() {
        let udl = (
            "components/suggest/src/suggest.udl",
            "namespace suggest {\n    void ingest(IngestConstraints constraints);\n};\ndictionary IngestConstraints {\n    Limits limits;\n};\ndictionary Limits {\n    u32? max;\n    u32? min = null;\n};\ndictionary Unused {\n    u32? max;\n    u32? min = null;\n};\n",
        );
        let found = check(&RULE, &[udl]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![8], "{found:#?}");
    }
}
