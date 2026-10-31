//! Detect binding API changes and require a ⚠️ Breaking Changes entry
//!
//! Docs and examples: `rules/uniffi-api-break-gate/`.
//!
//! For every crate a change touches, the UniFFI surface of its changed UDL
//! and Rust files is built before and after the change (see
//! `helpers::uniffi`) and compared item by item. Only changes that stop
//! existing Kotlin/Swift/JS code from compiling are reported: removed or
//! renamed items and members, changed types and signatures, and new record
//! fields or arguments without a UniFFI default. Purely additive changes are
//! fine. A CHANGELOG.md line under a "Breaking" heading in the same change
//! silences the rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diff::{git, Diff, FileChange, Status};
use crate::helpers::uniffi::{self as api, Field, FnSig, Item, Member, Shape};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::uniffi::{ApiKind, MemberKind};

pub static RULE: Rule = Rule {
    id: "uniffi-api-break-gate",
    area: Area::Uniffi,
    title: "Detect binding API changes and require a ⚠️ Breaking Changes entry",
    kind: Kind::Diff,
    clippy: &[],
    in_tests: false,
    check,
};

const FIX: &str = "add a CHANGELOG.md entry under Breaking Changes (or keep the old API)";

fn check(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    if changelog_has_breaking_entry(diff) {
        return;
    }
    let mut sides: BTreeMap<String, Side> = BTreeMap::new();
    for change in &diff.files {
        let old_path = change.old_path.as_deref().unwrap_or(&change.path);
        if change.status != Status::Added && api::is_shipped_api_file(old_path) {
            if let Some(text) = &change.old_text {
                let side = sides
                    .entry(api::crate_dir(old_path).to_string())
                    .or_default();
                side.changed.insert(old_path.to_string());
                side.unreadable |= !read(old_path, text, &mut side.old, &mut side.old_aliases);
            }
        }
        if change.status != Status::Deleted && api::is_shipped_api_file(&change.path) {
            if let Some(text) = &change.new_text {
                let side = sides
                    .entry(api::crate_dir(&change.path).to_string())
                    .or_default();
                side.changed.insert(change.path.clone());
                side.unreadable |= !read(&change.path, text, &mut side.new, &mut side.new_aliases);
            }
        }
    }
    sides.retain(|_, side| !side.unreadable);
    if sides.values().all(|s| s.old.is_empty()) {
        return;
    }
    // `Result` aliases from the crate's unchanged files resolve error types.
    for (dir, side) in &mut sides {
        let mut base = Vec::new();
        for file in cx.ws.rust_files() {
            if api::crate_dir(&file.path) == dir && !side.changed.contains(&file.path) {
                base.extend(api::result_aliases(&file.ast));
            }
        }
        for found in [&mut side.old_aliases, &mut side.new_aliases] {
            api::add_aliases(found, base.clone());
        }
        api::resolve_aliases(&mut side.old, &side.old_aliases);
        api::resolve_aliases(&mut side.new, &side.new_aliases);
    }
    let merged: BTreeMap<&str, (Surface, Surface)> = sides
        .iter()
        .map(|(dir, side)| {
            (
                dir.as_str(),
                (api::merge(side.old.clone()), api::merge(side.new.clone())),
            )
        })
        .collect();
    let release = Release::find(&cx.ws.root, &diff.base);
    let mut breaks = Vec::new();
    for (dir, (old, new)) in &merged {
        if old.is_empty() {
            continue;
        }
        let released = release.as_ref().map(|r| r.api(dir));
        let gate = Gate {
            diff,
            released: released.as_ref(),
        };
        for (name, before) in old {
            if !gate.shipped(name, None) {
                continue;
            }
            match new.get(name) {
                Some(after) => gate.compare(before, after, &mut breaks),
                None => {
                    let moved_to = merged
                        .iter()
                        .find(|(d, (_, n))| d != &dir && n.contains_key(name))
                        .map(|(d, _)| *d);
                    let (path, line) = head_position(diff, &before.path, before.line);
                    let what = match moved_to {
                        Some(to) => format!(
                            "`{name}` moved from `{}` to `{}`, which changes its Kotlin package",
                            crate_name(dir),
                            crate_name(to)
                        ),
                        None => {
                            format!("{} `{name}` was removed or renamed", kind_name(before.kind))
                        }
                    };
                    breaks.push((path, line, what));
                }
            }
        }
    }
    for (path, line, what) in breaks {
        if what.contains("CHANGELOG.md") {
            out.add(path, line, what);
        } else {
            out.add(path, line, format!("{what}; {FIX}"));
        }
    }
}

