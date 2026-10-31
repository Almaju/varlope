//! Exported items get a `///` docstring (it is what Kotlin/Swift consumers see)
//!
//! Docs and examples: `rules/uniffi-docstrings/`.
//!
//! Looks at UniFFI items (types, functions, methods, constructors, callback
//! methods) through the `crate::uniffi` model. On a PR diff it only reports
//! items the change adds, since reviewers ask for docs when an API is added;
//! on the code as it is, it reports every undocumented exported item.

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::Status;
use crate::helpers::uniffi as api;
use crate::rule::{Area, Context, Edit, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::uniffi::{self, ApiItem, ApiKind, MemberKind};

pub static RULE: Rule = Rule {
    id: "uniffi-docstrings",
    area: Area::Uniffi,
    title: "Exported items get a `///` docstring (it is what Kotlin/Swift consumers see)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let mut crates: BTreeMap<String, Crate> = BTreeMap::new();
    let Some(diff) = cx.diff else {
        // The code as it is: every shipped API file, nothing "before".
        for file in cx.ws.files() {
            if api::is_shipped_api_file(&file.path) {
                crates
                    .entry(api::crate_dir(&file.path).to_string())
                    .or_default()
                    .1
                    .extend(
                        items(&file.path, &file.text)
                            .into_iter()
                            .map(|i| (i, file.text.clone())),
                    );
            }
        }
        report_missing(&crates, out);
        return;
    };
    for change in &diff.files {
        let old_path = change.old_path.as_deref().unwrap_or(&change.path);
        if change.status != Status::Added && api::is_shipped_api_file(old_path) {
            if let Some(text) = &change.old_text {
                let entry = crates
                    .entry(api::crate_dir(old_path).to_string())
                    .or_default();
                for item in items(old_path, text) {
                    entry.0.insert(item.name.clone());
                    for m in &item.members {
                        entry.0.insert(member_key(&item, m.kind, &m.name));
                    }
                }
            }
        }
        if change.status != Status::Deleted && api::is_shipped_api_file(&change.path) {
            if let Some(text) = &change.new_text {
                let entry = crates
                    .entry(api::crate_dir(&change.path).to_string())
                    .or_default();
                for item in items(&change.path, text) {
                    entry.1.push((item, text.clone()));
                }
            }
        }
    }
    report_missing(&crates, out);
}

fn report_missing(crates: &BTreeMap<String, Crate>, out: &mut Findings) {
    for (before, after) in crates.values() {
        for (item, text) in after {
            let lines: Vec<&str> = text.lines().collect();
            if !before.contains(&item.name) && !item.has_doc && item.kind != ApiKind::Custom {
                let what = if item.kind == ApiKind::Function {
                    format!("what `{}` does", item.name)
                } else {
                    format!("what `{}` is", item.name)
                };
                report(out, &item.path, item.line, &lines, &item.name, &what);
            }
            if item.kind == ApiKind::Function {
                continue; // its one member is the function itself
            }
            let missing: Vec<_> = item
                .members
                .iter()
                .filter(|m| matches!(m.kind, MemberKind::Method | MemberKind::Constructor))
                .filter(|m| !m.has_doc && !before.contains(&member_key(item, m.kind, &m.name)))
                .collect();
            match missing.as_slice() {
                [] => {}
                [m] => {
                    let name = if m.kind == MemberKind::Constructor {
                        format!("{} constructor", item.name)
                    } else {
                        format!("{}.{}", item.name, m.name)
                    };
                    report(
                        out,
                        &item.path,
                        m.line,
                        &lines,
                        &name,
                        &member_what(item, m),
                    );
                }
                [first, ..] => {
                    let names: Vec<String> =
                        missing.iter().map(|m| format!("`{}`", m.name)).collect();
                    let message = format!(
                        "`{}` has {} exported methods without a `///` docstring ({}); it is the only documentation Kotlin/Swift/JS callers see",
                        item.name,
                        missing.len(),
                        names.join(", ")
                    );
                    // One docstring per method, all or none.
                    let fixes: Option<Vec<Fix>> = missing
                        .iter()
                        .map(|m| doc_fix(&item.path, &lines, m.line, &member_what(item, m)))
                        .collect();
                    match fixes {
                        Some(fixes) => {
                            let mut edits: Vec<Edit> =
                                fixes.into_iter().flat_map(|f| f.edits).collect();
                            edits.sort_by_key(|e| e.line);
                            out.add_fix(&item.path, first.line, message, Fix { edits });
                        }
                        None => out.add(&item.path, first.line, message),
                    }
                }
            }
        }
    }
}

/// Item and member names before the change, and items (with their file's
/// text) after it.
type Crate = (BTreeSet<String>, Vec<(ApiItem, String)>);

/// The exported items of a file, per the `crate::uniffi` model, minus test
/// code.
fn items(path: &str, text: &str) -> Vec<ApiItem> {
    if path.ends_with(".udl") {
        return uniffi::from_udl(path, text);
    }
    let Ok(file) = RustFile::parse(path, text.to_string()) else {
        return Vec::new();
    };
    uniffi::from_rust(&file)
        .into_iter()
        .filter(|i| !file.in_test(i.line))
        .map(|mut i| {
            // An Object's struct fields are private, not API.
            if i.kind == ApiKind::Object {
                i.members.retain(|m| m.kind != MemberKind::Field);
            }
            i
        })
        .collect()
}

fn member_key(item: &ApiItem, kind: MemberKind, name: &str) -> String {
    // A UDL `constructor` is the proc-macro `new`.
    let name = if kind == MemberKind::Constructor && name == "constructor" {
        "new"
    } else {
        name
    };
    format!("{}::{kind:?}::{name}", item.name)
}

/// What a method's docstring should say: `what `query` does`.
fn member_what(item: &ApiItem, m: &uniffi::ApiMember) -> String {
    if m.kind == MemberKind::Constructor {
        format!("what the `{}` constructor does", item.name)
    } else {
        format!("what `{}` does", m.name)
    }
}

fn report(out: &mut Findings, path: &str, line: usize, lines: &[&str], name: &str, what: &str) {
    let message = if comment_above(lines, line) {
        format!("the `//` comment on exported `{name}` is dropped from the bindings; make it a `///` docstring")
    } else {
        format!("exported `{name}` has no `///` docstring; it is the only documentation Kotlin/Swift/JS callers see")
    };
    match doc_fix(path, lines, line, what) {
        Some(fix) => out.add_fix(path, line, message, fix),
        None => out.add(path, line, message),
    }
}

/// Whether a plain `//` comment sits right above `line`, past attributes.
fn comment_above(lines: &[&str], line: usize) -> bool {
    let mut l = line.saturating_sub(1);
    while l >= 1 {
        let text = lines.get(l - 1).map(|t| t.trim()).unwrap_or("");
        let attribute = (text.starts_with("#[") || text.starts_with('['))
            && (text.ends_with(']') || text.ends_with(")]"));
        if attribute {
            l -= 1;
            continue;
        }
        return text.starts_with("//") && !text.starts_with("///") && !text.starts_with("//!");
    }
    false
}

/// A docstring for the item on `line`: the `//` comment above it turned
/// into `///` lines when it reads as documentation, else a placeholder line
/// naming `what` to describe, above the item's attributes and indented like
/// the item.
fn doc_fix(path: &str, lines: &[&str], line: usize, what: &str) -> Option<Fix> {
    let top = attributes_top(lines, line, path.ends_with(".udl"))?;
    // The comment block right above the attributes.
    let mut first = top;
    while first > 1 {
        let text = lines.get(first - 2)?.trim();
        if !text.starts_with("//") || text.starts_with("//!") {
            break;
        }
        first -= 1;
    }
    let block = &lines[first - 1..top - 1];
    if block.iter().any(|l| !l.trim().starts_with("///")) && block.iter().all(|l| is_prose(l)) {
        let docs: Vec<String> = block.iter().map(|l| to_doc(l)).collect();
        let changed: Vec<usize> = (0..block.len()).filter(|&i| docs[i] != block[i]).collect();
        let (&a, &b) = (changed.first()?, changed.last()?);
        return Some(Fix::edit(first + a, b - a + 1, docs[a..=b].to_vec()));
    }
    let item = lines.get(line - 1)?;
    let indent = &item[..item.len() - item.trim_start().len()];
    Some(Fix::insert_before(
        top,
        vec![format!("{indent}/// … {what}.")],
    ))
}

/// The first line of the attributes above `line` (`line` itself when there
/// are none). `None` when an attribute's start can't be found.
fn attributes_top(lines: &[&str], line: usize, udl: bool) -> Option<usize> {
    let open = if udl { "[" } else { "#[" };
    let mut top = line;
    while top > 1 {
        let text = lines.get(top - 2)?.trim();
        if text.starts_with(open) && text.ends_with(']') {
            top -= 1;
            continue;
        }
        if udl || !text.ends_with(']') || text.starts_with("//") {
            break;
        }
        // The end of an attribute over several lines: find its `#[`.
        let mut depth = 0i32;
        let mut l = top - 1;
        loop {
            let t = lines.get(l - 1)?.trim();
            if t.is_empty() || t.starts_with("//") || t.ends_with([';', '{', '}']) {
                return None;
            }
            depth += t.matches(']').count() as i32 - t.matches('[').count() as i32;
            if t.starts_with("#[") && depth == 0 {
                break;
            }
            if l == 1 || top - l > 20 {
                return None;
            }
            l -= 1;
        }
        top = l;
    }
    Some(top)
}

/// Whether a `//` line reads as documentation, not code or a work note.
fn is_prose(line: &str) -> bool {
    let text = line.trim().trim_start_matches('/').trim();
    let code = text.starts_with("#[")
        || text.ends_with(';')
        || text.ends_with('{')
        || text.ends_with('}')
        || [
            "let ", "fn ", "pub ", "use ", "impl ", "mod ", "struct ", "enum ",
        ]
        .iter()
        .any(|k| text.starts_with(k));
    let note = ["TODO", "FIXME", "XXX", "SAFETY", "This Source Code Form"]
        .iter()
        .any(|m| text.contains(m));
    !code && !note
}

/// `// text` -> `/// text`.
fn to_doc(line: &str) -> String {
    let indent = &line[..line.len() - line.trim_start().len()];
    let text = line.trim_start();
    if text.starts_with("///") {
        return line.to_string();
    }
    let rest = text.trim_start_matches('/');
    if rest.is_empty() || rest.starts_with(' ') {
        format!("{indent}///{rest}")
    } else {
        format!("{indent}/// {rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::uniffi::texts_from_unified;
    use crate::testing::check_diff;

    fn run_fixture(text: &str) -> Vec<crate::rule::Finding> {
        let (old, new) = texts_from_unified(text);
        let old: Vec<(&str, &str)> = old.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let new: Vec<(&str, &str)> = new.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        check_diff(&RULE, &old, &new)
    }

    const PATH: &str = "components/tabs/src/lib.rs";

    #[test]
    fn flags_dont() {
        let found = run_fixture(include_str!("../../rules/uniffi-docstrings/dont.diff"));
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 5);
        assert!(found[0].message.contains("`//` comment"));
        assert_eq!(found[1].line, 8);
        assert!(found[1].message.contains("`LocalTabsInfo` has no `///`"));
        // The comment becomes the docstring; a placeholder names what to document.
        assert_eq!(
            edits(&found[0]),
            vec![(
                4,
                1,
                vec!["    /// Sets the local tabs, with their window and group info.".into()]
            )]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(8, 0, vec!["/// … what `LocalTabsInfo` is.".into()])]
        );
    }

    fn edits(f: &crate::rule::Finding) -> Vec<(usize, usize, Vec<String>)> {
        f.fix
            .iter()
            .flat_map(|fix| &fix.edits)
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    #[test]
    fn places_docstrings_above_attributes() {
        let new = "#[derive(\n    Debug,\n    uniffi::Record,\n)]\npub struct Query {\n    pub keyword: String,\n}\n\n// #[non_exhaustive]\n#[derive(Debug, uniffi::Error)]\npub enum QueryError {}\n\n// Builds the query.\n//\n//  `keyword` may be empty.\n#[uniffi::export]\npub fn query(keyword: String) {}\n";
        let found = check_diff(&RULE, &[(PATH, "")], &[(PATH, new)]);
        let all: Vec<_> = found.iter().map(edits).collect();
        assert_eq!(
            all,
            vec![
                vec![(1, 0, vec!["/// … what `Query` is.".into()])],
                // Commented-out code stays a comment.
                vec![(10, 0, vec!["/// … what `QueryError` is.".into()])],
                vec![(
                    13,
                    3,
                    vec![
                        "/// Builds the query.".into(),
                        "///".into(),
                        "///  `keyword` may be empty.".into(),
                    ]
                )],
            ],
            "{found:#?}"
        );
    }

    #[test]
    fn accepts_do() {
        let found = run_fixture(include_str!("../../rules/uniffi-docstrings/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_new_proc_macro_items_only() {
        let old = "#[uniffi::export]\npub fn old_undocumented() {}\n";
        let new = "#[uniffi::export]\npub fn old_undocumented() {}\n\n#[derive(uniffi::Record)]\npub struct Query { pub keyword: String }\n\n#[uniffi::export]\nimpl Store {\n    /// Documented.\n    #[uniffi::constructor]\n    pub fn new() -> Self { todo!() }\n    #[handle_error(Error)]\n    pub fn query(&self) -> ApiResult<()> { todo!() }\n    fn private_helper(&self) {}\n}\n";
        let found = check_diff(&RULE, &[(PATH, old)], &[(PATH, new)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 13], "{found:#?}");
    }

    #[test]
    fn groups_undocumented_methods_of_one_item() {
        let new = "/// A store.\n#[derive(uniffi::Object)]\npub struct Store;\n#[uniffi::export]\nimpl Store {\n    pub fn a(&self) {}\n    pub fn b(&self) {}\n}\n";
        let found = check_diff(&RULE, &[(PATH, "")], &[(PATH, new)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].message.contains("2 exported methods"));
        // One placeholder per method.
        assert_eq!(
            edits(&found[0]),
            vec![
                (6, 0, vec!["    /// … what `a` does.".into()]),
                (7, 0, vec!["    /// … what `b` does.".into()]),
            ]
        );
    }

    #[test]
    fn udl_constructor_is_the_proc_macro_new() {
        let udl = (
            "components/tabs/src/tabs.udl",
            "interface Store {\n    constructor();\n};\n",
        );
        let rs = "/// A store.\n#[derive(uniffi::Object)]\npub struct Store;\n#[uniffi::export]\nimpl Store {\n    #[uniffi::constructor]\n    pub fn new() -> Self { Store }\n}\n";
        let found = check_diff(&RULE, &[udl, (PATH, "")], &[(PATH, rs)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn moved_items_are_not_new() {
        let a = "/// Docs.\n#[uniffi::export]\npub fn moved() {}\n";
        let found = check_diff(
            &RULE,
            &[(PATH, "#[uniffi::export]\npub fn moved() {}\n")],
            &[(PATH, ""), ("components/tabs/src/other.rs", a)],
        );
        assert!(found.is_empty(), "{found:#?}");
        let found = check_diff(
            &RULE,
            &[(PATH, "#[uniffi::export]\npub fn moved() {}\n")],
            &[
                (PATH, ""),
                (
                    "components/tabs/src/other.rs",
                    "#[uniffi::export]\npub fn moved() {}\n",
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_test_code() {
        let new = "#[cfg(test)]\nmod tests {\n    #[uniffi::export]\n    pub fn helper() {}\n}\n";
        assert!(check_diff(&RULE, &[(PATH, "")], &[(PATH, new)]).is_empty());
    }
}
