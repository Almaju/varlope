//! Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`
//!
//! Docs and examples: `rules/parking-lot-locks/`.
//!
//! Flags `use std::sync::{Mutex, RwLock}` imports and types written as
//! `std::sync::Mutex<..>` in shipped component code (clippy's
//! `disallowed_types` does the same with full type resolution). A file that
//! uses `Condvar` keeps the std mutex it needs.

use std::cmp::Ordering;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::errors::{self, TestSupport};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;
use crate::workspace::Workspace;

pub static RULE: Rule = Rule {
    id: "parking-lot-locks",
    area: Area::Concurrency,
    title: "Use `parking_lot` locks, not `std::sync::Mutex`/`RwLock`",
    kind: Kind::Code,
    clippy: &["clippy::disallowed_types"],
    in_tests: false,
    check,
};

const STD_LOCKS: &[&str] = &["Mutex", "RwLock"];

/// What a fix moves from `std::sync` to `parking_lot`: the locks and their
/// guards, which come together.
const MOVED: &[&str] = &[
    "Mutex",
    "MutexGuard",
    "RwLock",
    "RwLockReadGuard",
    "RwLockWriteGuard",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        if file.text.contains("Condvar") {
            continue;
        }
        let support = TestSupport::of(file);
        let mut v = StdLocks { hits: Vec::new() };
        v.visit_file(&file.ast);
        for Hit {
            line,
            column,
            names,
            site,
        } in v.hits
        {
            if !support.contains(line) {
                let std: Vec<String> = names.iter().map(|n| format!("`std::sync::{n}`")).collect();
                let pl: Vec<String> = names
                    .iter()
                    .map(|n| format!("`parking_lot::{n}`"))
                    .collect();
                out.add_at(
                    &file.path,
                    line,
                    column,
                    format!(
                        "{} {} on panic and {} `.lock().unwrap()`; use {}",
                        std.join(" and "),
                        if names.len() > 1 { "poison" } else { "poisons" },
                        if names.len() > 1 { "need" } else { "needs" },
                        pl.join(" and ")
                    ),
                );
                let fix = match site {
                    Site::Use(u) => use_fix(cx.ws, file, u),
                    Site::Lines(first, last) => path_fix(file, first, last),
                };
                if let (Some(fix), Some(last)) = (fix, out.items.last_mut()) {
                    last.fix = Some(fix);
                }
            }
        }
    }
}

struct Hit<'a> {
    line: usize,
    column: usize,
    names: Vec<String>,
    site: Site<'a>,
}

/// What a fix rewrites.
enum Site<'a> {
    /// A `use` item, split into a `parking_lot` import and what stays.
    Use(&'a syn::ItemUse),
    /// Lines naming `std::sync::Mutex`: a type, or a macro's lines.
    Lines(usize, usize),
}

struct StdLocks<'a> {
    hits: Vec<Hit<'a>>,
}

/// `std::sync::Mutex` -> `parking_lot::Mutex` (and `RwLock`, guards) on lines
/// `first..=last`.
fn path_fix(file: &RustFile, first: usize, last: usize) -> Option<Fix> {
    let mut lines: Vec<String> = (first..=last).map(|n| file.line(n).to_string()).collect();
    let original = lines.clone();
    for line in &mut lines {
        for lock in STD_LOCKS {
            *line = line.replace(
                &format!("std::sync::{lock}"),
                &format!("parking_lot::{lock}"),
            );
        }
    }
    // Only the lines that change, if rustfmt keeps them as they are.
    let changed: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i] != original[i])
        .collect();
    if changed.iter().any(|&i| lines[i].chars().count() > 100) {
        return None;
    }
    let (&a, &b) = (changed.first()?, changed.last()?);
    Some(Fix::edit(first + a, b - a + 1, lines[a..=b].to_vec()))
}

