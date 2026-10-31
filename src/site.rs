//! The docs site: an index of the rules and one page per rule, as plain
//! static HTML. Built from the rule registry, each rule's docs folder
//! (`rules/<id>/`), the research evidence and a `varlope report`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::rule::{Area, Kind, Rule};

#[derive(Deserialize)]
struct Report {
    repo: String,
    sha: String,
    date: String,
    clippy: bool,
    findings: Vec<ReportFinding>,
    commits: Vec<ReportCommit>,
}

#[derive(Deserialize)]
struct ReportFinding {
    rule: String,
    path: String,
    line: usize,
    message: String,
    origin: String,
    commit: Option<String>,
    fix: Option<FixJson>,
    snippet: Option<SnippetJson>,
}

#[derive(Deserialize)]
struct FixJson {
    edits: Vec<EditJson>,
}

#[derive(Deserialize)]
struct EditJson {
    line: usize,
    remove: usize,
    insert: Vec<String>,
}

#[derive(Deserialize)]
struct SnippetJson {
    start: usize,
    lines: Vec<String>,
}

#[derive(Deserialize)]
struct ReportCommit {
    sha: String,
    subject: String,
    pr: Option<u64>,
}

#[derive(Deserialize)]
struct Research {
    rules: BTreeMap<String, Evidence>,
}

#[derive(Deserialize)]
struct Evidence {
    threads: usize,
    prs_min: usize,
    sources: Vec<Source>,
    bugfixes: Vec<Bugfix>,
}

#[derive(Deserialize)]
struct Source {
    pr: u64,
    url: String,
    date: String,
    quote: String,
}

#[derive(Deserialize)]
struct Bugfix {
    pr: u64,
    url: String,
    bug: String,
    verdict: Option<String>,
}

const SOURCE_REPO: &str = "mozilla/application-services";
const SHOWN_FINDINGS: usize = 30;
const SHOWN_SOURCES: usize = 5;
/// Findings rendered per rule page; the rest are in the report file.
const MAX_RENDERED: usize = 300;

/// Write `index.html` and `rules/<id>.html` under `out`.
pub fn build(report: &Path, research: &Path, docs: &Path, out: &Path) -> Result<()> {
    let report: Report = serde_json::from_str(
        &std::fs::read_to_string(report)
            .with_context(|| format!("reading {}", report.display()))?,
    )?;
    let research: Research = serde_json::from_str(
        &std::fs::read_to_string(research)
            .with_context(|| format!("reading {}", research.display()))?,
    )?;
    std::fs::create_dir_all(out.join("rules"))?;

    let rules = crate::rules();
    let mut by_rule: BTreeMap<&str, Vec<&ReportFinding>> = BTreeMap::new();
    for f in &report.findings {
        by_rule.entry(f.rule.as_str()).or_default().push(f);
    }
    // PR-diff findings read best newest commit first, as replayed.
    let order: BTreeMap<&str, usize> = report
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.sha.as_str(), i))
        .collect();
    for list in by_rule.values_mut() {
        list.sort_by_key(|f| {
            f.commit
                .as_deref()
                .and_then(|c| order.get(c))
                .copied()
                .unwrap_or(0)
        });
    }
    let commits: BTreeMap<&str, &ReportCommit> =
        report.commits.iter().map(|c| (c.sha.as_str(), c)).collect();

    std::fs::write(
        out.join("index.html"),
        index(rules, &report, &research, &by_rule),
    )?;
    for (i, rule) in rules.iter().enumerate() {
        let page = rule_page(
            rule,
            rules.get(i.wrapping_sub(1)).copied(),
            rules.get(i + 1).copied(),
            &report,
            research.rules.get(rule.id),
            by_rule.get(rule.id).map(Vec::as_slice).unwrap_or(&[]),
            &commits,
            docs,
        )?;
        std::fs::write(out.join("rules").join(format!("{}.html", rule.id)), page)?;
    }
    Ok(())
}

