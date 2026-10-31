//! Hand-written Kotlin/Swift wrappers stay in sync with the Rust API and don't redefine UDL types
//!
//! Docs and examples: `rules/wrappers-in-sync/`.
//!
//! Reads the hand-written Kotlin (`components/*/android/src/main/`) and
//! Swift (`components/*/ios/`) code next to each component and compares it
//! with the component's UniFFI surface (plus the components it imports):
//! - an `enum class` / `data class` / `enum` / `struct` with the name, the
//!   variants or the fields of a generated type redefines it;
//! - a thin wrapper `fun foo(..)` that calls the generated `foo` with a
//!   literal for a parameter Rust exposes hard-codes it;
//! - a wrapper parameter whose default differs from the UniFFI default of
//!   the same Rust parameter re-defaults it.

use std::collections::BTreeMap;

use regex::Regex;

use crate::helpers::uniffi::{self as api, Item, Shape};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;
use crate::uniffi::ApiKind;

pub static RULE: Rule = Rule {
    id: "wrappers-in-sync",
    area: Area::Mobile,
    title: "Hand-written Kotlin/Swift wrappers stay in sync with the Rust API and don't redefine UDL types",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    // crate dir -> its UniFFI items
    let mut apis: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for file in cx.ws.files() {
        if !api::is_shipped_api_file(&file.path) {
            continue;
        }
        let items = if file.extension() == "udl" {
            api::udl_items(&file.path, &file.text)
        } else if let Some(rust) = cx.ws.rust(&file.path) {
            api::rust_items(rust)
        } else {
            continue;
        };
        apis.entry(api::crate_dir(&file.path).to_string())
            .or_default()
            .extend(items);
    }
    for file in cx.ws.files() {
        let Some(dir) = wrapper_crate(&file.path) else {
            continue;
        };
        let Some(own) = apis.get(dir) else { continue };
        let mut items: Vec<Item> = own.clone();
        // Types from other components the wrapper imports.
        for (other, other_items) in &apis {
            if other != dir && imports(&file.text, other) {
                items.extend(other_items.iter().cloned());
            }
        }
        let items = api::merge(items);
        let code = strip_comments(&file.text);
        duplicated_types(file, &code, &items, out);
        wrapper_functions(file, &code, own, out);
    }
}

/// `components/fxa-client` for a hand-written Kotlin or Swift file of it.
fn wrapper_crate(path: &str) -> Option<&str> {
    if !path.starts_with("components/") || path.contains("/generated/") {
        return None;
    }
    let kotlin = path.ends_with(".kt") && path.contains("/android/src/main/");
    let swift = path.ends_with(".swift") && path.contains("/ios/");
    if !(kotlin || swift) || path.to_lowercase().contains("test") {
        return None;
    }
    let marker = if kotlin { "/android/" } else { "/ios/" };
    path.find(marker).map(|i| &path[..i])
}

/// Whether the file imports the Kotlin/Swift module of crate `dir`.
fn imports(text: &str, dir: &str) -> bool {
    let name: String = dir
        .rsplit('/')
        .next()
        .unwrap_or(dir)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    text.lines().any(|l| {
        let l = l.trim();
        l.starts_with("import ")
            && l.to_lowercase()
                .replace('_', "")
                .contains(&format!(".{name}."))
    })
}

/// A Kotlin/Swift `enum class` / `data class` / `enum` / `struct` that
/// redefines a generated type: same name, same variants or same fields.
fn duplicated_types(
    file: &TextFile,
    code: &str,
    items: &BTreeMap<String, Item>,
    out: &mut Findings,
) {
    let decl = Regex::new(
        r"(?m)^\s*(?:(?:public|internal|private)\s+)?(enum class|data class|enum|struct)\s+(\w+)",
    )
    .expect("valid regex");
    for c in decl.captures_iter(code) {
        let keyword = c.get(1).expect("group").as_str();
        let name = c.get(2).expect("group").as_str();
        let start = c.get(0).expect("match").end();
        let line = file.line_of(c.get(2).expect("group").start());
        let is_enum = keyword.starts_with("enum");
        let members = if is_enum {
            enum_entries(code, start)
        } else {
            data_fields(code, start, keyword == "struct")
        };
        let found = items.values().find_map(|item| {
            let kinds: &[ApiKind] = if is_enum {
                &[ApiKind::Enum, ApiKind::Error]
            } else {
                &[ApiKind::Record]
            };
            if !kinds.contains(&item.kind) {
                return None;
            }
            if item.name == name {
                return Some((item, "same name"));
            }
            let theirs: Vec<String> = item.members.iter().map(|m| normal(&m.name)).collect();
            let same_set = members.len() >= 2
                && members.len() == theirs.len()
                && members.iter().all(|m| theirs.contains(m));
            same_set.then_some((
                item,
                if is_enum {
                    "same variants"
                } else {
                    "same fields"
                },
            ))
        });
        if let Some((item, why)) = found {
            out.add(
                &file.path,
                line,
                format!(
                    "`{name}` redefines the generated `{}` ({why}); use the UniFFI type, or define the type once in Rust",
                    item.name
                ),
            );
        }
    }
}

