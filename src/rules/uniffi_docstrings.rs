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
use crate::rule::{Area, Context, Findings, Kind, Rule};
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
                report(out, &item.path, item.line, &lines, &item.name);
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
                    report(out, &item.path, m.line, &lines, &name);
                }
                [first, ..] => {
                    let names: Vec<String> =
                        missing.iter().map(|m| format!("`{}`", m.name)).collect();
                    out.add(
                        &item.path,
                        first.line,
                        format!(
                            "`{}` has {} exported methods without a `///` docstring ({}); it is the only documentation Kotlin/Swift/JS callers see",
                            item.name,
                            missing.len(),
                            names.join(", ")
                        ),
                    );
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

fn report(out: &mut Findings, path: &str, line: usize, lines: &[&str], name: &str) {
    let message = if comment_above(lines, line) {
        format!("the `//` comment on exported `{name}` is dropped from the bindings; make it a `///` docstring")
    } else {
        format!("exported `{name}` has no `///` docstring; it is the only documentation Kotlin/Swift/JS callers see")
    };
    out.add(path, line, message);
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
