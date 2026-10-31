//! No URLs, titles, usernames or record payloads in logs, errors or reports
//!
//! Docs and examples: `rules/no-pii-in-logs/`.
//!
//! Looks at the arguments of logging and reporting macros (`trace!` …
//! `error!`, `report_error!`, `breadcrumb!`) and of `format!` calls that build
//! an error variant. An argument is user data when the value it ends with is
//! named `title`, `username`, `origin`, `payload`, `body`, or ends in
//! `_title`/`_record`, or when a `record`/`bso` value is printed with `{:?}`.
//! A `url`/`*_url` counts in the components that store browsing data
//! (history, tabs, logins...); elsewhere URLs are mostly service endpoints.
//! `redact_url(..)` and reductions such as `.len()` are fine.

use syn::visit::Visit;

use crate::helpers::errors::{self, text, TestSupport};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "no-pii-in-logs",
    area: Area::Privacy,
    title: "No URLs, titles, usernames or record payloads in logs, errors or reports",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const LOG_MACROS: &[&str] = &[
    "trace",
    "debug",
    "info",
    "warn",
    "error",
    "report_error",
    "breadcrumb",
];

const PII_NAMES: &[&str] = &["title", "username", "origin", "payload", "body"];
const PII_SUFFIXES: &[&str] = &["_title", "_origin", "_record", "_payload"];
const URL_NAMES: &[&str] = &["url", "uri", "href"];
const URL_SUFFIXES: &[&str] = &["_url", "_uri", "_href"];

/// Components whose URLs are the user's browsing data, not service endpoints.
const BROWSING_DATA: &[&str] = &[
    "places",
    "tabs",
    "logins",
    "autofill",
    "relevancy",
    "suggest",
    "webext-storage",
    "breach-alerts",
    "fxcontainers",
];

/// Methods that reduce a value to something that is not user data.
const REDUCERS: &[&str] = &[
    "len", "is_empty", "is_some", "is_none", "scheme", "count", "id", "guid", "redact_url",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in errors::shipped_files(cx.ws) {
        let support = TestSupport::of(file);
        let browsing = errors::component_of(&file.path).is_some_and(|c| BROWSING_DATA.contains(&c));
        let mut v = Logs {
            hits: Vec::new(),
            browsing,
        };
        v.visit_file(&file.ast);
        for (line, column, message) in v.hits {
            if !support.contains(line) {
                out.add_at(&file.path, line, column, message);
            }
        }
    }
}

struct Logs {
    hits: Vec<(usize, usize, String)>,
    /// Whether URLs in this file are browsing data.
    browsing: bool,
}

/// The name a value ends with: `url` for `self.url`, `url` for `url.as_str()`,
/// `None` for `url.len()` or `redact_url(url)`.
fn value_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(p) => p.path.get_ident().map(|i| i.to_string()),
        syn::Expr::Field(f) => match &f.member {
            syn::Member::Named(n) => Some(n.to_string()),
            syn::Member::Unnamed(_) => value_name(&f.base),
        },
        syn::Expr::Reference(r) => value_name(&r.expr),
        syn::Expr::Paren(p) => value_name(&p.expr),
        syn::Expr::MethodCall(m) => {
            let name = m.method.to_string();
            if REDUCERS.contains(&name.as_str()) {
                None
            } else if matches!(
                name.as_str(),
                "as_str" | "as_ref" | "to_string" | "clone" | "as_deref" | "unwrap" | "to_owned"
            ) {
                value_name(&m.receiver)
            } else {
                None
            }
        }
        _ => None, // calls (`redact_url(url)`), literals...
    }
}

fn is_pii_name(name: &str, browsing: bool) -> bool {
    let n = name.to_lowercase();
    // Flags and counts about a record are not the record; file paths are not browsing data.
    let not_data = ["is_", "has_", "can_", "should_", "num_", "count_"]
        .iter()
        .any(|p| n.starts_with(p))
        || ["file", "path", "db"].iter().any(|w| n.contains(w));
    if not_data {
        return false;
    }
    PII_NAMES.contains(&n.as_str())
        || PII_SUFFIXES.iter().any(|s| n.ends_with(s))
        || (browsing
            && (URL_NAMES.contains(&n.as_str()) || URL_SUFFIXES.iter().any(|s| n.ends_with(s))))
}

fn is_record_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "record" || n == "bso" || n.ends_with("_record") || n.ends_with("_bso")
}