/// Entry names of `enum class X { A, B("x"); ... }` / Swift `enum X { case a, b }`.
fn enum_entries(code: &str, from: usize) -> Vec<String> {
    let Some(body) = braced(code, from) else {
        return Vec::new();
    };
    let body = body.split(';').next().unwrap_or("");
    let mut out = Vec::new();
    for part in split_top(body, &[',', '\n']) {
        let part = part.trim().trim_start_matches("case ").trim();
        let name: String = part
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && !matches!(name.as_str(), "fun" | "val" | "var" | "companion") {
            out.push(normal(&name));
        }
    }
    out
}

/// Property names of `data class X(val a: T, val b: T)` / Swift `struct X { let a: T }`.
fn data_fields(code: &str, from: usize, swift: bool) -> Vec<String> {
    let props = Regex::new(r"(?:val|var|let)\s+(\w+)\s*:").expect("valid regex");
    let text = if swift {
        braced(code, from)
    } else {
        parenthesized(code, from)
    };
    text.map(|t| props.captures_iter(&t).map(|c| normal(&c[1])).collect())
        .unwrap_or_default()
}

/// Thin wrappers: `fun getDevices(ignoreCache: Boolean = true)` calling
/// `inner.getDevices(..)`.
fn wrapper_functions(file: &TextFile, code: &str, own: &[Item], out: &mut Findings) {
    let funs = Regex::new(r"(?:fun|func)\s+(?:<[^>]*>\s*)?(\w+)\s*\(").expect("valid regex");
    let starts: Vec<(usize, usize, String)> = funs
        .captures_iter(code)
        .map(|c| {
            let m = c.get(0).expect("match");
            (m.start(), m.end() - 1, c[1].to_string())
        })
        .collect();
    for (i, (start, open, name)) in starts.iter().enumerate() {
        let Some(params_text) = parenthesized(code, *open) else {
            continue;
        };
        let body_end = starts.get(i + 1).map(|s| s.0).unwrap_or(code.len());
        let body =
            &code[*open + params_text.len() + 2..body_end.max(*open + params_text.len() + 2)];
        let rust_name = snake(name);
        let candidates: Vec<&[api::Field]> = own
            .iter()
            .flat_map(|item| &item.members)
            .filter(|m| m.name == rust_name)
            .filter_map(|m| match &m.shape {
                Shape::Fn(sig) => Some(sig.args.as_slice()),
                _ => None,
            })
            .collect();
        let Some(args) = candidates.first() else {
            continue;
        };
        let Some((call_at, call)) = call_args(body, name) else {
            continue; // not a pass-through to the generated function
        };
        let body_start = *open + params_text.len() + 2;
        let call_line = file.line_of(body_start + call_at);
        let line = file.line_of(*start);
        let params = params(&params_text);
        // Re-defaulted parameters.
        for (param, default) in &params {
            let Some(default) = default else { continue };
            let Some(arg) = args.iter().find(|a| a.name == snake(param)) else {
                continue;
            };
            let (Some(theirs), Some(ours)) =
                (arg.default.as_deref().and_then(literal), literal(default))
            else {
                continue;
            };
            if theirs != ours {
                out.add(
                    &file.path,
                    line,
                    format!(
                        "`{name}` defaults `{param}` to `{default}`, but the Rust API defaults `{}` to `{}`; use the same default so apps get the same behaviour",
                        arg.name,
                        arg.default.as_deref().unwrap_or_default()
                    ),
                );
            }
        }
        // Hard-coded parameters.
        for (index, value) in call.iter().enumerate() {
            let (arg_name, value) = match value.split_once('=') {
                Some((n, v)) if !n.contains(['(', '"']) && !v.starts_with('=') => {
                    (Some(n.trim().to_string()), v.trim())
                }
                _ => (None, value.trim()),
            };
            if literal(value).is_none() {
                continue;
            }
            let rust_arg = match &arg_name {
                Some(n) => args.iter().find(|a| a.name == snake(n)),
                None => args.get(index),
            };
            let Some(rust_arg) = rust_arg else { continue };
            if params.iter().any(|(p, _)| snake(p) == rust_arg.name) {
                continue;
            }
            out.add(
                &file.path,
                call_line,
                format!(
                    "`{name}` hard-codes `{value}` for `{}`, which the Rust API exposes; take it as a parameter (with the Rust default) so apps can choose",
                    rust_arg.name
                ),
            );
        }
    }
}

/// `(name, default)` of each parameter in a parameter list.
fn params(text: &str) -> Vec<(String, Option<String>)> {
    split_top(text, &[','])
        .into_iter()
        .filter_map(|p| {
            let p = p.trim();
            let (decl, default) = match p.split_once('=') {
                Some((d, v)) => (d, Some(v.trim().to_string())),
                None => (p, None),
            };
            let name = decl
                .split(':')
                .next()?
                .split_whitespace()
                .last()?
                .to_string();
            (!name.is_empty()).then_some((name, default))
        })
        .collect()
}

