//! Expose typed values over FFI (enums, Url, timestamps), not strings, magic ints or raw JSON
//!
//! Docs and examples: `rules/uniffi-typed-values/`.

use std::collections::BTreeMap;

use crate::helpers::api_mobile::{all_slots, is_uniffi_crate_file, words, Slot, SlotKind};
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "uniffi-typed-values",
    area: Area::Uniffi,
    title:
        "Expose typed values over FFI (enums, Url, timestamps), not strings, magic ints or raw JSON",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// Name endings that mean "one of a small fixed set". Strings named like
/// this are often open-ended by contract (server labels, MIME types), so only
/// integers are flagged.
const ENUM_WORDS: &[&str] = &["type", "kind", "category", "strategy", "mode", "status"];

fn check(cx: &Context, out: &mut Findings) {
    let slots = all_slots(cx.ws);
    let url_types = url_custom_types(cx);
    for s in &slots {
        // UDL declarations are the old API surface: changing a type there is a
        // breaking change of long-standing API, and new API uses proc-macros.
        if s.from_udl {
            continue;
        }
        if explained(s) || parsed_by_hand(cx, s) {
            continue;
        }
        if let Some(message) = finding(s, url_types.get(&crate_dir(&s.path))) {
            out.add(&s.path, s.line, message);
        }
    }
}

fn finding(s: &Slot, url_type: Option<&String>) -> Option<String> {
    let base = s.base_type();
    // A return value is named by its function: `to_json`, `get_status`.
    let name = match s.kind {
        SlotKind::Return => s.owner.rsplit("::").next().unwrap_or(""),
        _ => s.name.as_str(),
    };
    let w = words(name);
    let last = w.last().map(String::as_str).unwrap_or("");
    let what = match s.kind {
        SlotKind::Arg => format!("argument `{name}` of `{}`", s.owner),
        SlotKind::Return => format!("`{}` returns", s.owner),
        _ => format!("`{}.{name}`", s.owner),
    };
    if base == "String" && matches!(last, "url" | "uri" | "href") {
        let url_type = url_type?;
        return Some(format!(
            "{what} is a `String` though this crate exports `{url_type}`; take `{url_type}` so bad URLs fail at the boundary"
        ));
    }
    let output = s.kind != SlotKind::Arg;
    if base == "String" && output && (last == "json" || name == "json") {
        return Some(format!(
            "{what} is raw JSON every consumer must parse; expose a Record or Enum instead"
        ));
    }
    let integer = matches!(
        base.as_str(),
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize"
    );
    let enumish = ENUM_WORDS.contains(&last) && w.len() > 1;
    // `status: u16` is an HTTP status code, open-ended by contract.
    if integer && enumish && last != "status" {
        return Some(format!(
            "{what} is a magic integer; use a `uniffi::Enum` so callers can't pass an unknown value"
        ));
    }
    None
}

/// A URL argument the function parses itself (`Url::parse(&click_url)`) to
/// map a bad URL to its own error: deliberate, since a failed lift of a
/// custom type surfaces as an unexpected error instead.
fn parsed_by_hand(cx: &Context, s: &Slot) -> bool {
    if s.kind != SlotKind::Arg {
        return false;
    }
    let Some(file) = cx.ws.file(&s.path) else {
        return false;
    };
    let pattern = format!(r"parse\(\s*&?\s*{}\s*\)", regex::escape(&s.name));
    regex::Regex::new(&pattern).is_ok_and(|re| re.is_match(&file.text))
}

/// An intentional raw string or JSON is explained in its docstring.
fn explained(s: &Slot) -> bool {
    let Some(doc) = s.doc.as_deref() else {
        return false;
    };
    let lower = doc.to_lowercase();
    s.kind != SlotKind::Arg
        && (lower.contains("opaque")
            || lower.contains("not a `url`")
            || lower.contains("raw json")
            || lower.contains("malformed"))
}

/// The URL custom type (`uniffi::custom_type!(AdsClientUrl, String, ..)` or a
/// UDL `[Custom] typedef string Url;`) each crate already exports, by crate dir.
fn url_custom_types(cx: &Context) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for file in cx.ws.files() {
        if !is_uniffi_crate_file(cx.ws, &file.path) {
            continue;
        }
        let mut names = Vec::new();
        if file.extension() == "udl" {
            for item in udl::parse(&file.text) {
                if item.kind == UdlKind::Typedef && item.has_attr("Custom") {
                    names.push(item.name);
                }
            }
        } else if let Some(rust) = cx.ws.rust(&file.path) {
            for item in syntax::all_items(&rust.ast) {
                if let syn::Item::Macro(m) = item {
                    let macro_name = syntax::macro_name(&m.mac);
                    if macro_name == "custom_type" || macro_name == "custom_newtype" {
                        if let Some(first) = m.mac.tokens.clone().into_iter().next() {
                            names.push(first.to_string());
                        }
                    }
                }
            }
        }
        for name in names {
            if words(&name).last().is_some_and(|w| w == "url") {
                out.entry(crate_dir(&file.path)).or_insert(name);
            }
        }
    }
    out
}

/// `components/ads-client` for `components/ads-client/src/lib.rs`.
fn crate_dir(path: &str) -> String {
    path.split("/src/").next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/suggest/Cargo.toml",
        "[package]\nname = \"suggest\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const PATH: &str = "components/suggest/src/yelp.rs";
    const URL_TYPE: (&str, &str) = (
        "components/suggest/src/lib.rs",
        "uniffi::custom_type!(SuggestUrl, String, { remote, try_lift: |s| Ok(Url::parse(&s)?), lower: |u| u.to_string() });\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                URL_TYPE,
                (PATH, include_str!("../../rules/uniffi-typed-values/dont.rs")),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![4, 5, 6], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                URL_TYPE,
                (PATH, include_str!("../../rules/uniffi-typed-values/do.rs")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn leaves_urls_alone_without_a_url_type() {
        let src = "#[derive(uniffi::Record)]\npub struct Amp { pub url: String, pub mime_type: String, pub status: u16, pub icon: EngineUrl, pub suggestion_type: String }\n#[derive(uniffi::Record)]\npub struct EngineUrl { pub base: String }\n";
        assert!(check(&RULE, &[MANIFEST, (PATH, src)]).is_empty());
    }

    #[test]
    fn checks_exported_methods_but_not_udl() {
        let src = r#"
#[uniffi::export]
impl Store {
    pub fn record_click(&self, click_url: String) {}
    pub fn record_view(&self, view_url: String) -> Result<()> { let url = Url::parse(&view_url)?; Ok(()) }
    pub fn telemetry(&self) -> String { todo!() }
    fn private(&self, subject_type: i32) {}
}
"#;
        let udl = "dictionary Card { string cc_type; string url; };\n";
        let found = check(
            &RULE,
            &[
                MANIFEST,
                URL_TYPE,
                (PATH, src),
                ("components/suggest/src/suggest.udl", udl),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("`click_url`"));
    }
}