/// The `{..}` placeholders of a format string: `(name or position, debug)`.
fn placeholders(fmt: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut rest = fmt;
    let mut position = 0;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if rest.starts_with('{') {
            rest = &rest[1..];
            continue;
        }
        let Some(close) = rest.find('}') else { break };
        let inner = &rest[..close];
        rest = &rest[close + 1..];
        let (arg, spec) = inner.split_once(':').unwrap_or((inner, ""));
        let debug = spec.contains('?');
        let arg = if arg.is_empty() {
            position += 1;
            format!("#{}", position - 1)
        } else {
            arg.to_string()
        };
        out.push((arg, debug));
    }
    out
}

impl Logs {
    /// Check a format-style argument list: `"fmt", args..`.
    fn check_format(&mut self, mac: &syn::Macro, args: &[syn::Expr], what: &str) {
        let Some(fmt_index) = args.iter().position(|a| {
            matches!(a, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(_), .. }))
        }) else {
            return;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(fmt),
            ..
        }) = &args[fmt_index]
        else {
            return;
        };
        let positional: Vec<&syn::Expr> = args[fmt_index + 1..]
            .iter()
            .filter(|a| !matches!(a, syn::Expr::Assign(_)))
            .collect();
        let mut found: Option<(String, bool)> = None;
        for (arg, debug) in placeholders(&fmt.value()) {
            let name = match arg.strip_prefix('#') {
                Some(i) => i
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| positional.get(i))
                    .and_then(|e| value_name(e)),
                None if arg.chars().all(|c| c.is_alphanumeric() || c == '_') => Some(arg.clone()),
                None => None,
            };
            let Some(name) = name else { continue };
            if is_pii_name(&name, self.browsing) || (debug && is_record_name(&name)) {
                found = Some((name, debug));
                break;
            }
        }
        if let Some((name, debug)) = found {
            let start = mac.path.segments[0].ident.span().start();
            let how = if debug { "`{:?}` of " } else { "" };
            self.hits.push((
                start.line,
                start.column + 1,
                format!("{what} includes {how}`{name}`, which can hold user data; log a guid, a count or `redact_url(..)` instead"),
            ));
        }
    }
}

impl<'ast> Visit<'ast> for Logs {
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let name = syntax::macro_name(m);
        if LOG_MACROS.contains(&name.as_str()) {
            if let Some(args) = errors::macro_args(m) {
                self.check_format(m, &args, &format!("`{name}!`"));
            }
        }
        errors::visit_macro_args(self, m);
        syn::visit::visit_macro(self, m);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        // `Error::Variant(format!(..))`: the message ends up in error reports.
        if let syn::Expr::Path(p) = &*c.func {
            let is_variant = p.path.segments.len() >= 2
                && p.path.segments[p.path.segments.len() - 2]
                    .ident
                    .to_string()
                    .ends_with("Error");
            if is_variant {
                for arg in &c.args {
                    if let syn::Expr::Macro(m) = arg {
                        if syntax::macro_name(&m.mac) == "format" {
                            if let Some(args) = errors::macro_args(&m.mac) {
                                let variant = text(&p.path);
                                self.check_format(&m.mac, &args, &format!("the message of `{variant}`"));
                            }
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/tabs/src/sync/engine.rs";

    fn run(path: &str, src: &str) -> Vec<crate::Finding> {
        check(&RULE, &[(path, src)])
    }

    #[test]
    fn flags_dont() {
        let found = run(PATH, include_str!("../../rules/no-pii-in-logs/dont.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn accepts_do() {
        assert!(run(PATH, include_str!("../../rules/no-pii-in-logs/do.rs")).is_empty());
    }

    #[test]
    fn flags_urls_titles_and_error_messages() {
        let src = "fn f(url: &Url, login: &Login) -> Error {\n    trace!(\"visit {} {}\", url, 3);\n    warn!(\"bad login {}\", login.username);\n    Error::InvalidLogin(format!(\"bad origin {}\", login.origin))\n}\n";
        let lines: Vec<usize> = run("components/logins/src/db.rs", src).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 3, 4]);
    }

    #[test]
    fn ignores_redacted_reduced_and_endpoint_urls() {
        let src = "fn f(url: &Url, record: &TabsRecord) {\n    debug!(\"{}\", redact_url(url.as_str()));\n    debug!(\"{} tabs in {}\", record.tabs.len(), record.id);\n    debug!(\"can post: {}\", can_post_record);\n}\n";
        assert!(run(PATH, src).is_empty());
        let endpoint = "fn f(url: &Url) { info!(\"unsubscribed from {}\", url); }";
        assert!(run("components/push/src/internal/communications.rs", endpoint).is_empty());
    }
}