/// A crate's items by exported name.
type Surface = BTreeMap<String, Item>;

#[derive(Default)]
struct Side {
    changed: BTreeSet<String>,
    /// A changed Rust file didn't parse: its items can't be compared.
    unreadable: bool,
    old: Vec<Item>,
    new: Vec<Item>,
    old_aliases: BTreeMap<String, Option<String>>,
    new_aliases: BTreeMap<String, Option<String>>,
}

/// Add the items of one file; `false` when it is Rust that doesn't parse.
fn read(
    path: &str,
    text: &str,
    items: &mut Vec<Item>,
    aliases: &mut BTreeMap<String, Option<String>>,
) -> bool {
    if path.ends_with(".udl") {
        items.extend(api::udl_items(path, text));
    } else {
        let Ok(file) = RustFile::parse(path, text.to_string()) else {
            return false;
        };
        items.extend(api::rust_items(&file));
        api::add_aliases(aliases, api::result_aliases(&file.ast));
    }
    true
}

/// The last release before the change: the most recent `Start release vN`
/// commit, whose tree is what the previous version shipped. API added since
/// then has not reached consumers, so changing it again breaks nobody.
struct Release {
    root: PathBuf,
    commit: String,
}

impl Release {
    fn find(root: &Path, base: &str) -> Option<Self> {
        if base.is_empty() {
            return None;
        }
        let log = git(
            root,
            &[
                "log",
                "--first-parent",
                "--format=%H%x09%s",
                "-n",
                "400",
                base,
            ],
        )
        .ok()?;
        let commit = log.lines().find_map(|line| {
            let (sha, subject) = line.split_once('\t')?;
            subject
                .starts_with("Start release v")
                .then(|| sha.to_string())
        })?;
        Some(Self {
            root: root.to_path_buf(),
            commit,
        })
    }

    /// The UniFFI surface of crate `dir` at the release.
    fn api(&self, dir: &str) -> BTreeMap<String, Item> {
        let mut items = Vec::new();
        let listing = git(
            &self.root,
            &["ls-tree", "-r", "--name-only", &self.commit, "--", dir],
        )
        .unwrap_or_default();
        for path in listing.lines().filter(|p| api::is_shipped_api_file(p)) {
            if let Ok(text) = git(&self.root, &["show", &format!("{}:{path}", self.commit)]) {
                read(path, &text, &mut items, &mut BTreeMap::new());
            }
        }
        api::merge(items)
    }
}

/// Compares one crate's items, knowing what shipped in the last release.
struct Gate<'a> {
    diff: &'a Diff,
    /// `None` when the release is unknown: everything counts as shipped.
    released: Option<&'a BTreeMap<String, Item>>,
}

