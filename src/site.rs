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
<figure class="dont"><figcaption>Don't <span>{dont_name}</span></figcaption><pre><code>{dont}</code></pre></figure>
<figure class="do"><figcaption>Do <span>{do_name}</span></figcaption><pre><code>{good}</code></pre></figure>
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
        dont = esc(&dont),
        good = esc(&good),
        dont_name = esc(&dont_name),
        do_name = esc(&do_name),
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
        let _ = write!(b, "<p class=\"meta\">{what}</p><ul class=\"found\">");
        for (i, f) in findings.iter().enumerate() {
            if i == SHOWN_FINDINGS {
                let _ = write!(
                    b,
                    "</ul><details><summary>{} more</summary><ul class=\"found\">",
                    findings.len() - SHOWN_FINDINGS
                );
            }
            b.push_str(&finding_item(f, report, commits));
        }
        b.push_str("</ul>");
        if findings.len() > SHOWN_FINDINGS {
            b.push_str("</details>");
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
    let clippy = if f.origin == "clippy" {
        " <span class=\"tag\">clippy</span>"
    } else {
        ""
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
            format!(
                r#"<li><a href="https://github.com/{repo}/blob/{sha}/{path}{anchor}"><code>{at}</code></a>{clippy} <span class="msg">{msg}</span></li>"#,
                sha = esc(&report.sha),
                path = esc(&f.path),
                at = esc(&at),
                msg = inline(&f.message),
            )
        }
    }
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
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #141619; --fg: #e3e5e8; --muted: #9aa1a9; --line: #2b2f34; --code: #1d2024;
    --link: #8ab4f8; --bad: #f28b82; --bad-bg: #2c1b1a; --good: #81c995; --good-bg: #17271d;
    color-scheme: dark;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.55 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
main { max-width: 880px; margin: 0 auto; padding: 40px 20px 80px; }
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
.pair { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin: 22px 0; }
@media (max-width: 760px) { .pair { grid-template-columns: 1fr; } }
figure { margin: 0; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; min-width: 0; }
figcaption { font-size: 0.85rem; font-weight: 600; padding: 6px 12px; display: flex; justify-content: space-between; }
figcaption span { font-weight: 400; color: var(--muted); font-family: ui-monospace, monospace; }
.dont figcaption { color: var(--bad); background: var(--bad-bg); }
.do figcaption { color: var(--good); background: var(--good-bg); }
pre { margin: 0; padding: 12px; overflow-x: auto; background: var(--code); }
ul { padding-left: 0; list-style: none; margin: 0; }
.cols { display: grid; grid-template-columns: 1fr 3.5rem 3rem; gap: 10px; margin: 28px 0 -18px;
  font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); text-align: right; }
.rules li { display: grid; grid-template-columns: 1fr 3.5rem 3rem; gap: 10px; align-items: baseline;
  padding: 7px 0; border-bottom: 1px solid var(--line); }
.rules .n, .rules .src { text-align: right; font-variant-numeric: tabular-nums; font-size: 0.9rem; }
.rules .n { font-weight: 600; }
.rules .zero { color: var(--muted); font-weight: 400; }
.rules .src { color: var(--muted); }
.found li { padding: 4px 0; font-size: 0.92rem; }
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
