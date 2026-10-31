//! Validate metrics.yaml: bugs, data reviews, notification emails, expiry
//!
//! Docs and examples: `rules/glean-metrics-yaml/`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "glean-metrics-yaml",
    area: Area::NimbusGlean,
    title: "Validate metrics.yaml: bugs, data reviews, notification emails, expiry",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Two releases: an expiry closer than this needs renewing now.
const WARN_DAYS: i64 = 8 * 7;
const WARN_VERSIONS: u64 = 2;

fn check(cx: &Context, out: &mut Findings) {
    let today = days_since_epoch_now();
    let major = cx
        .ws
        .file("version.txt")
        .and_then(|f| f.text.trim().split('.').next()?.parse::<u64>().ok());
    for file in cx.ws.files_with_ext(&["yaml", "yml"]) {
        if file.file_name() != "metrics.yaml" || crate::workspace::is_test_path(&file.path) {
            continue;
        }
        let metrics = parse_metrics(file);
        // The team alias the file's other metrics notify, when there is one.
        let mut aliases: Vec<&str> = metrics
            .iter()
            .filter_map(|m| m.get("notification_emails"))
            .flat_map(|p| p.items.iter().map(|(_, e)| e.as_str()))
            .filter(|e| is_team_alias(e))
            .collect();
        aliases.sort_unstable();
        aliases.dedup();
        let alias = match aliases.as_slice() {
            [one] => Some(*one),
            _ => None,
        };
        for metric in &metrics {
            check_metric(file, metric, today, major, alias, out);
        }
    }
}

/// What a missing link is replaced with: still flagged until filled in.
const PLACEHOLDER: &str = "https://bugzilla.mozilla.org/show_bug.cgi?id=TODO";

/// The keys a metric lists its links and owners under, in their usual order.
const ORDER: &[&str] = &["bugs", "data_reviews", "notification_emails", "expires"];

fn add(out: &mut Findings, path: &str, line: usize, message: String, fix: Option<Fix>) {
    match fix {
        Some(fix) => out.add_fix(path, line, message, fix),
        None => out.add(path, line, message),
    }
}

/// The indentation of list items in this file (`      - `).
fn item_indent(file: &TextFile) -> String {
    file.lines()
        .find_map(|(_, l)| {
            let rest = l.trim_start();
            let indent = l.len() - rest.len();
            (rest.starts_with("- ") && indent > 4).then(|| l[..indent].to_string())
        })
        .unwrap_or_else(|| " ".repeat(6))
}

/// `key:` with a placeholder link, before the next of [`ORDER`]'s keys.
fn missing_links(file: &TextFile, m: &Metric, key: &str) -> Option<Fix> {
    let after = ORDER.iter().position(|k| *k == key)? + 1;
    let next = ORDER[after..].iter().find_map(|k| m.get(k))?;
    let text = file.line(next.line);
    let indent = &text[..text.len() - text.trim_start().len()];
    Some(Fix::insert_before(
        next.line,
        vec![
            format!("{indent}{key}:"),
            format!("{}- {PLACEHOLDER}", item_indent(file)),
        ],
    ))
}

/// `key: []` -> `key:` with a placeholder link.
fn empty_links(file: &TextFile, p: &Prop) -> Option<Fix> {
    let line = file.line(p.line);
    let (key, value) = line.split_once(':')?;
    let value = strip_comment(value).trim();
    if !matches!(value, "" | "[]" | "\"\"" | "''") {
        return None;
    }
    Some(Fix::edit(
        p.line,
        1,
        vec![
            format!("{key}:"),
            format!("{}- {PLACEHOLDER}", item_indent(file)),
        ],
    ))
}

