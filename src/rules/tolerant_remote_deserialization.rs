//! Types deserialized from server/Remote Settings data tolerate absent and unknown values
//!
//! Docs and examples: `rules/tolerant-remote-deserialization/`.
//!
//! In the crates that read server or Remote Settings data, looks at
//! `Deserialize` types: `#[serde(deny_unknown_fields)]`; fieldless enums
//! used in a field of another `Deserialize` type of the crate, without an
//! `#[serde(other)]` fallback or whose fallback is a real value;
//! and `#[serde(untagged)]` enums where an earlier struct variant's fields
//! are a subset of a later one's, so the later one never matches. With a PR
//! diff (`--base`), also a newly added field that is neither `Option` nor
//! `#[serde(default)]`: older records and servers don't send it.

use std::collections::BTreeSet;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "tolerant-remote-deserialization",
    area: Area::Remote,
    title: "Types deserialized from server/Remote Settings data tolerate absent and unknown values",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Crates that deserialize data a newer server can change under them.
const REMOTE_CRATES: &[&str] = &[
    "components/remote_settings/",
    "components/suggest/",
    "components/search/",
    "components/merino/",
    "components/ads-client/",
    "components/relevancy/",
];

fn check(cx: &Context, out: &mut Findings) {
    let files: Vec<&RustFile> = errors::shipped_files(cx.ws)
        .filter(|f| REMOTE_CRATES.iter().any(|c| f.path.starts_with(c)))
        .collect();
    // Type names that appear in a field of a `Deserialize` type, per crate.
    let mut nested: BTreeSet<(String, String)> = BTreeSet::new();
    for file in &files {
        let krate = errors::component_of(&file.path).unwrap_or_default().to_string();
        for name in deserialized_field_types(&file.ast) {
            nested.insert((krate.clone(), name));
        }
    }
    for file in files {
        let krate = errors::component_of(&file.path).unwrap_or_default().to_string();
        let support = TestSupport::of(file);
        let added: BTreeSet<usize> = cx
            .diff
            .and_then(|d| d.file(&file.path))
            .map(|f| f.added.iter().map(|(n, _)| *n).collect())
            .unwrap_or_default();
        let mut hits = Vec::new();
        let is_nested = |name: &str| nested.contains(&(krate.clone(), name.to_string()));
        check_file(file, &added, &is_nested, &mut hits);
        for (line, column, message) in hits {
            if !support.contains(line) {
                out.add_at(&file.path, line, column, message);
            }
        }
    }
}

/// The text of every `#[serde(..)]` attribute, joined.
fn serde_args(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("serde"))
        .map(syntax::attr_args)
        .collect::<Vec<_>>()
        .join(",")
}

fn has_serde(attrs: &[syn::Attribute], word: &str) -> bool {
    serde_args(attrs)
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// Words that name a "don't know" value.
fn is_unknown_name(name: &str) -> bool {
    let n = name.to_lowercase();
    ["unknown", "other", "unsupported", "unrecognized", "invalid"]
        .iter()
        .any(|w| n.contains(w))
}

/// Every type name used in the fields of the file's `Deserialize` types,
/// plus the associated types they go through (`type DeviceType = X;`).
/// Request types are sent, not received, and don't count.
fn deserialized_field_types(file: &syn::File) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |ty: &syn::Type| {
        for word in syntax::type_string(ty).split(|c: char| !c.is_alphanumeric() && c != '_') {
            out.insert(word.to_string());
        }
    };
    for item in syntax::all_items(file) {
        match item {
            syn::Item::Struct(s)
                if s.ident.to_string().ends_with("Request")
                    || s.ident.to_string().ends_with("Params") => {}
            syn::Item::Impl(i) => {
                for it in &i.items {
                    if let syn::ImplItem::Type(t) = it {
                        add(&t.ty);
                    }
                }
            }
            syn::Item::Struct(s) if syntax::has_derive(&s.attrs, "Deserialize") => {
                s.fields.iter().for_each(|f| add(&f.ty));
            }
            syn::Item::Enum(e) if syntax::has_derive(&e.attrs, "Deserialize") => {
                e.variants
                    .iter()
                    .flat_map(|v| v.fields.iter())
                    .for_each(|f| add(&f.ty));
            }
            _ => {}
        }
    }
    out
}

