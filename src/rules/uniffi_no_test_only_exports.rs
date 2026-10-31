//! Don't export test-only helpers over FFI
//!
//! Docs and examples: `rules/uniffi-no-test-only-exports/`.

use std::sync::OnceLock;

use regex::Regex;
use syn::spanned::Spanned;

use crate::helpers::api_mobile::{
    is_production_rust, is_testing_feature_gated, is_uniffi_crate_file, udl_docs, udl_name_line,
};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "uniffi-no-test-only-exports",
    area: Area::Uniffi,
    title: "Don't export test-only helpers over FFI",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Words that look like tests but are product vocabulary.
const ALLOWED_NAMES: &[&str] = &["test_connection"];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files() {
        if !is_uniffi_crate_file(cx.ws, &file.path) {
            continue;
        }
        if file.extension() == "udl" && !is_test_path(&file.path) {
            check_udl(&file.path, &file.text, out);
        } else if is_production_rust(&file.path) {
            if let Some(rust) = cx.ws.rust(&file.path) {
                check_rust(rust, out);
            }
        }
    }
}

/// Why `name` (documented by `doc`) looks test-only, if it does.
fn test_only(name: &str, doc: Option<&str>) -> Option<&'static str> {
    static DOC: OnceLock<Regex> = OnceLock::new();
    let doc_re = DOC.get_or_init(|| {
        Regex::new(
            r"(?ix)
            \b(only|primarily|mainly|mostly|solely)\b [^.]{0,30}? \b(for|in|by)\s+(unit\s+)?test(s|ing)?\b
            | \btest[-\ ]only\b
            | \btesting\s+only\b
            | \bused\s+(only\s+)?(by|in)\s+tests\b
            | \b(exists?|exposed|here)\s+(only\s+)?for\s+test(s|ing)\b",
        )
        .expect("valid regex")
    });
    let lower = name.to_ascii_lowercase();
    if ALLOWED_NAMES.contains(&lower.as_str()) {
        return None;
    }
    let by_name = lower.starts_with("test_")
        || lower.ends_with("_for_test")
        || lower.ends_with("_for_tests")
        || lower.ends_with("_for_testing")
        || lower.ends_with("_testing");
    if by_name {
        return Some("is named like a test helper");
    }
    if doc.is_some_and(|d| doc_re.is_match(d)) {
        return Some("is documented as test-only");
    }
    None
}

fn message(what: &str, name: &str, why: &str) -> String {
    format!(
        "{what} `{name}` {why} but ships in every app's API; keep it out of the export (e.g. a `#[cfg(any(test, feature = \"testing\"))]` impl the megazords never enable)"
    )
}

fn check_rust(file: &RustFile, out: &mut Findings) {
    for item in syntax::all_items(&file.ast) {
        let (attrs, line) = match item {
            syn::Item::Fn(f) => (&f.attrs, file.line_of_node(&f.sig.ident)),
            syn::Item::Impl(i) => (&i.attrs, file.line_of_node(&i.self_ty)),
            syn::Item::Struct(s) => (&s.attrs, file.line_of_node(&s.ident)),
            syn::Item::Enum(e) => (&e.attrs, file.line_of_node(&e.ident)),
            _ => continue,
        };
        if file.in_test(line) || syntax::is_test_gated(attrs) || is_testing_feature_gated(attrs) {
            continue;
        }
        match item {
            syn::Item::Fn(f) if syntax::has_attr(&f.attrs, "uniffi::export") => {
                let name = f.sig.ident.to_string();
                if let Some(why) = test_only(&name, syntax::doc(&f.attrs).as_deref()) {
                    out.add(&file.path, line, message("exported fn", &name, why));
                }
            }
            syn::Item::Impl(i) if syntax::has_attr(&i.attrs, "uniffi::export") => {
                let ty = syntax::type_ident(&i.self_ty).unwrap_or_default();
                for it in &i.items {
                    let syn::ImplItem::Fn(m) = it else { continue };
                    let exported =
                        matches!(m.vis, syn::Visibility::Public(_)) || i.trait_.is_some();
                    if !exported
                        || syntax::is_test_gated(&m.attrs)
                        || is_testing_feature_gated(&m.attrs)
                    {
                        continue;
                    }
                    let name = m.sig.ident.to_string();
                    if let Some(why) = test_only(&name, syntax::doc(&m.attrs).as_deref()) {
                        out.add(
                            &file.path,
                            m.sig.ident.span().start().line,
                            message("exported method", &format!("{ty}::{name}"), why),
                        );
                    }
                }
            }
            syn::Item::Struct(_) | syn::Item::Enum(_) => {
                let exported = ["uniffi::Record", "uniffi::Object", "uniffi::Enum"]
                    .iter()
                    .any(|d| syntax::has_derive(attrs, d));
                let doc = syntax::doc(attrs);
                let documented = doc
                    .as_deref()
                    .is_some_and(|d| test_only("", Some(d)).is_some());
                if exported && documented {
                    let name = match item {
                        syn::Item::Struct(s) => s.ident.to_string(),
                        syn::Item::Enum(e) => e.ident.to_string(),
                        _ => String::new(),
                    };
                    let span_line = item.span().start().line.max(line);
                    out.add(
                        &file.path,
                        span_line,
                        message("exported type", &name, "is documented as test-only"),
                    );
                }
            }
            _ => {}
        }
    }
}