fn check_metric(
    file: &TextFile,
    m: &Metric,
    today: i64,
    major: Option<u64>,
    alias: Option<&str>,
    out: &mut Findings,
) {
    let id = format!("{}.{}", m.category, m.name);
    for key in ["bugs", "data_reviews"] {
        match m.get(key) {
            None => add(
                out,
                &file.path,
                m.line,
                format!("`{id}` has no `{key}`; link the Bugzilla bug or GitHub issue"),
                missing_links(file, m, key),
            ),
            Some(p) if p.items.is_empty() && p.value.is_empty() => add(
                out,
                &file.path,
                p.line,
                format!("`{id}` has an empty `{key}`; link the Bugzilla bug or GitHub issue"),
                empty_links(file, p),
            ),
            Some(p) => {
                for (line, item) in &p.items {
                    if !is_link(item) {
                        out.add(
                            &file.path,
                            *line,
                            format!(
                                "`{item}` in `{id}.{key}` is not a Bugzilla, GitHub or Jira link"
                            ),
                        );
                    }
                }
            }
        }
    }
    if let Some(p) = m.get("notification_emails") {
        if !p.items.is_empty() && !p.items.iter().any(|(_, e)| is_team_alias(e)) {
            // The alias the rest of the file uses, after the last address.
            let fix = alias.and_then(|alias| {
                let (last, _) = p.items.last()?;
                let text = file.line(*last);
                let dash = text.find("- ")?;
                (*last != p.line).then(|| {
                    Fix::insert_before(last + 1, vec![format!("{}- {alias}", &text[..dash])])
                })
            });
            add(
                out,
                &file.path,
                p.line,
                format!("`{id}` only notifies individuals; add a team alias so expiry and alert emails outlive team changes"),
                fix,
            );
        }
    }
    if let Some(p) = m.get("disabled") {
        if p.value == "false" {
            out.add_fix(
                &file.path,
                p.line,
                format!("`disabled: false` on `{id}` is the default and reads as if the metric were off; delete the line"),
                Fix::delete(p.line, 1),
            );
        }
    }
    if let Some(p) = m.get("expires") {
        let soon = match parse_date(&p.value) {
            Some(day) => day - today <= WARN_DAYS,
            None => match (p.value.parse::<u64>(), major) {
                (Ok(version), Some(major)) => version <= major + WARN_VERSIONS,
                _ => false,
            },
        };
        if soon {
            out.add(
                &file.path,
                p.line,
                format!(
                    "`{id}` expires at `{}`, within two releases; renew it (with data review) or remove the metric",
                    p.value
                ),
            );
        }
    }
}

fn is_link(item: &str) -> bool {
    let item = item.trim();
    if item.chars().all(|c| c.is_ascii_digit()) && !item.is_empty() {
        return true; // a bare bug number
    }
    [
        "https://bugzilla.mozilla.org/",
        "https://bugzil.la/",
        "https://github.com/",
        "https://mozilla-hub.atlassian.net/",
        "https://jira.mozilla.com/",
        "https://phabricator.services.mozilla.com/",
    ]
    .iter()
    .any(|p| item.starts_with(p))
        && !item.contains("TODO")
}

/// A list address rather than a person: `sync-team@`, `project-nimbus@`.
fn is_team_alias(email: &str) -> bool {
    let local = email.split('@').next().unwrap_or("");
    local.contains('-') || local.contains("team") || local.ends_with("dev")
}

// ---------------------------------------------------------------------------
// A small reader for the metrics.yaml layout: `category:` at column 0,
// `metric:` at column 2, `key: value` (or a `- item` list) at column 4.

#[derive(Debug)]
struct Metric {
    category: String,
    name: String,
    line: usize,
    props: Vec<Prop>,
}

#[derive(Debug)]
struct Prop {
    key: String,
    line: usize,
    /// Scalar value without quotes or trailing comment; `""` for a list.
    value: String,
    items: Vec<(usize, String)>,
}

impl Metric {
    fn get(&self, key: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.key == key)
    }
}

fn parse_metrics(file: &TextFile) -> Vec<Metric> {
    let mut out: Vec<Metric> = Vec::new();
    let mut category: Option<String> = None;
    for (n, raw) in file.lines() {
        let line = strip_comment(raw);
        if line.trim().is_empty() || line.trim() == "---" {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if indent == 0 {
            category = key_only(text)
                .filter(|k| !k.starts_with('$') && *k != "no_lint")
                .map(str::to_string);
            continue;
        }
        let Some(cat) = &category else { continue };
        if indent == 2 {
            if let Some(name) = key_only(text) {
                out.push(Metric {
                    category: cat.clone(),
                    name: name.to_string(),
                    line: n,
                    props: Vec::new(),
                });
            }
            continue;
        }
        let Some(metric) = out.last_mut() else {
            continue;
        };
        if indent == 4 {
            if let Some((key, value)) = text.split_once(':') {
                let value = value.trim();
                let mut prop = Prop {
                    key: key.trim().to_string(),
                    line: n,
                    value: String::new(),
                    items: Vec::new(),
                };
                if let Some(list) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    prop.items = list
                        .split(',')
                        .map(unquote)
                        .filter(|s| !s.is_empty())
                        .map(|s| (n, s))
                        .collect();
                } else {
                    prop.value = unquote(value);
                }
                metric.props.push(prop);
            }
        } else if let (Some(item), Some(prop)) = (text.strip_prefix("- "), metric.props.last_mut())
        {
            if indent >= 4 {
                prop.items.push((n, unquote(item)));
            }
        }
    }
    out
}