/// Split `use std::sync::{Arc, Mutex};` into `use parking_lot::Mutex;`, put
/// where rustfmt sorts it among the `use` items next to it, and
/// `use std::sync::Arc;` in place. The new imports keep the item's
/// attributes and visibility. No fix for a renamed lock, an import inside a
/// function, or one rustfmt would have to move or wrap.
fn use_fix(ws: &Workspace, file: &RustFile, u: &syn::ItemUse) -> Option<Fix> {
    let (items, at) = siblings(&file.ast.items, u)?;
    let mut moved = Vec::new();
    let rest = remaining(&u.tree, &mut Vec::new(), &mut moved).ok()?;
    let v2024 = style_2024(ws, &file.path);
    moved.sort_by(|a, b| ident_cmp(a, b, v2024));
    moved.dedup();

    // The item has its lines to itself, each attribute one line, and no
    // comment the rewrite would drop.
    let (start, end) = (u.span().start().line, u.span().end().line);
    let first = file.line(start);
    let indent = &first[..first.len() - first.trim_start().len()];
    let text = file.text_of(u);
    if !text.starts_with(first.trim())
        || !file.line(end).trim_end().ends_with(';')
        || file.text_of(&u.tree).contains("//")
        || file.text_of(&u.tree).contains("/*")
    {
        return None;
    }
    let mut head = Vec::new();
    for attr in &u.attrs {
        let text = file.text_of(attr);
        if text.contains('\n') {
            return None;
        }
        head.push(format!("{indent}{text}"));
    }
    let vis = match &u.vis {
        syn::Visibility::Inherited => String::new(),
        v => format!("{} ", file.text_of(v)),
    };
    let import = |tree: &str| {
        let mut lines = head.clone();
        match nested_list(tree) {
            // rustfmt puts a list holding a list one item per line.
            Some((prefix, items)) => {
                lines.push(format!("{indent}{vis}use {prefix}{{"));
                lines.extend(items.iter().map(|i| format!("{indent}    {i},")));
                lines.push(format!("{indent}}};"));
            }
            None => lines.push(format!("{indent}{vis}use {tree};")),
        }
        lines
    };
    let moved_tree = match moved.as_slice() {
        [one] => one.clone(),
        many => format!("{{{}}}", many.join(", ")),
    };
    let new_import = import(&format!("parking_lot::{moved_tree}"));
    let kept = rest.as_deref().map(import).unwrap_or_default();
    if new_import
        .iter()
        .chain(&kept)
        .any(|l| l.chars().count() > 100)
    {
        return None; // rustfmt would wrap it
    }

    // Runs of `use` items with no line between them: rustfmt sorts each.
    let use_at = |i: usize| match items.get(i) {
        Some(syn::Item::Use(u)) => Some(u),
        _ => None,
    };
    let adjacent = |a: usize, b: usize| match (use_at(a), use_at(b)) {
        (Some(a), Some(b)) => a.span().end().line + 1 == b.span().start().line,
        _ => false,
    };
    let run_of = |i: usize| {
        let (mut lo, mut hi) = (i, i);
        while lo > 0 && adjacent(lo - 1, lo) {
            lo -= 1;
        }
        while adjacent(hi, hi + 1) {
            hi += 1;
        }
        (lo, hi)
    };
    let kept_key = match &rest {
        Some(r) => Some(segments(&syn::parse_str::<syn::UseTree>(r).ok()?)),
        None => None,
    };
    // A run's keys, with this item rewritten (or gone).
    let keys_of = |(lo, hi): (usize, usize)| -> Option<Vec<(usize, Vec<Seg>)>> {
        let mut keys = Vec::new();
        for i in lo..=hi {
            let key = if i == at {
                kept_key.clone()
            } else {
                Some(segments(&use_at(i)?.tree))
            };
            keys.extend(key.map(|k| (i, k)));
        }
        Some(keys)
    };
    let in_order = |keys: &[(usize, Vec<Seg>)]| {
        keys.windows(2)
            .all(|w| path_cmp(&w[0].1, &w[1].1, v2024) != Ordering::Greater)
    };
    // What stays must keep its place.
    let own = run_of(at);
    if !in_order(&keys_of(own)?) {
        return None;
    }
    // The new import joins the other crates' imports: a run of those alone,
    // else one with some, else this item's run.
    let external = |i: usize| {
        use_at(i).is_some_and(|other| {
            file.text_of(&other.vis) == file.text_of(&u.vis)
                && matches!(segments(&other.tree).first(),
                    Some(Seg::Ident(c)) if !["std", "core", "alloc"].contains(&c.as_str()))
        })
    };
    let mut runs = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if use_at(i).is_some() {
            runs.push(run_of(i));
            i = run_of(i).1;
        }
        i += 1;
    }
    let others = |(lo, hi): (usize, usize)| (lo..=hi).filter(move |&i| i != at);
    let target = runs
        .iter()
        .copied()
        .find(|&r| others(r).all(external) && others(r).any(external))
        .or_else(|| runs.iter().copied().find(|&r| others(r).any(external)))
        .unwrap_or(own);
    let keys = keys_of(target)?;
    if !in_order(&keys) {
        return None;
    }
    let new_key = vec![
        Seg::Ident("parking_lot".into()),
        match moved.as_slice() {
            [one] => Seg::Ident(one.clone()),
            many => Seg::List(many.iter().map(|m| vec![Seg::Ident(m.clone())]).collect()),
        },
    ];
    let mut insert_before = None;
    for (i, key) in &keys {
        match path_cmp(&new_key, key, v2024) {
            Ordering::Less => {
                insert_before = Some(*i);
                break;
            }
            Ordering::Equal => return None,
            Ordering::Greater => {}
        }
    }
    let line = match insert_before {
        Some(i) if i == at => start,
        Some(i) => use_at(i)?.span().start().line,
        None => use_at(target.1)?.span().end().line + 1,
    };

    let replaced = end - start + 1;
    let edits = if line == start {
        vec![(start, replaced, [new_import, kept].concat())]
    } else if line == end + 1 {
        vec![(start, replaced, [kept, new_import].concat())]
    } else {
        vec![(line, 0, new_import), (start, replaced, kept)]
    };
    let mut fix = Fix::default();
    for (line, remove, insert) in edits {
        fix.edits.extend(Fix::edit(line, remove, insert).edits);
    }
    fix.edits.sort_by_key(|e| e.line);
    Some(fix)
}