fn index(
    rules: &[&Rule],
    report: &Report,
    research: &Research,
    by_rule: &BTreeMap<&str, Vec<&ReportFinding>>,
) -> String {
    let threads: usize = research.rules.values().map(|e| e.threads).sum();
    let found_rules = rules.iter().filter(|r| by_rule.contains_key(r.id)).count();
    let total: usize = by_rule.values().map(Vec::len).sum();
    let mut body = String::new();
    let _ = write!(
        body,
        r#"<header class="top"><h1>varlope</h1>
<p class="lede">{count} lint rules for <a href="https://github.com/{SOURCE_REPO}">application-services</a>, each one something reviewers kept asking for in PR reviews ({threads} review threads, 2022–2026).</p>
<p class="meta">Run on <a href="https://github.com/{repo}/commit/{sha}"><code>{short}</code></a> ({date}): {total} findings from {found_rules} rules{clippy}. An experiment: feedback welcome.</p></header>
"#,
        count = rules.len(),
        repo = esc(&report.repo),
        sha = esc(&report.sha),
        short = esc(&report.sha[..report.sha.len().min(10)]),
        date = esc(&report.date),
        clippy = if report.clippy {
            ", clippy included"
        } else {
            ""
        },
    );
    body.push_str(r#"<p class="cols"><span></span><span>found</span><span>threads</span></p>"#);
    for area in Area::ALL {
        let in_area: Vec<&&Rule> = rules.iter().filter(|r| r.area == area).collect();
        if in_area.is_empty() {
            continue;
        }
        let _ = write!(
            body,
            "<section><h2>{}</h2><ul class=\"rules\">",
            area.title()
        );
        for rule in in_area {
            let found = by_rule.get(rule.id).map(Vec::len).unwrap_or(0);
            let evidence = research.rules.get(rule.id);
            let _ = write!(
                body,
                r#"<li><a href="rules/{id}.html">{title}</a><span class="n{zero}" title="findings in application-services">{found}</span><span class="src" title="review threads">{threads}</span></li>"#,
                id = rule.id,
                title = inline(rule.title),
                zero = if found == 0 { " zero" } else { "" },
                threads = evidence.map(|e| e.threads).unwrap_or(0),
            );
        }
        body.push_str("</ul></section>\n");
    }
    body.push_str(
        r#"<footer><p><b>found</b>: findings in application-services today (or in recent commits, for PR rules). <b>threads</b>: review threads that asked for it. Source: <a href="https://github.com/Almaju/varlope">github.com/Almaju/varlope</a>.</p></footer>"#,
    );
    page("varlope", &body)
}

#[allow(
    clippy::too_many_arguments,
    reason = "a rule page draws on all of these"
)]
fn rule_page(
    rule: &Rule,
    prev: Option<&Rule>,
    next: Option<&Rule>,
    report: &Report,
    evidence: Option<&Evidence>,
    findings: &[&ReportFinding],
    commits: &BTreeMap<&str, &ReportCommit>,
    docs: &Path,
) -> Result<String> {
    let dir = docs.join(rule.id);
    let why = std::fs::read_to_string(dir.join("why.md")).unwrap_or_default();
    let (dont_name, dont) = example(&dir, "dont");
    let (do_name, good) = example(&dir, "do");
    let mut b = String::new();
    let _ = write!(
        b,
        r#"<nav class="crumbs"><a href="../index.html">varlope</a> / {area}</nav>
<h1>{title}</h1>
<p class="meta"><code>{id}</code> · {kind}{clippy}</p>
<div class="why">{why}</div>
<div class="pair">
{dont_panel}
{do_panel}
</div>
"#,
        area = rule.area.title(),
        title = inline(rule.title),
        id = rule.id,
        kind = rule.kind.label(),
        clippy = if rule.clippy.is_empty() {
            String::new()
        } else {
            format!(
                " · {}",
                rule.clippy
                    .iter()
                    .map(|c| format!("<code>{c}</code>"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        },
        why = markdown(&why),
        dont_panel = example_panel(false, &dont_name, &dont, &good),
        do_panel = example_panel(true, &do_name, &good, &dont),
    );

    // What it finds today.
    let short = &report.sha[..report.sha.len().min(10)];
    b.push_str("<h2>In application-services</h2>");
    if findings.is_empty() {
        let note = match rule.kind {
            Kind::Clippy if !report.clippy => {
                "Not run: this rule runs with <code>varlope check --clippy</code>.".to_string()
            }
            Kind::Diff => format!("Nothing in the last {} commits.", report.commits.len()),
            _ => format!("Nothing found at <code>{short}</code>."),
        };
        let _ = write!(b, "<p class=\"meta\">{note}</p>");
    } else if findings.iter().all(|f| f.commit.is_some()) {
        // PR findings: one entry per commit, its findings below it.
        let mut groups: Vec<(&str, Vec<&ReportFinding>)> = Vec::new();
        for f in findings {
            let sha = f.commit.as_deref().unwrap_or("");
            match groups.last_mut() {
                Some((last, list)) if *last == sha => list.push(f),
                _ => groups.push((sha, vec![f])),
            }
        }
        let _ = write!(
            b,
            "<p class=\"meta\">{} findings in {} of the last {} merged commits</p><ul class=\"found\">",
            findings.len(),
            groups.len(),
            report.commits.len()
        );
        for (i, (sha, list)) in groups.iter().enumerate() {
            if i == SHOWN_FINDINGS {
                let _ = write!(
                    b,
                    "</ul><details><summary>{} more commits</summary><ul class=\"found\">",
                    groups.len() - SHOWN_FINDINGS
                );
            }
            b.push_str(&commit_group(sha, list, report, commits));
        }
        b.push_str("</ul>");
        if groups.len() > SHOWN_FINDINGS {
            b.push_str("</details>");
        }
    } else {
        let what = if rule.kind == Kind::Diff {
            format!(
                "{} in the last {} commits",
                findings.len(),
                report.commits.len()
            )
        } else {
            format!(
                "{} at <code>{short}</code> ({})",
                findings.len(),
                esc(&report.date)
            )
        };
        let fixes = findings.iter().filter(|f| f.fix.is_some()).count();
        let fixes = if fixes > 0 {
            format!(" · {fixes} with a suggested change")
        } else {
            String::new()
        };
        let _ = write!(
            b,
            "<p class=\"meta\">{what}{fixes}. Click one to see the code.</p><div class=\"found\">"
        );
        for f in findings.iter().take(MAX_RENDERED) {
            b.push_str(&finding_item(f, report, commits));
        }
        b.push_str("</div>");
        if findings.len() > MAX_RENDERED {
            let _ = write!(
                b,
                r#"<p class="meta">And {} more in <a href="https://github.com/Almaju/varlope/blob/main/report/findings.json">the report</a>.</p>"#,
                findings.len() - MAX_RENDERED
            );
        }
    }

    // Where it comes from.
    if let Some(e) = evidence {
        b.push_str("<h2>Where it comes from</h2>");
        if e.threads > 0 {
            let _ = write!(
                b,
                "<p class=\"meta\">Raised in {} review threads across at least {} PRs.</p><ul class=\"quotes\">",
                e.threads, e.prs_min
            );
            for (i, s) in e.sources.iter().enumerate() {
                if i == SHOWN_SOURCES {
                    let _ = write!(
                        b,
                        "</ul><details><summary>{} more comments</summary><ul class=\"quotes\">",
                        e.sources.len() - SHOWN_SOURCES
                    );
                }
                let _ = write!(
                    b,
                    r#"<li><blockquote>{quote}</blockquote><a href="{url}">#{pr}</a> · {date}</li>"#,
                    quote = inline(&s.quote),
                    url = esc(&s.url),
                    pr = s.pr,
                    date = esc(&s.date),
                );
            }
            b.push_str("</ul>");
            if e.sources.len() > SHOWN_SOURCES {
                b.push_str("</details>");
            }
        }
        if !e.bugfixes.is_empty() {
            b.push_str("<p class=\"meta\">Bugs it would have caught:</p><ul class=\"bugs\">");
            for fix in &e.bugfixes {
                let partly = if fix.verdict.as_deref() == Some("partially") {
                    " (partly)"
                } else {
                    ""
                };
                let _ = write!(
                    b,
                    r#"<li><a href="{url}">#{pr}</a> {bug}{partly}</li>"#,
                    url = esc(&fix.url),
                    pr = fix.pr,
                    bug = inline(&fix.bug),
                );
            }
            b.push_str("</ul>");
        }
    }

    b.push_str("<nav class=\"pager\">");
    if let Some(p) = prev {
        let _ = write!(b, r#"<a href="{}.html">← {}</a>"#, p.id, inline(p.title));
    } else {
        b.push_str("<span></span>");
    }
    if let Some(n) = next {
        let _ = write!(
            b,
            r#"<a class="next" href="{}.html">{} →</a>"#,
            n.id,
            inline(n.title)
        );
    }
    b.push_str("</nav>");
    Ok(page(&strip_ticks(rule.title), &b))
}

/// One replayed commit and what the rule found in it.
fn commit_group(
    sha: &str,
    list: &[&ReportFinding],
    report: &Report,
    commits: &BTreeMap<&str, &ReportCommit>,
) -> String {
    const SHOWN: usize = 4;
    let repo = esc(&report.repo);
    let c = commits.get(sha);
    let pr = c
        .and_then(|c| c.pr)
        .map(|n| format!(r#"<a href="https://github.com/{repo}/pull/{n}">#{n}</a> "#))
        .unwrap_or_default();
    let subject = c
        .map(|c| esc(strip_pr_suffix(&c.subject)))
        .unwrap_or_default();
    let mut out = format!(
        r#"<li>{pr}<a href="https://github.com/{repo}/commit/{sha}"><code>{short}</code></a> {subject}<ul class="sub">"#,
        sha = esc(sha),
        short = esc(&sha[..sha.len().min(8)]),
    );
    for f in list.iter().take(SHOWN) {
        let _ = write!(
            out,
            r#"<li class="msg">{}: {}</li>"#,
            esc(&f.path),
            inline(&f.message)
        );
    }
    if list.len() > SHOWN {
        let _ = write!(
            out,
            r#"<li class="msg">and {} more</li>"#,
            list.len() - SHOWN
        );
    }
    out.push_str("</ul></li>");
    out
}

fn finding_item(
    f: &ReportFinding,
    report: &Report,
    commits: &BTreeMap<&str, &ReportCommit>,
) -> String {
    let repo = esc(&report.repo);
    let (message, lint) = split_lint(&f.message);
    let clippy = match lint {
        Some(lint) => format!(r#" <span class="tag">{}</span>"#, esc(lint)),
        None if f.origin == "clippy" => r#" <span class="tag">clippy</span>"#.to_string(),
        None => String::new(),
    };
    match &f.commit {
        Some(sha) => {
            let c = commits.get(sha.as_str());
            let pr = c
                .and_then(|c| c.pr)
                .map(|n| format!(r#"<a href="https://github.com/{repo}/pull/{n}">#{n}</a> "#))
                .unwrap_or_default();
            let subject = c
                .map(|c| esc(strip_pr_suffix(&c.subject)))
                .unwrap_or_default();
            format!(
                r#"<li>{pr}<a href="https://github.com/{repo}/commit/{sha}"><code>{short}</code></a> {subject}<br><span class="msg">{path}: {msg}</span></li>"#,
                sha = esc(sha),
                short = esc(&sha[..sha.len().min(8)]),
                path = esc(&f.path),
                msg = inline(&f.message),
            )
        }
        None => {
            let anchor = if f.line > 0 {
                format!("#L{}", f.line)
            } else {
                String::new()
            };
            let at = if f.line > 0 {
                format!("{}:{}", f.path, f.line)
            } else {
                f.path.clone()
            };
            let view = code_view(f);
            let note = if f.fix.is_some() {
                "Suggested change"
            } else if view.is_empty() {
                "No code to show for this finding"
            } else {
                "No suggested change: see the Do example above"
            };
            format!(
                r#"<details class="finding"><summary><code>{at}</code><span class="msg">{msg} <span class="tags">{clippy}{fixed}</span></span></summary><div class="fbody"><p class="fnote">{note} · <a href="https://github.com/{repo}/blob/{sha}/{path}{anchor}">view on GitHub</a></p>{view}</div></details>"#,
                sha = esc(&report.sha),
                path = esc(&f.path),
                at = esc(&at),
                msg = inline(message),
                fixed = if f.fix.is_some() {
                    " <span class=\"tag fix\">fix</span>"
                } else {
                    ""
                },
            )
        }
    }
}

/// A clippy message without its `(clippy::lint)` suffix, and the lint.
fn split_lint(message: &str) -> (&str, Option<&str>) {
    match message.rsplit_once(" (clippy::") {
        Some((msg, rest)) if rest.ends_with(')') => {
            (msg, Some(&message[msg.len() + 2..message.len() - 1]))
        }
        _ => (message, None),
    }
}

/// A Don't or Do example: line numbers, syntax highlighting, the lines the
/// rule flags (Don't) or the lines that changed (Do) marked.
fn example_panel(good: bool, ext: &str, text: &str, other: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let marks: std::collections::BTreeSet<usize> = if ext == "diff" {
        if good {
            Default::default()
        } else {
            flagged_lines(&lines)
        }
    } else if good {
        let (_, added) = crate::diff::line_diff(&strip_flags(other), &strip_flags(text));
        added.into_iter().map(|(n, _)| n).collect()
    } else {
        let flagged = flagged_lines(&lines);
        if flagged.is_empty() {
            let (removed, _) = crate::diff::line_diff(&strip_flags(text), &strip_flags(other));
            removed.into_iter().map(|(n, _)| n).collect()
        } else {
            flagged
        }
    };
    let (class, caption) = if good {
        ("do", "✓ Do")
    } else {
        ("dont", "✕ Don't")
    };
    let mut body = String::new();
    let mut in_block = crate::highlight::Carry::default();
    let mut diff_ext = String::new();
    for (i, l) in lines.iter().enumerate() {
        let n = i + 1;
        let (prefix_class, html) = if ext == "diff" {
            diff_line(l, &mut diff_ext, &mut in_block)
        } else {
            ("", crate::highlight::line(ext, l, &mut in_block))
        };
        let mark = if marks.contains(&n) {
            if good {
                " fixed"
            } else {
                " flagged"
            }
        } else {
            ""
        };
        let _ = write!(
            body,
            r#"<span class="ln{mark}{prefix_class}" data-n="{n}"><span>{html}</span></span>"#
        );
    }
    format!(
        r#"<figure class="{class}"><figcaption>{caption} <span>{ext}</span></figcaption><pre class="code">{body}</pre></figure>"#,
        ext = esc(ext),
    )
}

/// One line of a `.diff` example: its kind, and the line highlighted in the
/// language of the file it belongs to.
fn diff_line(
    l: &str,
    file_ext: &mut String,
    in_block: &mut crate::highlight::Carry,
) -> (&'static str, String) {
    if l.starts_with("+++ ")
        || l.starts_with("--- ")
        || l.starts_with("@@")
        || l.starts_with("diff ")
    {
        if l.starts_with("+++ ") || l.starts_with("--- ") {
            *file_ext = crate::highlight::diff_file_ext(l);
            *in_block = crate::highlight::Carry::default();
        }
        return (" meta", esc(l));
    }
    let (class, sign, rest) = match l.chars().next() {
        Some('+') => (" add", "+", &l[1..]),
        Some('-') => (" del", "-", &l[1..]),
        Some(' ') => ("", " ", &l[1..]),
        _ => ("", "", l),
    };
    (
        class,
        format!(
            "{}{}",
            esc(sign),
            crate::highlight::line(file_ext, rest, in_block)
        ),
    )
}

/// Lines whose comment marks them as what the rule flags.
fn flagged_lines(lines: &[&str]) -> std::collections::BTreeSet<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.to_ascii_lowercase().contains("flagged"))
        .map(|(i, _)| i + 1)
        .collect()
}

/// The example without its `// flagged: …` notes, for comparing Don't and Do.
fn strip_flags(text: &str) -> String {
    text.lines()
        .map(|l| {
            let lower = l.to_ascii_lowercase();
            match lower.find("flagged") {
                Some(at) => {
                    let cut = ["//", "#", "--"]
                        .iter()
                        .filter_map(|o| l[..at].rfind(o))
                        .max()
                        .unwrap_or(at);
                    l[..cut].trim_end().to_string()
                }
                None => l.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The code around a finding: with its suggested change as a diff, or with
/// the flagged line marked.
fn code_view(f: &ReportFinding) -> String {
    let Some(snippet) = &f.snippet else {
        return String::new();
    };
    let ext = crate::highlight::ext_of(&f.path);
    let mut out = String::from(r#"<pre class="code diffview">"#);
    let mut in_block = crate::highlight::Carry::default();
    let edits: Vec<&EditJson> = f
        .fix
        .as_ref()
        .map(|x| x.edits.iter().collect())
        .unwrap_or_default();
    let mut n = snippet.start;
    let mut i = 0;
    while i < snippet.lines.len() {
        if let Some(edit) = edits.iter().find(|e| e.line == n) {
            let unchanged = edit.remove > 0
                && snippet.lines.get(i..i + edit.remove) == Some(edit.insert.as_slice());
            if unchanged {
                // Same text: the edit only adds the missing final newline.
                for k in 0..edit.remove {
                    let html = crate::highlight::line(ext, &snippet.lines[i + k], &mut in_block);
                    let _ = write!(
                        out,
                        r#"<span class="ln" data-n="{}"><span>{html}</span></span>"#,
                        n + k
                    );
                }
                out.push_str(r#"<span class="ln add" data-n="+"><span class="cm">⏎ newline at end of file</span></span>"#);
                i += edit.remove;
                n += edit.remove;
                continue;
            }
            for k in 0..edit.remove {
                if let Some(l) = snippet.lines.get(i + k) {
                    let html = crate::highlight::line(ext, l, &mut in_block);
                    let _ = write!(
                        out,
                        r#"<span class="ln del" data-n="{}"><span>{html}</span></span>"#,
                        n + k
                    );
                }
            }
            let mut add_block = crate::highlight::Carry::default();
            for l in &edit.insert {
                let html = crate::highlight::line(ext, l, &mut add_block);
                let _ = write!(
                    out,
                    r#"<span class="ln add" data-n="+"><span>{html}</span></span>"#
                );
            }
            i += edit.remove;
            n += edit.remove;
            if edit.remove == 0 {
                // Insertion: the original line follows unchanged.
                let html = crate::highlight::line(ext, &snippet.lines[i], &mut in_block);
                let _ = write!(
                    out,
                    r#"<span class="ln" data-n="{n}"><span>{html}</span></span>"#
                );
                i += 1;
                n += 1;
            }
            continue;
        }
        let l = &snippet.lines[i];
        let html = crate::highlight::line(ext, l, &mut in_block);
        let mark = if edits.is_empty() && n == f.line {
            " flagged"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="ln{mark}" data-n="{n}"><span>{html}</span></span>"#
        );
        i += 1;
        n += 1;
    }
    out.push_str("</pre>");
    out
}

/// `(file name, contents)` of `rules/<id>/<stem>.*`.
fn example(dir: &Path, stem: &str) -> (String, String) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (String::new(), String::new());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{stem}.")) {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let ext = name.rsplit('.').next().unwrap_or("").to_string();
            return (ext, text.trim_end().to_string());
        }
    }
    (String::new(), String::new())
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>{STYLE}</style>
</head>
<body><main>
{body}
</main></body>
</html>
"#,
        title = esc(title),
    )
}

pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `Fix it (#7612)` → `Fix it`: the PR is linked separately.
fn strip_pr_suffix(subject: &str) -> &str {
    match subject.rfind(" (#") {
        Some(i) if subject.ends_with(')') => &subject[..i],
        _ => subject,
    }
}

fn strip_ticks(text: &str) -> String {
    text.replace('`', "")
}

/// Escape, then render `code` spans, **bold**, [links](url) and `#1234` PR refs.
pub fn inline(text: &str) -> String {
    let mut out = String::new();
    for (i, part) in text.split('`').enumerate() {
        if i % 2 == 1 {
            let _ = write!(out, "<code>{}</code>", esc(part));
            continue;
        }
        let mut s = esc(part);
        s = replace_pattern(&s, r"\*\*([^*]+)\*\*", |c| {
            format!("<strong>{}</strong>", &c[1])
        });
        s = replace_pattern(&s, r"\[([^\]]+)\]\((https?://[^)\s]+)\)", |c| {
            format!(r#"<a href="{}">{}</a>"#, &c[2], &c[1])
        });
        s = replace_pattern(&s, r"(^|[\s(])#(\d{3,5})\b", |c| {
            format!(
                r#"{}<a href="https://github.com/{SOURCE_REPO}/pull/{n}">#{n}</a>"#,
                &c[1],
                n = &c[2]
            )
        });
        out.push_str(&s);
    }
    out
}

fn replace_pattern(text: &str, pattern: &str, f: impl Fn(&regex::Captures) -> String) -> String {
    let re = regex::Regex::new(pattern).expect("valid pattern");
    re.replace_all(text, |c: &regex::Captures| f(c))
        .into_owned()
}

/// Paragraphs and bullet lists of inline markdown.
pub fn markdown(text: &str) -> String {
    let mut out = String::new();
    for block in text.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
        let lines: Vec<&str> = block.lines().collect();
        if lines.iter().all(|l| l.trim_start().starts_with("- ")) {
            out.push_str("<ul>");
            for l in lines {
                let _ = write!(
                    out,
                    "<li>{}</li>",
                    inline(l.trim_start().trim_start_matches("- "))
                );
            }
            out.push_str("</ul>");
        } else {
            let _ = write!(out, "<p>{}</p>", inline(&lines.join(" ")));
        }
    }
    out
}

const STYLE: &str = r#":root {
  --bg: #fbfbfa; --fg: #1d2125; --muted: #646b73; --line: #e2e4e7; --code: #f2f3f5;
  --link: #1a5fb4; --bad: #b3261e; --bad-bg: #fbeceb; --good: #1e7a46; --good-bg: #e8f4ec;
  --kw: #8250df; --st: #0a3069; --nu: #0550ae; --ty: #953800; --ma: #1b7c83; --at: #57606a;
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #141619; --fg: #e3e5e8; --muted: #9aa1a9; --line: #2b2f34; --code: #1d2024;
    --link: #8ab4f8; --bad: #f28b82; --bad-bg: #3a1f1d; --good: #81c995; --good-bg: #183223;
    --kw: #d2a8ff; --st: #a5d6ff; --nu: #79c0ff; --ty: #ffa657; --ma: #56d4dd; --at: #8b949e;
    color-scheme: dark;
  }
}
:root[data-theme="dark"] {
  --bg: #141619; --fg: #e3e5e8; --muted: #9aa1a9; --line: #2b2f34; --code: #1d2024;
  --link: #8ab4f8; --bad: #f28b82; --bad-bg: #3a1f1d; --good: #81c995; --good-bg: #183223;
  --kw: #d2a8ff; --st: #a5d6ff; --nu: #79c0ff; --ty: #ffa657; --ma: #56d4dd; --at: #8b949e;
  color-scheme: dark;
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.55 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
main { max-width: 880px; margin: 0 auto; padding-block: 40px 80px;
  padding-left: max(16px, env(safe-area-inset-left, 0px));
  padding-right: max(16px, env(safe-area-inset-right, 0px)); }
a { color: var(--link); text-decoration: none; }
a:hover { text-decoration: underline; }
h1 { font-size: 1.7rem; line-height: 1.25; margin: 0 0 6px; letter-spacing: -0.01em; }
h2 { font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted);
  margin: 36px 0 10px; }
code, pre { font: 0.86em/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
:not(pre) > code { background: var(--code); padding: 1px 4px; border-radius: 4px; }
.lede { font-size: 1.1rem; margin: 6px 0; max-width: 62ch; }
.meta { color: var(--muted); font-size: 0.9rem; margin: 4px 0; }
.crumbs { font-size: 0.9rem; color: var(--muted); margin-bottom: 14px; }
.why { max-width: 68ch; margin: 18px 0; }
.why p { margin: 0 0 10px; }
.pair { display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; margin: 22px 0; }
figure { margin: 0; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; min-width: 0; }
figcaption { font-size: 0.85rem; font-weight: 600; padding: 6px 12px; display: flex; justify-content: space-between; }
figcaption span { font-weight: 400; color: var(--muted); font-family: ui-monospace, monospace; }
.dont figcaption { color: var(--bad); background: var(--bad-bg); }
.do figcaption { color: var(--good); background: var(--good-bg); }
pre { margin: 0; padding: 12px; overflow-x: auto; background: var(--code); }
pre.code { padding: 10px 0; font-size: 0.8rem; line-height: 1.6; font-variant-ligatures: none; }
pre.code .ln { display: grid; grid-template-columns: 2.8em minmax(0, 1fr); padding-right: 14px; min-width: max-content; }
pre.code .ln::before { content: attr(data-n); text-align: right; padding-right: 0.9em; color: var(--muted);
  opacity: 0.6; user-select: none; }
pre.code .ln > span { white-space: pre; }
.ln.flagged, .ln.del { background: var(--bad-bg); }
.ln.flagged > span { text-decoration: underline wavy var(--bad); text-decoration-skip-ink: none;
  text-underline-offset: 3px; }
.ln.fixed, .ln.add { background: var(--good-bg); }
.ln.add::before, .ln.fixed::before { color: var(--good); opacity: 1; }
.ln.del::before, .ln.flagged::before { color: var(--bad); opacity: 1; }
.ln.flagged.add { background: var(--bad-bg); }
.ln.meta { color: var(--muted); }
.kw { color: var(--kw); } .st { color: var(--st); } .nu { color: var(--nu); } .ty { color: var(--ty); }
.ma { color: var(--ma); } .at { color: var(--at); } .cm { color: var(--muted); font-style: italic; }
ul { padding-left: 0; list-style: none; margin: 0; }
.cols { display: grid; grid-template-columns: 1fr 3.5rem 3rem; gap: 10px; margin: 28px 0 -18px;
  font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); text-align: right; }
.rules li { display: grid; grid-template-columns: 1fr 3.5rem 3rem; gap: 10px; align-items: baseline;
  padding: 7px 0; border-bottom: 1px solid var(--line); }
.rules .n, .rules .src { text-align: right; font-variant-numeric: tabular-nums; font-size: 0.9rem; }
.rules .n { font-weight: 600; }
.rules .zero { color: var(--muted); font-weight: 400; }
.rules .src { color: var(--muted); }
ul.found li { padding: 4px 0; font-size: 0.92rem; }
div.found { border-top: 1px solid var(--line); }
details.finding { border-bottom: 1px solid var(--line); }
details.finding > summary { cursor: pointer; padding: 7px 0; font-size: 0.9rem; color: var(--fg); margin: 0;
  list-style: none; display: grid; grid-template-columns: 0.8em fit-content(55%) minmax(0, 1fr); gap: 4px 10px;
  align-items: baseline; }
details.finding > summary code { overflow-wrap: anywhere; }
details.finding .tags { white-space: nowrap; }
details.finding .tags .tag { margin-left: 4px; }
@media (max-width: 640px) {
  details.finding > summary { grid-template-columns: 0.8em minmax(0, 1fr); }
  details.finding .msg { grid-column: 2; }
}
details.finding > summary::-webkit-details-marker { display: none; }
details.finding > summary::before { content: "▸"; color: var(--muted); width: 0.8em; }
details.finding[open] > summary::before { content: "▾"; }
.fbody { margin: 0 0 12px; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
.fnote { margin: 0; padding: 6px 12px; font-size: 0.8rem; color: var(--muted); background: var(--code);
  border-bottom: 1px solid var(--line); }
.tag.fix { color: var(--good); border-color: var(--good); }
.found .msg { color: var(--muted); }
.found .sub { margin: 2px 0 6px 14px; }
.found .sub li { padding: 1px 0; font-size: 0.88rem; }
.tag { font-size: 0.75rem; color: var(--muted); border: 1px solid var(--line); border-radius: 4px; padding: 0 4px; }
.quotes li { margin: 0 0 14px; font-size: 0.9rem; color: var(--muted); }
.quotes blockquote { margin: 0 0 2px; color: var(--fg); border-left: 3px solid var(--line); padding-left: 12px;
  font-size: 0.95rem; }
.bugs li { padding: 3px 0; font-size: 0.92rem; }
details summary { cursor: pointer; color: var(--link); font-size: 0.9rem; margin: 6px 0 10px; }
.pager { display: flex; justify-content: space-between; gap: 16px; margin-top: 48px; padding-top: 16px;
  border-top: 1px solid var(--line); font-size: 0.9rem; }
.pager .next { text-align: right; }
footer { margin-top: 40px; color: var(--muted); font-size: 0.85rem; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_markdown() {
        assert_eq!(
            inline("Use `a<b>` and **this**, see #6189."),
            "Use <code>a&lt;b&gt;</code> and <strong>this</strong>, see <a href=\"https://github.com/mozilla/application-services/pull/6189\">#6189</a>."
        );
        assert_eq!(
            markdown("One.\n\n- a\n- b"),
            "<p>One.</p><ul><li>a</li><li>b</li></ul>"
        );
    }
}
