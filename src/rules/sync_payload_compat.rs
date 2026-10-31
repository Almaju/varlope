//! Sync payloads round-trip unknown fields and tolerate nulls
//!
//! Docs and examples: `rules/sync-payload-compat/`.
//!
//! A sync payload type is the `T` an engine deserializes incoming records
//! into (`into_content::<T>()`, `into_content_with_fixup::<T>(..)`,
//! `IncomingContent<TRecord>`), or a struct wrapped by such an enum. The rule
//! flags payload structs without a `#[serde(flatten)]` `UnknownFields` field,
//! on themselves or on a struct they nest (fields written by newer clients
//! are dropped and the lossy record is uploaded again), and
//! `#[serde(default)]` on a `String` field with no `deserialize_with`:
//! `default` covers a missing key, not the `"title": null` older Desktop
//! versions wrote. Payloads only their own device writes (`TabsRecord`) are
//! exempt. Round-trip tests are left to review.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "sync-payload-compat",
    area: Area::Remote,
    title: "Sync payloads round-trip unknown fields and tolerate nulls",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Payloads no other client re-uploads: each device writes only its own.
const EXEMPT: &[&str] = &["TabsRecord", "ClientRecord"];

fn check(cx: &Context, out: &mut Findings) {
    for package in cx.ws.packages() {
        let files: Vec<_> = cx
            .ws
            .package_files(package)
            .filter_map(|f| cx.ws.rust(&f.path))
            .filter(|f| errors::is_shipped(&f.path))
            .collect();
        // Payload type names used by this package's engines.
        let mut payloads: BTreeSet<String> = BTreeSet::new();
        for file in &files {
            let support = TestSupport::of(file);
            let mut v = Payloads(Vec::new());
            v.visit_file(&file.ast);
            payloads.extend(
                v.0.into_iter()
                    .filter(|(line, _)| !support.contains(*line) && !file.in_test(*line))
                    .map(|(_, name)| name),
            );
        }
        if payloads.is_empty() {
            continue;
        }
        // Struct and enum declarations of the package, by name.
        let mut structs: BTreeMap<String, (&str, &syn::ItemStruct)> = BTreeMap::new();
        let mut enums: BTreeMap<String, &syn::ItemEnum> = BTreeMap::new();
        for file in &files {
            for item in syntax::all_items(&file.ast) {
                match item {
                    syn::Item::Struct(s) => {
                        structs.insert(s.ident.to_string(), (file.path.as_str(), s));
                    }
                    syn::Item::Enum(e) => {
                        enums.insert(e.ident.to_string(), e);
                    }
                    _ => {}
                }
            }
        }
        // An enum payload (`BookmarkItemRecord`) is made of its variants' structs.
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for name in &payloads {
            match enums.get(name) {
                Some(e) => {
                    for v in &e.variants {
                        if let syn::Fields::Unnamed(f) = &v.fields {
                            if let Some(t) =
                                f.unnamed.first().and_then(|f| syntax::type_ident(&f.ty))
                            {
                                targets.insert(t);
                            }
                        }
                    }
                }
                None => {
                    targets.insert(name.clone());
                }
            }
        }
        let keeps = |s: &syn::ItemStruct| keeps_unknown(s);
        for name in targets {
            let Some((path, s)) = structs.get(&name) else {
                continue;
            };
            if !syntax::has_derive(&s.attrs, "Deserialize") || EXEMPT.contains(&name.as_str()) {
                continue;
            }
            // `AddressPayload { entry: PayloadEntry }` keeps them in `entry`.
            let nested_keeps = s.fields.iter().any(|f| {
                syntax::type_ident(&f.ty)
                    .and_then(|t| structs.get(&t))
                    .is_some_and(|(_, inner)| keeps(inner))
            });
            check_struct(path, s, keeps(s) || nested_keeps, out);
        }
    }
}

fn serde_args(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("serde"))
        .map(syntax::attr_args)
        .collect::<Vec<_>>()
        .join(",")
}