fn check_file(
    file: &RustFile,
    added: &BTreeSet<usize>,
    is_nested: &dyn Fn(&str) -> bool,
    hits: &mut Vec<(usize, usize, String)>,
) {
    for item in syntax::all_items(&file.ast) {
        match item {
            syn::Item::Struct(s) if syntax::has_derive(&s.attrs, "Deserialize") => {
                let name = s.ident.to_string();
                if has_serde(&s.attrs, "deny_unknown_fields") {
                    let start = s.ident.span().start();
                    hits.push((start.line, start.column + 1, format!("`{name}` denies unknown fields, so a server adding one breaks every shipped client; drop `deny_unknown_fields`")));
                }
                let struct_default = has_serde(&s.attrs, "default");
                for field in &s.fields {
                    let line = field.span_line();
                    let optional = syntax::type_ident(&field.ty).as_deref() == Some("Option");
                    let defaulted = struct_default
                        || has_serde(&field.attrs, "default")
                        || has_serde(&field.attrs, "flatten");
                    if added.contains(&line) && !optional && !defaulted {
                        let fname = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                        hits.push((line, 1, format!("new field `{name}::{fname}` is required: records and servers without it fail to parse; make it `Option<_>` or `#[serde(default)]`")));
                    }
                }
            }
            syn::Item::Enum(e) if syntax::has_derive(&e.attrs, "Deserialize") => {
                let name = e.ident.to_string();
                let start = e.ident.span().start();
                if has_serde(&e.attrs, "deny_unknown_fields") {
                    hits.push((start.line, start.column + 1, format!("`{name}` denies unknown fields, so a server adding one breaks every shipped client; drop `deny_unknown_fields`")));
                }
                if has_serde(&e.attrs, "untagged") {
                    untagged_shadowing(e, hits);
                    continue;
                }
                let fieldless = e.variants.iter().all(|v| v.fields.is_empty());
                let fallback = e.variants.iter().find(|v| has_serde(&v.attrs, "other"));
                // Internally tagged enums (`#[serde(tag = "type")]`) of records
                // are matched on their tag; leave those to review.
                if !fieldless || e.variants.len() < 2 || !is_nested(&name) {
                    continue;
                }
                match fallback {
                    None => hits.push((start.line, start.column + 1, format!("`{name}` has no `#[serde(other)]` variant, so a value added on the server fails the whole record; add an `Unknown` fallback"))),
                    Some(v) if !is_unknown_name(&v.ident.to_string()) => {
                        let vs = v.ident.span().start();
                        hits.push((vs.line, vs.column + 1, format!("unknown `{name}` values fall back to the real value `{}`; add a separate `Unknown` variant that callers treat as unsupported", v.ident)));
                    }
                    Some(_) => {}
                }
            }
            _ => {}
        }
    }
}

/// `#[serde(untagged)]` tries variants in order: a struct variant whose
/// fields are a subset of a later one's always wins.
fn untagged_shadowing(e: &syn::ItemEnum, hits: &mut Vec<(usize, usize, String)>) {
    let fields_of = |v: &syn::Variant| -> Option<BTreeSet<String>> {
        match &v.fields {
            syn::Fields::Named(n) => Some(
                n.named
                    .iter()
                    .filter(|f| syntax::type_ident(&f.ty).as_deref() != Some("Option"))
                    .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
                    .collect(),
            ),
            _ => None,
        }
    };
    let variants: Vec<_> = e.variants.iter().collect();
    for (i, early) in variants.iter().enumerate() {
        let Some(early_fields) = fields_of(early) else { continue };
        for late in &variants[i + 1..] {
            let Some(late_fields) = fields_of(late) else { continue };
            if early_fields.is_subset(&late_fields) {
                let start = late.ident.span().start();
                hits.push((start.line, start.column + 1, format!("untagged `{}::{}` never matches: `{}` comes first and needs only a subset of its fields; put the more specific variant first", e.ident, late.ident, early.ident)));
                break;
            }
        }
    }
}

trait SpanLine {
    fn span_line(&self) -> usize;
}

impl SpanLine for syn::Field {
    fn span_line(&self) -> usize {
        match &self.ident {
            Some(i) => i.span().start().line,
            None => syn::spanned::Spanned::span(&self.ty).start().line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check, check_diff};

    const PATH: &str = "components/search/src/configuration_types.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/tolerant-remote-deserialization/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 9);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/tolerant-remote-deserialization/do.rs")).is_empty());
    }

    #[test]
    fn flags_real_fallbacks_strictness_and_shadowed_variants() {
        let src = "#[derive(Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct R { pub app: App, pub ad: Ad }\n#[derive(Deserialize)]\npub enum App {\n    Android,\n    #[serde(other)]\n    Desktop,\n}\n#[derive(Deserialize)]\n#[serde(untagged)]\npub enum Ad {\n    Image { url: String },\n    Spoc { url: String, sponsor: String },\n}\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 8, 14]);
    }

    #[test]
    fn ignores_enums_never_read_from_the_server() {
        let src = "#[derive(Deserialize, uniffi::Enum)]\npub enum Locale { Fr, En }\n#[derive(Serialize, Deserialize)]\npub struct RecsRequest { pub locale: Locale }\n";
        assert!(run(src).is_empty());
        let other_crate = "#[derive(Deserialize)]\npub struct R { pub kind: Kind }\n#[derive(Deserialize)]\npub enum Kind { A, B }\n";
        assert!(check(&RULE, &[("components/places/src/x.rs", other_crate)]).is_empty());
    }

    #[test]
    fn flags_new_required_fields_in_a_diff() {
        let old = "#[derive(Deserialize)]\npub struct Rec {\n    pub id: String,\n}\n";
        let new = "#[derive(Deserialize)]\npub struct Rec {\n    pub id: String,\n    pub suggestion_id: String,\n    pub score: Option<f64>,\n    #[serde(default)]\n    pub tags: Vec<String>,\n}\n";
        let found = check_diff(&RULE, &[(PATH, old)], &[(PATH, new)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4]);
    }
}