fn check_udl(path: &str, text: &str, out: &mut Findings) {
    let docs = udl_docs(text);
    for item in udl::parse(text) {
        let what = match item.kind {
            UdlKind::Namespace => "exported fn",
            UdlKind::Interface | UdlKind::Dictionary => "exported method",
            _ => continue,
        };
        if matches!(item.kind, UdlKind::Interface | UdlKind::Dictionary) {
            if let Some(doc) = docs.get(&item.line) {
                if test_only("", Some(doc)).is_some() {
                    out.add(
                        path,
                        item.line,
                        message("exported type", &item.name, "is documented as test-only"),
                    );
                    continue; // its members go with it
                }
            }
        }
        if item.kind == UdlKind::Dictionary || item.has_attr("Error") || item.has_attr("Enum") {
            continue;
        }
        for m in &item.members {
            if !m.signature.contains('(') {
                continue;
            }
            let name = if item.kind == UdlKind::Namespace {
                m.name.clone()
            } else {
                format!("{}::{}", item.name, m.name)
            };
            if let Some(why) = test_only(&m.name, docs.get(&m.line).map(String::as_str)) {
                out.add(
                    path,
                    udl_name_line(text, m.line, &m.name),
                    message(what, &name, why),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/tabs/Cargo.toml",
        "[package]\nname = \"tabs\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const PATH: &str = "components/tabs/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-no-test-only-exports/dont.rs"),
                ),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-no-test-only-exports/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_udl_members_by_doc() {
        let udl = r#"
interface RemoteCommandStore {
    /// Add a new command with an explicit timestamp. Primarily used by tests.
    [Throws=TabsApiError]
    boolean add_remote_command_at(string device_id, Timestamp when);

    /// Records an event as if it were in the past. Useful for testing.
    void record_past_event(string event_id);
};

/// A testing interface. This should only be used for testing.
interface OhttpTestServer {
    constructor();
};
"#;
        let found = check(&RULE, &[MANIFEST, ("components/tabs/src/tabs.udl", udl)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 12], "{found:#?}");
    }

    #[test]
    fn ignores_gated_and_private_helpers() {
        let src = r#"
#[uniffi::export]
impl Store {
    /// Only for tests.
    #[cfg(feature = "testing")]
    pub fn reset_for_tests(&self) {}
    fn wipe_for_tests(&self) {}
    /// Runs the store's connection test against the server.
    pub fn check_server(&self) {}
}

#[cfg_attr(feature = "testing", uniffi::export)]
pub fn set_last_sync_for_tests() {}

pub fn open_for_tests() {}
"#;
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_crates_without_uniffi() {
        let manifest = ("components/tabs/Cargo.toml", "[package]\nname = \"tabs\"\n");
        let src = "#[uniffi::export]\npub fn test_reset() {}\n";
        assert!(check(&RULE, &[manifest, (PATH, src)]).is_empty());
    }
}
