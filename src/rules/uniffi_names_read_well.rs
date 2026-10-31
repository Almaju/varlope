//! Exported names must read well in Kotlin/Swift and say what they do
//!
//! Docs and examples: `rules/uniffi-names-read-well/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::api_mobile::{all_slots, is_generated_or_vendored, words, SlotKind};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::uniffi::{self, ApiKind, MemberKind};
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "uniffi-names-read-well",
    area: Area::Uniffi,
    title: "Exported names must read well in Kotlin/Swift and say what they do",
    kind: Kind::Review,
    clippy: &[],
    in_tests: false,
    check,
};

/// Words about how the code is built, not what it does.
const IMPLEMENTATION_WORDS: &[&str] = &["rust", "impl", "ffi", "uniffi", "inner"];

/// Function names that already mean something in Kotlin or Swift.
const IDIOMS: &[(&str, &str)] = &[
    (
        "run_blocking",
        "Kotlin's `runBlocking` runs a coroutine and blocks on it",
    ),
    ("to_string", "it shadows `toString()` in Kotlin"),
    ("hash_code", "it shadows `hashCode()` in Kotlin"),
    ("equals", "it shadows `equals()` in Kotlin"),
    (
        "description",
        "Swift uses `description` for `CustomStringConvertible`",
    ),
    (
        "debug_description",
        "Swift uses `debugDescription` for `CustomDebugStringConvertible`",
    ),
];

/// Verbs that promise to only look, and words that say a call destroys data.
const LOOKING_VERBS: &[&str] = &[
    "verify", "check", "validate", "is", "has", "get", "count", "find", "list", "query", "fetch",
    "read", "load", "scan",
];
const DESTRUCTIVE: &[&str] = &[
    "delete",
    "deletes",
    "remove",
    "removes",
    "wipe",
    "wipes",
    "clear",
    "clears",
    "discard",
    "discards",
    "purge",
    "purges",
    "drop",
    "drops",
    "overwrite",
    "overwrites",
];

/// One exported name, where it is declared.
struct Name {
    name: String,
    path: String,
    line: usize,
    /// What it is, for messages: "type", "fn", "field"...
    what: &'static str,
    doc: Option<String>,
}

fn check(cx: &Context, out: &mut Findings) {
    let names = exported_names(cx);
    for n in &names {
        let w = words(&n.name);
        // Only type names: `trigger_rust_panic` or `init_backend_rust` use the
        // word for what it is.
        let implementation_word = w
            .iter()
            .find(|w| IMPLEMENTATION_WORDS.contains(&w.as_str()))
            .filter(|_| n.what == "type");
        if let Some(word) = implementation_word {
            out.add(
                &n.path,
                n.line,
                format!(
                    "exported {} `{}` says `{word}`, an implementation detail callers don't care about; drop it from the name",
                    n.what, n.name
                ),
            );
        }
        if n.what == "fn" {
            if let Some((_, why)) = IDIOMS.iter().find(|(idiom, _)| *idiom == n.name) {
                out.add(
                    &n.path,
                    n.line,
                    format!(
                        "exported fn `{}` reads wrong in the bindings: {why}; rename it",
                        n.name
                    ),
                );
            }
            if let Some(message) = destructive_but_quiet(n, &w) {
                out.add(&n.path, n.line, message);
            }
        }
    }
    spelled_two_ways(&names, out);
}

/// `verify_logins` documented as deleting what it can't decrypt.
fn destructive_but_quiet(n: &Name, w: &[String]) -> Option<String> {
    let first = w.first()?;
    if !LOOKING_VERBS.contains(&first.as_str())
        || w.iter().any(|w| DESTRUCTIVE.contains(&w.as_str()))
    {
        return None;
    }
    // The summary sentence, without `code` spans.
    let doc = n.doc.as_deref()?.to_lowercase();
    let summary = doc.split("\n\n").next().unwrap_or("");
    let summary = summary.split(". ").next().unwrap_or(summary);
    let doc: String = summary
        .split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(" ");
    let doc_words: Vec<&str> = doc
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|d| !d.is_empty())
        .collect();
    // "never deletes", "doesn't remove", "without wiping" don't count.
    let negated = |i: usize| {
        doc_words[i.saturating_sub(2)..i]
            .iter()
            .any(|d| matches!(*d, "never" | "not" | "without" | "no") || d.ends_with("n't"))
    };
    let word = (0..doc_words.len())
        .find(|&i| DESTRUCTIVE.contains(&doc_words[i]) && !negated(i))
        .map(|i| doc_words[i])?;
    Some(format!(
        "exported fn `{}` is documented to {} data but its name says `{first}`; name it after what it destroys (`delete_*`, `wipe_*`, `clear_*`)",
        n.name,
        word.trim_end_matches('s')
    ))
}

/// The same name spelled with different word breaks in one crate
/// (`mimetype` and `mime_type`): the bindings show both.
fn spelled_two_ways(names: &[Name], out: &mut Findings) {
    // crate -> squashed spelling -> spelling -> where
    let mut by_crate: BTreeMap<String, BTreeMap<String, BTreeMap<String, Vec<&Name>>>> =
        BTreeMap::new();
    for n in names.iter().filter(|n| n.what != "type") {
        let w = words(&n.name);
        let squashed: String = w.concat();
        let spelling = w.join("_");
        by_crate
            .entry(crate_dir(&n.path))
            .or_default()
            .entry(squashed)
            .or_default()
            .entry(spelling)
            .or_default()
            .push(n);
    }
    for spellings in by_crate.values().flat_map(|c| c.values()) {
        if spellings.len() < 2 {
            continue;
        }
        // Report the rarer spellings, pointing at the common one.
        let (common, _) = spellings
            .iter()
            .max_by_key(|(spelling, uses)| (uses.len(), spelling.len()))
            .expect("two spellings");
        for (spelling, uses) in spellings {
            if spelling == common {
                continue;
            }
            let mut seen = BTreeSet::new();
            for n in uses {
                if seen.insert((&n.path, n.line)) {
                    out.add(
                        &n.path,
                        n.line,
                        format!(
                            "`{}` is spelled `{common}` elsewhere in this crate's API; pick one spelling",
                            n.name
                        ),
                    );
                }
            }
        }
    }
}