fn has_word(args: &str, word: &str) -> bool {
    args.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// Whether a struct has a `#[serde(flatten)] UnknownFields` field.
fn keeps_unknown(s: &syn::ItemStruct) -> bool {
    s.fields.iter().any(|f| {
        has_word(&serde_args(&f.attrs), "flatten")
            && syntax::type_string(&f.ty).contains("UnknownFields")
    })
}

fn check_struct(path: &str, s: &syn::ItemStruct, keeps_unknown: bool, out: &mut Findings) {
    let name = s.ident.to_string();
    if !keeps_unknown {
        let start = s.ident.span().start();
        out.add_at(
            path,
            start.line,
            start.column + 1,
            format!("sync payload `{name}` drops fields written by newer clients and uploads the record without them; add `#[serde(flatten)] unknown_fields: UnknownFields`"),
        );
    }
    let struct_default = has_word(&serde_args(&s.attrs), "default");
    for field in &s.fields {
        let args = serde_args(&field.attrs);
        let string = syntax::type_ident(&field.ty).as_deref() == Some("String");
        let null_handled = args.contains("deserialize_with") || args.contains("with");
        let field_default = has_word(&args, "default");
        if field_default && !struct_default && string && !null_handled {
            let fname = field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_default();
            let start = field
                .ident
                .as_ref()
                .map(|i| i.span().start())
                .unwrap_or_else(|| syn::spanned::Spanned::span(&field.ty).start());
            out.add_at(
                path,
                start.line,
                start.column + 1,
                format!("`#[serde(default)]` on `{fname}` covers a missing key, not the explicit `null` older clients wrote; use `Option<_>` or a null-tolerant `deserialize_with`"),
            );
        }
    }
}

/// `(line, type name)` of each payload type use.
struct Payloads(Vec<(usize, String)>);

impl<'ast> Visit<'ast> for Payloads {
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if name == "into_content" || name == "into_content_with_fixup" {
            if let Some(turbofish) = &m.turbofish {
                for arg in &turbofish.args {
                    if let syn::GenericArgument::Type(t) = arg {
                        if let Some(ident) = syntax::type_ident(t) {
                            self.0.push((m.method.span().start().line, ident));
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        if let Some(last) = t.path.segments.last() {
            if last.ident == "IncomingContent" {
                let ty = syn::Type::Path(t.clone());
                for arg in syntax::type_args(&ty) {
                    if let Some(ident) = syntax::type_ident(arg) {
                        if ident.ends_with("Record") || ident.ends_with("Payload") {
                            self.0.push((last.ident.span().start().line, ident));
                        }
                    }
                }
            }
        }
        syn::visit::visit_type_path(self, t);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/places/Cargo.toml",
        "[package]\nname = \"places\"\n",
    );
    const PATH: &str = "components/places/src/history_sync/record.rs";
    const ENGINE: (&str, &str) = (
        "components/places/src/history_sync/plan.rs",
        "fn plan(incoming: IncomingBso) { let content = incoming.into_content::<HistoryRecord>(); }\n",
    );

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[MANIFEST, ENGINE, (PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/sync-payload-compat/dont.rs"));
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 6]);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/sync-payload-compat/do.rs")).is_empty());
    }

    #[test]
    fn accepts_unknown_fields_on_a_nested_struct() {
        let src = "#[derive(Deserialize, Serialize)]\npub struct HistoryRecord { id: Guid, entry: Entry }\n#[derive(Deserialize, Serialize)]\n#[serde(default)]\nstruct Entry {\n    name: String,\n    #[serde(flatten)]\n    unknown_fields: UnknownFields,\n}\n";
        assert!(run(src).is_empty());
    }

    #[test]
    fn ignores_types_that_are_not_payloads() {
        let src = "#[derive(Deserialize, Serialize)]\npub struct HistoryRow { id: Guid }\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, src)]).is_empty());
    }
}