impl Gate<'_> {
    /// Whether item `name` (or its `member`) was in the last release.
    fn shipped(&self, name: &str, member: Option<&Member>) -> bool {
        let Some(released) = self.released else {
            return true;
        };
        let Some(item) = released.get(name) else {
            return false;
        };
        member.is_none_or(|m| {
            item.members
                .iter()
                .any(|r| r.name == m.name && api::same_slot(r.kind, m.kind))
        })
    }

    /// Compare one item before and after, collecting what breaks callers.
    fn compare(&self, before: &Item, after: &Item, out: &mut Vec<(String, usize, String)>) {
        if before.decl && after.decl && !compatible_kinds(before, after) {
            out.push((
                after.path.clone(),
                after.line,
                format!(
                    "`{}` changed from {} to {}",
                    after.name,
                    with_article(kind_name(before.kind)),
                    with_article(kind_name(after.kind))
                ),
            ));
            return;
        }
        let foreign = before.foreign || after.foreign;
        for old in &before.members {
            if !self.shipped(&before.name, Some(old)) {
                continue;
            }
            let label = member_label(after, old);
            let Some(new) = after
                .members
                .iter()
                .find(|m| m.name == old.name && api::same_slot(m.kind, old.kind))
            else {
                let (path, line) = head_position(self.diff, &old.path, old.line);
                out.push((path, line, format!("{label} was removed or renamed")));
                continue;
            };
            let (problems, added) = match (&old.shape, &new.shape) {
                (Shape::Field(o), Shape::Field(n)) => (field_changes(o, n), Vec::new()),
                (Shape::Variant(o), Shape::Variant(n)) if !(before.flat || after.flat) => {
                    (variant_changes(o, n), first_added(o, n))
                }
                (Shape::Fn(o), Shape::Fn(n)) => {
                    (fn_changes(o, n, foreign), first_added(&o.args, &n.args))
                }
                _ => (Vec::new(), Vec::new()),
            };
            if !problems.is_empty() {
                // Point at the new field or argument when there is one.
                let line = added.first().copied().unwrap_or(new.line);
                out.push((
                    new.path.clone(),
                    line,
                    format!("{label}: {}", problems.join(", ")),
                ));
            }
        }
        // New members the foreign side must supply, grouped per item.
        let fresh: Vec<&Member> = after
            .members
            .iter()
            .filter(|n| {
                !before
                    .members
                    .iter()
                    .any(|m| m.name == n.name && api::same_slot(m.kind, n.kind))
            })
            .collect();
        let required_fields: Vec<&Member> = fresh
            .iter()
            .copied()
            .filter(|m| matches!(&m.shape, Shape::Field(f) if f.default.is_none()))
            .collect();
        if let Some(first) = required_fields.first() {
            out.push((
                first.path.clone(),
                first.line,
                format!(
                    "`{}` gains {} with no UniFFI default, so Kotlin/Swift code that builds it stops compiling; give {} a default or add a CHANGELOG.md entry under Breaking Changes",
                    after.name,
                    names(&required_fields, "field"),
                    if required_fields.len() == 1 { "it" } else { "them" },
                ),
            ));
        }
        let callbacks: Vec<&Member> = fresh
            .iter()
            .copied()
            .filter(|m| foreign && m.kind == MemberKind::Method)
            .collect();
        if let Some(first) = callbacks.first() {
            out.push((
                first.path.clone(),
                first.line,
                format!(
                    "{} `{}` gains {}, which every Kotlin/Swift/JS implementation must add",
                    kind_name(after.kind),
                    after.name,
                    names(&callbacks, "method"),
                ),
            ));
        }
    }
}

/// "field `a`" / "fields `a`, `b`".
fn names(members: &[&Member], noun: &str) -> String {
    let list: Vec<String> = members.iter().map(|m| format!("`{}`", m.name)).collect();
    if list.len() == 1 {
        format!("{noun} {}", list[0])
    } else {
        format!("{noun}s {}", list.join(", "))
    }
}

/// Lines of the fields or arguments in `new` that `old` doesn't have.
fn first_added(old: &[Field], new: &[Field]) -> Vec<usize> {
    new.iter()
        .filter(|n| !old.iter().any(|o| o.name == n.name))
        .map(|n| n.line)
        .filter(|l| *l > 0)
        .collect()
}

fn field_changes(old: &Field, new: &Field) -> Vec<String> {
    let mut out = Vec::new();
    if old.ty != new.ty {
        out.push(format!("type changed from `{}` to `{}`", old.ty, new.ty));
    }
    if old.default.is_some() && new.default.is_none() {
        out.push("lost its UniFFI default".into());
    }
    out
}