fn crate_dir(path: &str) -> String {
    path.split("/src/").next().unwrap_or(path).to_string()
}

/// Exported type, function, method, field and argument names.
fn exported_names(cx: &Context) -> Vec<Name> {
    let mut out = Vec::new();
    for package in uniffi::uniffi_packages(cx.ws) {
        for item in uniffi::package_api(cx.ws, package) {
            if is_generated_or_vendored(&item.path) || is_test_path(&item.path) {
                continue;
            }
            match item.kind {
                ApiKind::Function => {
                    let doc = doc_for(cx, &item.path, item.line);
                    out.push(Name {
                        name: item.name.clone(),
                        path: item.path.clone(),
                        line: item.line,
                        what: "fn",
                        doc,
                    });
                }
                _ => {
                    // Methods attach to an Object declared elsewhere; the
                    // `impl` block's own name is not a declaration.
                    let declared = !(item.kind == ApiKind::Object
                        && item.source == uniffi::ApiSource::ProcMacro
                        && item.members.iter().all(|m| {
                            matches!(m.kind, MemberKind::Method | MemberKind::Constructor)
                        })
                        && !item.members.is_empty());
                    if declared {
                        out.push(Name {
                            name: item.name.clone(),
                            path: item.path.clone(),
                            line: item.line,
                            what: "type",
                            doc: None,
                        });
                    }
                    // Only Record fields cross the FFI; Object fields are private.
                    let fields_exported = item.kind == ApiKind::Record;
                    for m in &item.members {
                        if m.kind == MemberKind::Field && !fields_exported {
                            continue;
                        }
                        let what = match m.kind {
                            MemberKind::Method => "fn",
                            MemberKind::Field => "field",
                            MemberKind::Variant => "variant",
                            MemberKind::Constructor => continue,
                        };
                        if what == "variant" {
                            continue; // variants are type-like names, checked as part of the type
                        }
                        out.push(Name {
                            name: m.name.clone(),
                            path: item.path.clone(),
                            line: m.line,
                            what,
                            doc: doc_for(cx, &item.path, m.line),
                        });
                    }
                }
            }
        }
    }
    for slot in all_slots(cx.ws) {
        if slot.kind == SlotKind::Arg || slot.kind == SlotKind::VariantField {
            out.push(Name {
                name: slot.name.clone(),
                path: slot.path.clone(),
                line: slot.line,
                what: if slot.kind == SlotKind::Arg {
                    "argument"
                } else {
                    "field"
                },
                doc: None,
            });
        }
    }
    out
}

/// The doc comment right above `line`: `///` lines in Rust or UDL, skipping
/// attributes.
fn doc_for(cx: &Context, path: &str, line: usize) -> Option<String> {
    let file = cx.ws.file(path)?;
    let lines: Vec<&str> = file.text.lines().collect();
    let mut doc = Vec::new();
    let mut k = line.saturating_sub(1);
    while k > 0 {
        let above = lines.get(k - 1)?.trim();
        if let Some(d) = above.strip_prefix("///") {
            doc.push(d.trim());
        } else if !(above.starts_with("#[")
            || above.starts_with('[')
            || above.is_empty() && doc.is_empty())
        {
            break;
        }
        k -= 1;
    }
    doc.reverse();
    (!doc.is_empty()).then(|| doc.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MANIFEST: (&str, &str) = (
        "components/logins/Cargo.toml",
        "[package]\nname = \"logins\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const PATH: &str = "components/logins/src/store.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-names-read-well/dont.rs"),
                ),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 10], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MANIFEST,
                (
                    PATH,
                    include_str!("../../rules/uniffi-names-read-well/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_implementation_words_and_two_spellings() {
        let src = r#"
#[derive(uniffi::Enum)]
pub enum FxaRustAuthState { Disconnected }

#[derive(uniffi::Record)]
pub struct Icon { pub mime_type: String }
#[derive(uniffi::Record)]
pub struct Amp { pub icon_mimetype: String, pub icon_mime_type: String, pub data_mime_type: String }
#[derive(uniffi::Record)]
pub struct Yelp { pub mimetype: String }
"#;
        let found = check(&RULE, &[MANIFEST, (PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 8, 10], "{found:#?}");
    }

    #[test]
    fn ignores_private_and_honest_names() {
        let src = r#"
#[derive(uniffi::Object)]
pub struct Store { inner: Mutex<StoreImpl> }

#[uniffi::export]
impl Store {
    /// Get the state.
    ///
    /// TODO: remove this, and call `clear_cache` first.
    pub fn get_state(&self) {}
    /// Triggers a Rust panic, for crash reporting tests.
    pub fn trigger_rust_panic(&self) {}
    /// Deletes every record that can't be decrypted.
    pub fn delete_undecryptable_records(&self) {}
    /// Checks the canary; never deletes anything.
    pub fn check_canary(&self) -> bool { true }
    fn run_blocking(&self) {}
}
"#;
        assert!(check(&RULE, &[MANIFEST, (PATH, src)]).is_empty());
    }
}