/// Where the first `.name(..)` call in `body` is, and its arguments.
fn call_args(body: &str, name: &str) -> Option<(usize, Vec<String>)> {
    let needle = format!(".{name}(");
    let at = body.find(&needle)?;
    let inner = parenthesized(body, at + needle.len() - 1)?;
    let args = split_top(&inner, &[','])
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some((at, args))
}

/// A literal value in a comparable form: `true`, `false`, `null`, numbers,
/// strings, empty collections. Kotlin, Swift, Rust and UDL spellings agree.
fn literal(text: &str) -> Option<String> {
    let t = text.trim();
    match t {
        "true" | "false" => return Some(t.into()),
        "null" | "nil" | "None" => return Some("null".into()),
        "listOf()" | "emptyList()" | "[]" | "vec![]" | "Vec::new()" => return Some("[]".into()),
        "mapOf()" | "emptyMap()" | "[:]" | "{}" | "HashMap::new()" => return Some("{}".into()),
        _ => {}
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return Some(t.into());
    }
    let digits: String = t
        .trim_end_matches(['L', 'U', 'u', 'f'])
        .chars()
        .filter(|c| *c != '_')
        .collect();
    (!digits.is_empty() && digits.parse::<f64>().is_ok()).then_some(digits)
}

/// The text inside the `{..}` that follows `from`.
fn braced(code: &str, from: usize) -> Option<String> {
    let open = code[from..].find('{')? + from;
    // Stop at a declaration without a body.
    if code[from..open].contains(['\n']) && code[from..open].trim().is_empty() {
        return None;
    }
    enclosed(code, open, '{', '}')
}

/// The text inside the `(..)` that follows `from`.
fn parenthesized(code: &str, from: usize) -> Option<String> {
    let open = code[from..].find('(')? + from;
    if !code[from..open].trim().is_empty() && !code[from..open].trim().starts_with('<') {
        return None;
    }
    enclosed(code, open, '(', ')')
}

fn enclosed(code: &str, open: usize, left: char, right: char) -> Option<String> {
    let mut depth = 0;
    let mut in_string = false;
    for (i, c) in code[open..].char_indices() {
        if in_string {
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            c if c == left => depth += 1,
            c if c == right => {
                depth -= 1;
                if depth == 0 {
                    return Some(code[open + 1..open + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Split on any of `seps` outside brackets and strings.
fn split_top<'a>(text: &'a str, seps: &[char]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if in_string {
            in_string = c != '"';
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            c if depth == 0 && seps.contains(&c) => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// Replace comments with spaces, keeping offsets and newlines.
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = text[i..].chars().next().expect("char");
        let len = c.len_utf8();
        if in_string {
            in_string = c != '"';
            out.push(c);
        } else if text[i..].starts_with("//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        } else if text[i..].starts_with("/*") {
            let end = text[i + 2..]
                .find("*/")
                .map(|e| i + 2 + e + 2)
                .unwrap_or(text.len());
            for b in text[i..end].bytes() {
                out.push(if b == b'\n' { '\n' } else { ' ' });
            }
            i = end;
            continue;
        } else {
            in_string = c == '"';
            out.push(c);
        }
        i += len;
    }
    out
}

/// `getDevices` -> `get_devices`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A member name compared across languages: `DESKTOP`, `Desktop`, `desktop`
/// and `ccNumber` / `cc_number` all match.
fn normal(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const RUST: (&str, &str) = (
        "components/fxa-client/src/lib.rs",
        "#[derive(uniffi::Enum)]\npub enum DeviceType { Desktop, Mobile, Tablet }\n\
         #[uniffi::export]\nimpl FirefoxAccount {\n    #[uniffi::method(default(ignore_cache = false))]\n    pub fn get_devices(&self, ignore_cache: bool) -> Vec<Device> { todo!() }\n}\n",
    );
    const KT: &str =
        "components/fxa-client/android/src/main/java/mozilla/appservices/fxaclient/FxaClient.kt";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                RUST,
                (KT, include_str!("../../rules/wrappers-in-sync/dont.kt")),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 5], "{found:#?}");
        assert!(found[0]
            .message
            .contains("redefines the generated `DeviceType`"));
        assert!(found[1]
            .message
            .contains("hard-codes `false` for `ignore_cache`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                RUST,
                (KT, include_str!("../../rules/wrappers-in-sync/do.kt")),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_re_defaulted_parameters() {
        let kt = "class FxaClient(private val inner: FirefoxAccount) {\n    fun getDevices(ignoreCache: Boolean = true) = inner.getDevices(ignoreCache)\n}\n";
        let found = check(&RULE, &[RUST, (KT, kt)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0]
            .message
            .contains("defaults `ignoreCache` to `true`"));
    }

    #[test]
    fn ignores_wrappers_that_add_behaviour_and_typealiases() {
        let kt = "typealias Device = DeviceType\n\
                  enum class KeyRegenerationEventReason { Lost, Corrupt, Other }\n\
                  class FxaClient(private val inner: FirefoxAccount) {\n    fun refreshDevices(): List<Device> = inner.getDevices(true)\n}\n";
        let found = check(&RULE, &[RUST, (KT, kt)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