fn variant_changes(old: &[Field], new: &[Field]) -> Vec<String> {
    let names = |fields: &[Field]| fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    if names(old) != names(new) {
        let added: Vec<String> = new
            .iter()
            .filter(|n| !old.iter().any(|o| o.name == n.name))
            .map(|f| format!("`{}`", f.name))
            .collect();
        let removed: Vec<String> = old
            .iter()
            .filter(|o| !new.iter().any(|n| n.name == o.name))
            .map(|f| format!("`{}`", f.name))
            .collect();
        let mut out = Vec::new();
        if !added.is_empty() {
            out.push(format!(
                "gains {} (Swift code destructuring it breaks)",
                added.join(", ")
            ));
        }
        if !removed.is_empty() {
            out.push(format!("loses {}", removed.join(", ")));
        }
        if out.is_empty() {
            out.push("fields were reordered".into());
        }
        return out;
    }
    old.iter()
        .zip(new)
        .filter(|(o, n)| o.ty != n.ty)
        .map(|(o, n)| format!("`{}` changed type from `{}` to `{}`", o.name, o.ty, n.ty))
        .collect()
}

/// What changed in a function's signature that breaks callers. For a
/// foreign-implemented trait, implementers break on any argument change.
fn fn_changes(old: &FnSig, new: &FnSig, foreign: bool) -> Vec<String> {
    let mut out = Vec::new();
    if old.is_async != new.is_async {
        out.push(if new.is_async {
            "became async".to_string()
        } else {
            "is no longer async".to_string()
        });
    }
    if old.ret != new.ret {
        out.push(format!(
            "return type changed from `{}` to `{}`",
            display_ty(&old.ret),
            display_ty(&new.ret)
        ));
    }
    if !foreign {
        match (&old.throws, &new.throws) {
            (None, Some(_)) => out.push(format!(
                "now throws `{}` (Swift callers need `try`)",
                new.throws_known().unwrap_or("an error")
            )),
            (Some(_), Some(_)) => {
                if let (Some(o), Some(n)) = (old.throws_known(), new.throws_known()) {
                    if o != n {
                        out.push(format!("error type changed from `{o}` to `{n}`"));
                    }
                }
            }
            _ => {}
        }
    }
    for o in &old.args {
        match new.args.iter().find(|n| n.name == o.name) {
            None => out.push(format!("argument `{}` was removed or renamed", o.name)),
            Some(n) => {
                if o.ty != n.ty {
                    out.push(format!(
                        "argument `{}` changed type from `{}` to `{}`",
                        o.name, o.ty, n.ty
                    ));
                }
                if o.default.is_some() && n.default.is_none() && !foreign {
                    out.push(format!("argument `{}` lost its default", o.name));
                }
            }
        }
    }
    for n in &new.args {
        if old.args.iter().any(|o| o.name == n.name) {
            continue;
        }
        if foreign {
            out.push(format!("new argument `{}`", n.name));
        } else if n.default.is_none() {
            out.push(format!("new argument `{}` has no default", n.name));
        }
    }
    let common = |a: &[Field], b: &[Field]| -> Vec<String> {
        a.iter()
            .filter(|f| b.iter().any(|g| g.name == f.name))
            .map(|f| f.name.clone())
            .collect()
    };
    if common(&old.args, &new.args) != common(&new.args, &old.args) {
        out.push("arguments were reordered".into());
    }
    out
}

/// Kinds a consumer can't tell apart: a callback interface and a
/// `with_foreign` trait are both an interface to implement.
fn compatible_kinds(before: &Item, after: &Item) -> bool {
    if before.kind == after.kind {
        return true;
    }
    let to_implement =
        |i: &Item| i.kind == ApiKind::CallbackInterface || (i.kind == ApiKind::Trait && i.foreign);
    (to_implement(before) && to_implement(after))
        || before.kind == ApiKind::Custom
        || after.kind == ApiKind::Custom
}