/// `name:` with nothing after it.
fn key_only(text: &str) -> Option<&str> {
    let key = text.strip_suffix(':')?;
    (!key.contains(' ') && !key.is_empty()).then_some(key)
}

fn strip_comment(line: &str) -> &str {
    // A `#` starts a comment at the line start or after a space (not in URLs' `#issuecomment`).
    let bytes = line.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && (i == 0 || bytes[i - 1] == b' ') {
            return &line[..i];
        }
    }
    line
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

// ---------------------------------------------------------------------------
// Dates

fn days_since_epoch_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` as days since 1970-01-01.
fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/suggest/metrics.yaml";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/glean-metrics-yaml/dont.yaml"),
            )],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![6, 8, 9, 11, 12], "{found:#?}");
    }

    /// `(line, remove, insert)` of each edit of a finding's fix.
    fn edits(f: &crate::rule::Finding) -> Vec<(usize, usize, Vec<String>)> {
        let fix = f.fix.as_ref().expect("a fix");
        fix.edits
            .iter()
            .map(|e| (e.line, e.remove, e.insert.clone()))
            .collect()
    }

    #[test]
    fn fixes_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/glean-metrics-yaml/dont.yaml"),
            )],
        );
        // An empty `bugs`: a placeholder that stays flagged until filled in.
        assert_eq!(
            edits(&found[0]),
            vec![(
                6,
                1,
                vec![
                    "    bugs:".to_string(),
                    "      - https://bugzilla.mozilla.org/show_bug.cgi?id=TODO".to_string()
                ]
            )]
        );
        // `disabled: false` goes.
        assert_eq!(edits(&found[3]), vec![(11, 1, vec![])]);
        // No link, alias or expiry to make up.
        for i in [1, 2, 4] {
            assert!(found[i].fix.is_none(), "{:#?}", found[i]);
        }
    }

    #[test]
    fn fix_adds_missing_links_and_the_files_team_alias() {
        let yaml = "x:\n  a:\n    type: counter\n    bugs:\n      - 1234567\n    data_reviews:\n      - 1234567\n    notification_emails:\n      - sync-team@mozilla.com\n    expires: never\n  b:\n    type: counter\n    bugs:\n      - 1234567\n    notification_emails:\n      - someone@example.com\n    expires: never\n";
        let found = check(&RULE, &[(PATH, yaml)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(
            edits(&found[0]),
            vec![(
                15,
                0,
                vec![
                    "    data_reviews:".to_string(),
                    "      - https://bugzilla.mozilla.org/show_bug.cgi?id=TODO".to_string()
                ]
            )]
        );
        assert_eq!(
            edits(&found[1]),
            vec![(17, 0, vec!["      - sync-team@mozilla.com".to_string()])]
        );
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/glean-metrics-yaml/do.yaml"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn dates() {
        assert_eq!(super::parse_date("1970-01-01"), Some(0));
        assert_eq!(super::parse_date("2000-03-01"), Some(11_017));
        assert_eq!(super::parse_date("never"), None);
    }

    #[test]
    fn version_expiry_uses_version_txt() {
        let yaml = "x:\n  m:\n    type: counter\n    bugs:\n      - https://bugzilla.mozilla.org/show_bug.cgi?id=1\n    data_reviews:\n      - https://bugzilla.mozilla.org/show_bug.cgi?id=1#c2\n    notification_emails:\n      - sync-team@mozilla.com\n    expires: 162\n";
        let found = check(&RULE, &[("version.txt", "161.0a1\n"), (PATH, yaml)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        let later = yaml.replace("expires: 162", "expires: 170");
        assert!(check(&RULE, &[("version.txt", "161.0a1\n"), (PATH, &later)]).is_empty());
    }
}