/// `std::{a::{b, c}, d}` -> `("std::", ["a::{b, c}", "d"])`: a list with a
/// list in it.
fn nested_list(tree: &str) -> Option<(&str, Vec<&str>)> {
    let open = tree.find('{')?;
    let inner = tree[open + 1..].strip_suffix('}')?;
    let mut items = Vec::new();
    let (mut depth, mut from) = (0, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(inner[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    items.push(inner[from..].trim());
    items
        .iter()
        .any(|i| i.contains('{'))
        .then_some((&tree[..open], items))
}

/// The items around a top-level or `mod { }` item, and its index.
fn siblings<'a>(items: &'a [syn::Item], target: &syn::ItemUse) -> Option<(&'a [syn::Item], usize)> {
    for (i, item) in items.iter().enumerate() {
        match item {
            syn::Item::Use(u) if std::ptr::eq(u, target) => return Some((items, i)),
            syn::Item::Mod(m) => {
                if let Some(found) = m
                    .content
                    .as_ref()
                    .and_then(|(_, inner)| siblings(inner, target))
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `use` tree without the names [`MOVED`] out of `std::sync`, as rustfmt
/// writes it (`None` when nothing is left). `Err` for a renamed lock.
fn remaining(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    moved: &mut Vec<String>,
) -> Result<Option<String>, ()> {
    let in_std_sync = |prefix: &[String]| prefix == ["std", "sync"];
    Ok(match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            let rest = remaining(&p.tree, prefix, moved)?;
            prefix.pop();
            rest.map(|r| format!("{}::{r}", p.ident))
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            if in_std_sync(prefix) && MOVED.contains(&name.as_str()) {
                moved.push(name);
                None
            } else {
                Some(name)
            }
        }
        syn::UseTree::Rename(r) => {
            if in_std_sync(prefix) && MOVED.contains(&r.ident.to_string().as_str()) {
                return Err(());
            }
            Some(format!("{} as {}", r.ident, r.rename))
        }
        syn::UseTree::Glob(_) => Some("*".to_string()),
        syn::UseTree::Group(g) => {
            let mut kept = Vec::new();
            for item in &g.items {
                kept.extend(remaining(item, prefix, moved)?);
            }
            match kept.as_slice() {
                [] => None,
                [one] if one != "self" => Some(one.clone()),
                _ => Some(format!("{{{}}}", kept.join(", "))),
            }
        }
    })
}

/// Whether rustfmt formats the file in the 2024 style (its crate's edition).
fn style_2024(ws: &Workspace, path: &str) -> bool {
    let Some(package) = ws.package_of(path) else {
        return false;
    };
    let edition = match package.package_field("edition") {
        Some(toml::Value::String(e)) => Some(e.clone()),
        // `edition.workspace = true`
        Some(toml::Value::Table(_)) => ws
            .file("Cargo.toml")
            .and_then(|f| f.text.parse::<toml::Table>().ok())
            .and_then(|t| {
                t.get("workspace")?
                    .get("package")?
                    .get("edition")?
                    .as_str()
                    .map(str::to_string)
            }),
        _ => None,
    };
    edition.is_some_and(|e| e.as_str() >= "2024")
}

/// A `use` path segment, as rustfmt orders them.
#[derive(Clone, Debug)]
enum Seg {
    SelfValue,
    Super,
    Crate,
    Ident(String),
    Glob,
    List(Vec<Vec<Seg>>),
}

fn segments(tree: &syn::UseTree) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut tree = tree;
    loop {
        let ident = |i: &syn::Ident| match i.to_string().as_str() {
            "self" => Seg::SelfValue,
            "super" => Seg::Super,
            "crate" => Seg::Crate,
            other => Seg::Ident(other.to_string()),
        };
        match tree {
            syn::UseTree::Path(p) => {
                out.push(ident(&p.ident));
                tree = &p.tree;
                continue;
            }
            syn::UseTree::Name(n) => out.push(ident(&n.ident)),
            syn::UseTree::Rename(r) => out.push(ident(&r.ident)),
            syn::UseTree::Glob(_) => out.push(Seg::Glob),
            syn::UseTree::Group(g) => out.push(Seg::List(g.items.iter().map(segments).collect())),
        }
        return out;
    }
}

fn path_cmp(a: &[Seg], b: &[Seg], v2024: bool) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let o = seg_cmp(x, y, v2024);
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

fn seg_cmp(a: &Seg, b: &Seg, v2024: bool) -> Ordering {
    let rank = |s: &Seg| match s {
        Seg::SelfValue => 0,
        Seg::Super => 1,
        Seg::Crate => 2,
        Seg::Ident(_) => 3,
        Seg::Glob => 4,
        Seg::List(_) => 5,
    };
    match (a, b) {
        (Seg::Ident(x), Seg::Ident(y)) => ident_cmp(x, y, v2024),
        (Seg::List(x), Seg::List(y)) => {
            for (p, q) in x.iter().zip(y) {
                let o = path_cmp(p, q, v2024);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

/// rustfmt's order of names: before 2024, `snake_case` < `CamelCase` <
/// `UPPER_CASE` then byte order; in 2024, "version sorting" (`_` first,
/// numbers by value, capitals before small letters).
fn ident_cmp(a: &str, b: &str, v2024: bool) -> Ordering {
    if !v2024 {
        let class = |s: &str| {
            if s.starts_with(|c: char| c.is_lowercase()) {
                0
            } else if s
                .chars()
                .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
            {
                2
            } else if s.starts_with(|c: char| c.is_uppercase()) {
                1
            } else {
                0
            }
        };
        return class(a).cmp(&class(b)).then_with(|| a.cmp(b));
    }
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n.trim_start_matches('0').to_string()
                };
                let (m, n) = (number(&mut x), number(&mut y));
                let o = m.len().cmp(&n.len()).then_with(|| m.cmp(&n));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let rank = |c: char| match c {
                    '_' => (0, c),
                    c if c.is_ascii_digit() => (1, c),
                    c => (2, c),
                };
                let o = rank(c).cmp(&rank(d));
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

impl<'ast> Visit<'ast> for StdLocks<'ast> {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut found = Vec::new();
        collect_use(&u.tree, &mut Vec::new(), &mut found);
        // One finding per `use` item.
        if !found.is_empty() {
            let start = syn::spanned::Spanned::span(u).start();
            self.hits.push(Hit {
                line: start.line,
                column: start.column + 1,
                names: found,
                site: Site::Use(u),
            });
        }
    }
    fn visit_item_macro(&mut self, m: &'ast syn::ItemMacro) {
        // A `macro_rules!` body expands in other crates, which may not depend
        // on parking_lot.
        if !m.mac.path.is_ident("macro_rules") {
            syn::visit::visit_item_macro(self, m);
        }
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // `lazy_static! { static ref X: std::sync::Mutex<..> = .. }`
        let mut found: Option<(usize, usize, String)> = None;
        errors::scan_tokens(m.tokens.clone(), &mut |tokens, i| {
            let ident = |k: usize| match tokens.get(k) {
                Some(proc_macro2::TokenTree::Ident(id)) => Some(id.to_string()),
                _ => None,
            };
            if found.is_none()
                && ident(i).as_deref() == Some("std")
                && ident(i + 3).as_deref() == Some("sync")
            {
                if let Some(name) = ident(i + 6).filter(|n| STD_LOCKS.contains(&n.as_str())) {
                    let start = tokens[i].span().start();
                    found = Some((start.line, start.column + 1, name));
                }
            }
        });
        if let Some((line, column, name)) = found {
            self.hits.push(Hit {
                line,
                column,
                names: vec![name],
                site: Site::Lines(line, m.span().end().line),
            });
        }
        syn::visit::visit_macro(self, m);
    }
    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        let segs: Vec<String> = t
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if let [.., a, b, c] = segs.as_slice() {
            if a == "std" && b == "sync" && STD_LOCKS.contains(&c.as_str()) {
                let start = syn::spanned::Spanned::span(t).start();
                self.hits.push(Hit {
                    line: start.line,
                    column: start.column + 1,
                    names: vec![c.clone()],
                    site: Site::Lines(start.line, start.line),
                });
            }
        }
        syn::visit::visit_type_path(self, t);
    }
}

/// The std lock names a `use` tree imports from `std::sync`.
fn collect_use(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            collect_use(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Group(g) => {
            for t in &g.items {
                collect_use(t, prefix, out);
            }
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            if prefix.as_slice() == ["std", "sync"] && STD_LOCKS.contains(&name.as_str()) {
                out.push(name);
            }
        }
        syn::UseTree::Rename(r) => {
            let name = r.ident.to_string();
            if prefix.as_slice() == ["std", "sync"] && STD_LOCKS.contains(&name.as_str()) {
                out.push(name);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/tabs/src/store.rs";

    fn run(src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(PATH, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(include_str!("../../rules/parking-lot-locks/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn accepts_do() {
        assert!(run(include_str!("../../rules/parking-lot-locks/do.rs")).is_empty());
    }

    #[test]
    fn flags_qualified_types() {
        let src = "struct S {\n    events: std::sync::RwLock<Vec<u8>>,\n}\n";
        let found = run(src);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("parking_lot::RwLock"));
    }

    fn edits(found: &[crate::Finding]) -> Vec<Vec<(usize, usize, Vec<&str>)>> {
        found
            .iter()
            .map(|f| {
                f.fix
                    .iter()
                    .flat_map(|fix| &fix.edits)
                    .map(|e| {
                        (
                            e.line,
                            e.remove,
                            e.insert.iter().map(String::as_str).collect(),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn splits_imports_where_rustfmt_keeps_them() {
        // The new import joins the other crates' `use` items.
        let src = "use std::collections::HashMap;\nuse std::sync::{Arc, Mutex, MutexGuard};\n\nuse rusqlite::Connection;\nuse serde::Deserialize;\n";
        assert_eq!(
            edits(&run(src)),
            vec![vec![
                (2, 1, vec!["use std::sync::Arc;"]),
                (4, 0, vec!["use parking_lot::{Mutex, MutexGuard};"]),
            ]]
        );
        // Nothing else to join: it takes the import's place, with its attributes.
        let src =
            "use crate::db::Db;\n#[cfg(feature = \"sync\")]\npub(crate) use std::sync::RwLock;\n";
        assert_eq!(
            edits(&run(src)),
            vec![vec![(
                2,
                2,
                vec![
                    "#[cfg(feature = \"sync\")]",
                    "pub(crate) use parking_lot::RwLock;"
                ]
            )]]
        );
        // A list holding a list is one item per line.
        let src = "use std::{\n    collections::{HashMap, HashSet},\n    sync::Mutex,\n};\n";
        assert_eq!(
            edits(&run(src)),
            vec![vec![(
                1,
                4,
                vec![
                    "use parking_lot::Mutex;",
                    "use std::collections::{HashMap, HashSet};"
                ]
            )]]
        );
    }

    #[test]
    fn fixes_named_types_and_leaves_the_rest() {
        let src = "struct S {\n    events: std::sync::RwLock<Vec<u8>>,\n}\n";
        let fixed = vec![(2, 1, vec!["    events: parking_lot::RwLock<Vec<u8>>,"])];
        assert_eq!(edits(&run(src)), vec![fixed]);
        // A renamed lock, and a comment that would be lost.
        assert_eq!(
            edits(&run("use std::sync::Mutex as StdMutex;\n")),
            vec![vec![]]
        );
        let dont = include_str!("../../rules/parking-lot-locks/dont.rs");
        assert_eq!(edits(&run(dont)), vec![vec![]]);
    }

    #[test]
    fn orders_like_rustfmt() {
        use super::ident_cmp;
        use std::cmp::Ordering::Less;
        for (a, b) in [("atomic", "Arc"), ("b2", "b_c"), ("Arc", "MAX")] {
            assert_eq!(ident_cmp(a, b, false), Less, "{a} < {b}");
        }
        for (a, b) in [
            ("Arc", "atomic"),
            ("b_c", "b2"),
            ("b2", "b10"),
            ("error_support", "errors"),
        ] {
            assert_eq!(ident_cmp(a, b, true), Less, "{a} < {b}");
        }
    }

    #[test]
    fn ignores_other_sync_items_and_condvars() {
        assert!(run("use std::sync::{Arc, OnceLock, atomic::AtomicBool};\n").is_empty());
        assert!(run("use std::sync::{Condvar, Mutex};\n").is_empty());
    }

    #[test]
    fn flags_lazy_statics_but_not_macro_rules() {
        let src = "lazy_static! {\n    static ref X: std::sync::Mutex<u8> = std::sync::Mutex::new(0);\n}\nmacro_rules! m { () => { struct S(std::sync::Mutex<u8>); } }\n";
        let lines: Vec<usize> = run(src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2]);
    }

    #[test]
    fn ignores_test_modules() {
        assert!(run("#[cfg(test)]\nmod tests {\n    use std::sync::Mutex;\n}\n").is_empty());
    }
}