fn member_label(item: &Item, member: &Member) -> String {
    match (item.kind, member.kind) {
        (ApiKind::Function, _) => format!("`{}`", member.name),
        (_, MemberKind::Variant) => format!("`{}::{}`", item.name, member.name),
        (_, MemberKind::Constructor) => format!("constructor `{}::{}`", item.name, member.name),
        _ => format!("`{}.{}`", item.name, member.name),
    }
}

fn kind_name(kind: ApiKind) -> &'static str {
    match kind {
        ApiKind::Record => "record",
        ApiKind::Enum => "enum",
        ApiKind::Object => "object",
        ApiKind::Error => "error",
        ApiKind::Function => "function",
        ApiKind::Trait => "trait",
        ApiKind::CallbackInterface => "callback interface",
        ApiKind::Custom => "custom type",
    }
}

fn with_article(name: &str) -> String {
    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!("{article} {name}")
}

fn display_ty(ty: &str) -> &str {
    if ty.is_empty() {
        "()"
    } else {
        ty
    }
}

fn crate_name(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or(dir)
}

/// Where something that was at `old_path:old_line` before the change sits
/// after it: the same file (or its new name) at the shifted line, or the
/// old position when the file is gone.
fn head_position(diff: &Diff, old_path: &str, old_line: usize) -> (String, usize) {
    let Some(change) = diff
        .files
        .iter()
        .find(|f| f.old_path.as_deref().unwrap_or(&f.path) == old_path)
    else {
        return (old_path.to_string(), old_line);
    };
    if change.status == Status::Deleted {
        return (old_path.to_string(), old_line);
    }
    (change.path.clone(), shift(change, old_line))
}

/// Map a line of the old file to the new one using the hunks' removed and
/// added lines.
fn shift(change: &FileChange, old_line: usize) -> usize {
    let removed_before = change.removed.iter().filter(|(l, _)| *l < old_line).count();
    let mut line = old_line.saturating_sub(removed_before).max(1);
    let mut added: Vec<usize> = change.added.iter().map(|(l, _)| *l).collect();
    added.sort_unstable();
    for at in added {
        if at <= line {
            line += 1;
        } else {
            break;
        }
    }
    let len = change
        .new_text
        .as_deref()
        .map(|t| t.lines().count())
        .unwrap_or(usize::MAX);
    line.min(len.max(1))
}

/// Whether CHANGELOG.md gains a non-heading line under a heading that says
/// "breaking" (`## ⚠️ Breaking Changes ⚠️`, `### Breaking`...), within the
/// release section it sits in.
pub fn changelog_has_breaking_entry(diff: &Diff) -> bool {
    let Some(change) = diff.file("CHANGELOG.md") else {
        return false;
    };
    let breaking = |l: &str| l.to_lowercase().contains("breaking");
    let Some(text) = &change.new_text else {
        return change.added_text().any(breaking);
    };
    let lines: Vec<&str> = text.lines().collect();
    change.added.iter().any(|(number, added)| {
        let added = added.trim();
        if added.is_empty() || added.starts_with('#') {
            return false;
        }
        for line in lines[..number.saturating_sub(1).min(lines.len())]
            .iter()
            .rev()
        {
            let heading = line.trim_start();
            let level = heading.chars().take_while(|c| *c == '#').count();
            if level == 0 || !heading[level..].starts_with(' ') {
                continue;
            }
            if breaking(heading) {
                return true;
            }
            if level <= 2 {
                return false;
            }
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::helpers::uniffi::texts_from_unified;
    use crate::testing::check_diff;

    const SUGGEST: &str = "components/suggest/src/lib.rs";

    fn run_fixture(text: &str) -> Vec<crate::rule::Finding> {
        let (old, new) = texts_from_unified(text);
        let old: Vec<(&str, &str)> = old.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let new: Vec<(&str, &str)> = new.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        check_diff(&RULE, &old, &new)
    }

    fn breaks(old: &str, new: &str) -> Vec<String> {
        check_diff(&RULE, &[(SUGGEST, old)], &[(SUGGEST, new)])
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn flags_dont() {
        let found = run_fixture(include_str!("../../rules/uniffi-api-break-gate/dont.diff"));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].path, "components/suggest/src/suggestion.rs");
        assert_eq!(found[0].line, 6);
        assert!(found[0].message.contains("`Suggestion::Amp`"));
        assert!(found[0].message.contains("`suggestion_id`"));
    }

    #[test]
    fn accepts_do() {
        let found = run_fixture(include_str!("../../rules/uniffi-api-break-gate/do.diff"));
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_removed_and_changed_functions() {
        let old = "#[uniffi::export]\npub fn a(x: String) {}\n#[uniffi::export]\npub fn b(x: String) {}\n#[uniffi::export]\npub fn c(x: String) -> u32 { 0 }\n";
        let new = "#[uniffi::export]\npub fn b(x: u32) {}\n#[uniffi::export]\npub fn c(x: String, y: bool) -> u32 { 0 }\n";
        let found = breaks(old, new);
        assert_eq!(found.len(), 3, "{found:#?}");
        let has = |s: &str| found.iter().any(|f| f.contains(s));
        assert!(has("function `a` was removed or renamed"), "{found:#?}");
        assert!(has("`b`: argument `x` changed type from `String` to `u32`"));
        assert!(has("`c`: new argument `y` has no default"));
    }

    #[test]
    fn accepts_additive_changes() {
        let old = r#"
#[derive(uniffi::Record)]
pub struct Query { pub keyword: String }
#[uniffi::export]
impl Store {
    pub fn query(&self, query: Query) -> Vec<String> { todo!() }
}
#[derive(uniffi::Enum)]
pub enum Provider { Amp }
"#;
        let new = r#"
/// Now documented.
#[derive(uniffi::Record)]
pub struct Query {
    pub keyword: String,
    #[uniffi(default)]
    pub limit: Option<u32>,
}
#[uniffi::export]
impl Store {
    #[uniffi::method(default(limit = None))]
    pub fn query(&self, query: &Query, limit: Option<u32>) -> Vec<String> { todo!() }
    pub fn clear(&self) {}
}
#[derive(uniffi::Enum)]
pub enum Provider { Amp, Wikipedia }
#[uniffi::export]
pub fn brand_new() {}
"#;
        assert!(breaks(old, new).is_empty(), "{:#?}", breaks(old, new));
    }

    #[test]
    fn ignores_non_exported_rust_api() {
        let old = "pub fn helper(x: String) {}\npub struct Internal { pub a: u8 }\n";
        let new = "pub fn helper(x: u32, y: u8) {}\n";
        assert!(breaks(old, new).is_empty());
    }

    #[test]
    fn udl_to_proc_macro_migration_is_not_a_break() {
        let udl = "namespace suggest {\n    [Throws=SuggestApiError]\n    sequence<string> keywords([ByRef] string prefix, optional u32? limit = null);\n};\ndictionary Query {\n    string keyword;\n    i32? limit = null;\n};\n";
        let rs = "pub type SuggestApiResult<T> = std::result::Result<T, SuggestApiError>;\n#[uniffi::export(default(limit = None))]\npub fn keywords(prefix: &str, limit: Option<u32>) -> SuggestApiResult<Vec<String>> { todo!() }\n#[derive(uniffi::Record)]\npub struct Query {\n    pub keyword: String,\n    #[uniffi(default = None)]\n    pub limit: Option<i32>,\n}\n";
        let udl_path = "components/suggest/src/suggest.udl";
        let found = check_diff(&RULE, &[(udl_path, udl), (SUGGEST, "")], &[(SUGGEST, rs)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_udl_changes() {
        let path = "components/places/src/places.udl";
        let old = "interface PlacesConnection {\n    [Throws=PlacesApiError]\n    void delete_visit(string url, i64 timestamp);\n    void ping();\n};\n";
        let new = "interface PlacesConnection {\n    [Throws=PlacesApiError]\n    void delete_visit(string url, i64 timestamp, boolean all);\n};\n";
        let found = check_diff(&RULE, &[(path, old)], &[(path, new)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert!(found[0]
            .message
            .contains("new argument `all` has no default"));
        assert!(
            found[1]
                .message
                .contains("`PlacesConnection.ping` was removed"),
            "{found:#?}"
        );
    }

    #[test]
    fn breaking_changelog_entry_silences() {
        let old = "#[uniffi::export]\npub fn gone() {}\n";
        let changelog_old = "# v2.0 (In progress)\n\n## Breaking Changes\n\n# v1.0\n";
        let changelog_new =
            "# v2.0 (In progress)\n\n## Breaking Changes\n\n### Suggest\n- Removed `gone()`.\n\n# v1.0\n";
        let found = check_diff(
            &RULE,
            &[(SUGGEST, old), ("CHANGELOG.md", changelog_old)],
            &[(SUGGEST, ""), ("CHANGELOG.md", changelog_new)],
        );
        assert!(found.is_empty(), "{found:#?}");
        // An entry under What's New does not count.
        let changelog_new =
            "# v2.0 (In progress)\n\n## Breaking Changes\n\n## What's New\n- Removed `gone()`.\n\n# v1.0\n";
        let found = check_diff(
            &RULE,
            &[(SUGGEST, old), ("CHANGELOG.md", changelog_old)],
            &[(SUGGEST, ""), ("CHANGELOG.md", changelog_new)],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    #[test]
    fn flat_errors_and_borrowed_args_are_not_breaks() {
        let old = "#[derive(uniffi::Error)]\n#[uniffi(flat_error)]\npub enum E { A { reason: String } }\n#[uniffi::export]\npub fn f(x: String, s: Arc<Store>) {}\n";
        let new = "#[derive(uniffi::Error)]\n#[uniffi(flat_error)]\npub enum E { A { reason: String, code: u16 } }\n#[uniffi::export]\npub fn f(x: &str, s: &Store) {}\n";
        assert!(breaks(old, new).is_empty(), "{:#?}", breaks(old, new));
    }

    #[test]
    fn flags_new_callback_methods() {
        let old = "#[uniffi::export(callback_interface)]\npub trait Sink: Send + Sync { fn on_event(&self, e: String); }\n";
        let new = "#[uniffi::export(callback_interface)]\npub trait Sink: Send + Sync {\n    fn on_event(&self, e: String);\n    fn flush(&self);\n}\n";
        let found = breaks(old, new);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("callback interface `Sink` gains method `flush`"));
    }

    #[test]
    fn changes_to_api_added_since_the_release_are_not_breaks() {
        use super::{Gate, Surface};
        use crate::helpers::uniffi as api;
        use crate::source::RustFile;
        let surface = |src: &str| -> Surface {
            let file = RustFile::parse(SUGGEST, src.to_string()).unwrap();
            api::merge(api::rust_items(&file))
        };
        let released = surface("#[uniffi::export]\nimpl Store {\n    pub fn old(&self) {}\n}\n");
        let before = surface("#[uniffi::export]\nimpl Store {\n    pub fn old(&self) {}\n    pub fn v3(&self, a: u8) {}\n}\n");
        let after = surface("#[uniffi::export]\nimpl Store {\n    pub fn old(&self, x: u8) {}\n    pub fn v3(&self, a: u16) {}\n}\n");
        let diff = crate::diff::Diff::default();
        let gate = Gate {
            diff: &diff,
            released: Some(&released),
        };
        let mut found = Vec::new();
        gate.compare(&before["Store"], &after["Store"], &mut found);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].2.contains("`Store.old`"));
        assert!(!gate.shipped("Brand", None));
    }

    #[test]
    fn skips_files_that_do_not_parse() {
        let old = "#[uniffi::export]\npub fn a() {}\n";
        assert!(breaks(old, "#[uniffi::export]\npub fn a( {}\n").is_empty());
    }

    #[test]
    fn ignores_test_code() {
        let old = "#[cfg(test)]\nmod tests {\n    #[uniffi::export]\n    pub fn helper() {}\n}\n";
        let new = "#[cfg(test)]\nmod tests {}\n";
        assert!(breaks(old, new).is_empty());
    }
}
