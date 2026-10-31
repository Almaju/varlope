//! Build URLs with `url::Url`, never by string concat; no hard-coded endpoints
//!
//! Docs and examples: `rules/url-hygiene/`.

use std::collections::BTreeMap;

use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::visit::Visit;

use crate::helpers::api_mobile::{is_production_rust, words};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "url-hygiene",
    area: Area::Api,
    title: "Build URLs with `url::Url`, never by string concat; no hard-coded endpoints",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Words that make a value a URL (or part of one).
const URL_WORDS: &[&str] = &["url", "uri", "endpoint", "host"];

fn check(cx: &Context, out: &mut Findings) {
    // crate dir -> URL literal -> where it appears
    let mut literals: BTreeMap<String, BTreeMap<String, Vec<(String, usize)>>> = BTreeMap::new();
    for file in cx.ws.rust_files() {
        if !is_production_rust(&file.path) {
            continue;
        }
        let mut v = Visitor {
            file,
            consts: consts(file),
            hits: Vec::new(),
            urls: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, message) in v.hits {
            if !file.in_test(line) {
                out.add(&file.path, line, message);
            }
        }
        let krate = file.path.split("/src/").next().unwrap_or("").to_string();
        for (line, url) in v.urls {
            if !file.in_test(line) {
                literals
                    .entry(krate.clone())
                    .or_default()
                    .entry(url)
                    .or_default()
                    .push((file.path.clone(), line));
            }
        }
    }
    for urls in literals.values() {
        for (url, places) in urls {
            let Some((first_path, first_line)) = places.first() else {
                continue;
            };
            for (path, line) in &places[1..] {
                let at = if path == first_path {
                    format!("line {first_line}")
                } else {
                    format!("{first_path}:{first_line}")
                };
                out.add(
                    path,
                    *line,
                    format!("endpoint `{url}` is also hard-coded at {at}; define it once as a `const` and build on it"),
                );
            }
        }
    }
}

/// `const NAME: &str = "https://..";` values in a file.
fn consts(file: &RustFile) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for item in syntax::all_items(&file.ast) {
        if let syn::Item::Const(c) = item {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &*c.expr
            {
                out.insert(c.ident.to_string(), s.value());
            }
        }
    }
    out
}

struct Visitor<'a> {
    file: &'a RustFile,
    consts: BTreeMap<String, String>,
    hits: Vec<(usize, String)>,
    /// Service URL literals, to find the ones hard-coded twice.
    urls: Vec<(usize, String)>,
}

impl Visitor<'_> {
    /// `format!("{}/v1/token", base_url)` or `format!("{endpoint}/api")`.
    fn format_macro(&mut self, mac: &syn::Macro) {
        let name = syntax::macro_name(mac);
        if !matches!(
            name.as_str(),
            "format" | "write" | "writeln" | "format_args"
        ) {
            return;
        }
        let Ok(args) =
            mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        else {
            return;
        };
        let mut args = args.into_iter();
        if matches!(name.as_str(), "write" | "writeln") {
            args.next(); // the destination
        }
        let Some(syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(fmt),
            ..
        })) = args.next()
        else {
            return;
        };
        let rest: Vec<syn::Expr> = args.collect();
        let text = fmt.value();
        let Some(close) = text.strip_prefix('{').and_then(|t| t.find('}')) else {
            return;
        };
        let after = &text[close + 2..];
        // `format!("{}/", base)` only adds the trailing `/` that `Url::join` needs.
        if after == "/" || !after.starts_with(['/', '?', '&', '#']) {
            return;
        }
        let placeholder = text[1..close + 1].split(':').next().unwrap_or("").trim();
        let named = |n: &str| {
            rest.iter().find_map(|e| match e {
                syn::Expr::Assign(a) if syntax::tokens(&a.left) == n => Some((*a.right).clone()),
                _ => None,
            })
        };
        let operand = if placeholder.is_empty() || placeholder == "0" {
            rest.iter()
                .find(|e| !matches!(e, syn::Expr::Assign(_)))
                .map(last_name)
        } else if placeholder.chars().all(|c| c.is_ascii_digit()) {
            placeholder
                .parse::<usize>()
                .ok()
                .and_then(|i| rest.get(i))
                .map(last_name)
        } else {
            Some(
                named(placeholder)
                    .as_ref()
                    .map_or_else(|| placeholder.to_string(), last_name),
            )
        };
        let Some(operand) = operand.filter(|o| !o.is_empty()) else {
            return;
        };
        if words(&operand)
            .iter()
            .any(|w| URL_WORDS.contains(&w.as_str()))
        {
            self.hits.push((
                fmt.span().start().line,
                format!(
                    "URL built by formatting `{operand}`; use `url::Url` instead (`join` on a base ending in `/`, `query_pairs_mut()`)"
                ),
            ));
        }
    }
}

fn macro_literals(stream: proc_macro2::TokenStream, out: &mut Vec<(usize, String)>) {
    for tt in stream {
        match tt {
            proc_macro2::TokenTree::Group(g) => macro_literals(g.stream(), out),
            proc_macro2::TokenTree::Literal(lit) => {
                let line = lit.span().start().line;
                let tokens = proc_macro2::TokenStream::from(proc_macro2::TokenTree::Literal(lit));
                if let Ok(syn::Lit::Str(s)) = syn::parse2::<syn::Lit>(tokens) {
                    out.push((line, s.value()));
                }
            }
            _ => {}
        }
    }
}

/// The name an operand ends with: `base_url` for `self.config.base_url`,
/// `""` for calls and other expressions.
fn last_name(e: &syn::Expr) -> String {
    match e {
        syn::Expr::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        syn::Expr::Field(f) => match &f.member {
            syn::Member::Named(n) => n.to_string(),
            syn::Member::Unnamed(_) => String::new(),
        },
        syn::Expr::Reference(r) => last_name(&r.expr),
        syn::Expr::Paren(p) => last_name(&p.expr),
        _ => String::new(),
    }
}

/// Domains of the services components talk to.
const SERVICE_DOMAINS: &[&str] = &[
    "mozilla.com",
    "mozilla.org",
    "mozilla.net",
    "allizom.org",
    "firefox.com",
    "mozaws.net",
];

/// The URL in a string literal, if it is an `https://` endpoint of a Mozilla
/// service (`identity.mozilla.com` URLs are OAuth scope names, not endpoints).
fn endpoint(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("https://")?;
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
    let service = SERVICE_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")));
    (service && host != "identity.mozilla.com" && !text.contains(char::is_whitespace))
        .then_some(text)
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.format_macro(mac);
        // `url!("..")`, `format!("..")`: count the literals inside too.
        let mut literals = Vec::new();
        macro_literals(mac.tokens.clone(), &mut literals);
        for (line, value) in literals {
            if let Some(url) = endpoint(&value) {
                self.urls.push((line, url.to_string()));
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    // Doc comments and attributes are not endpoints.
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        if let Some(url) = endpoint(&lit.value()) {
            self.urls.push((lit.span().start().line, url.to_string()));
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "join" {
            if let Some(base) = literal_base(&call.receiver, &self.consts) {
                let path = base
                    .split_once("://")
                    .map_or("", |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
                if !path.is_empty() && !base.ends_with('/') {
                    self.hits.push((
                        call.method.span().start().line,
                        format!(
                            "`Url::join` on `{base}` drops its last path segment because the base has no trailing `/`; end the base with `/`"
                        ),
                    ));
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let is_parse = matches!(&*call.func, syn::Expr::Path(p) if syntax::path_ends_with(&p.path, "Url::parse"));
        if is_parse {
            if let Some(arg) = call.args.first() {
                let text = syntax::tokens(arg);
                if words(&text).iter().any(|w| w == "location") {
                    self.hits.push((
                        self.file.line_of_node(&call.func),
                        "a `Location` header may be relative; resolve it with `request_url.join(..)` instead of `Url::parse`".to_string(),
                    ));
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// The literal a `join` receiver was parsed from: `Url::parse("..")?`,
/// `url!("..")`, or the same with a `const`.
fn literal_base(receiver: &syn::Expr, consts: &BTreeMap<String, String>) -> Option<String> {
    let mut e = receiver;
    loop {
        match e {
            syn::Expr::Try(t) => e = &t.expr,
            syn::Expr::Paren(p) => e = &p.expr,
            syn::Expr::MethodCall(m) if m.method == "unwrap" || m.method == "expect" => {
                e = &m.receiver
            }
            _ => break,
        }
    }
    let arg = match e {
        syn::Expr::Call(c) => match &*c.func {
            syn::Expr::Path(p) if syntax::path_ends_with(&p.path, "Url::parse") => {
                c.args.first()?.clone()
            }
            _ => return None,
        },
        syn::Expr::Macro(m) if syntax::macro_name(&m.mac) == "url" => {
            syn::parse2::<syn::Expr>(m.mac.tokens.clone()).ok()?
        }
        _ => return None,
    };
    match arg {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.value()),
        syn::Expr::Path(p) => consts.get(&p.path.to_token_stream().to_string()).cloned(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/mars.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/url-hygiene/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 10], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/url-hygiene/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_formatted_urls_by_operand_name() {
        let src = r#"
fn a(base_host: &str, endpoint: &str, cfg: &Config, url: &Url) {
    let x = format!("{}/api/v1/suggest", base_host);
    let y = format!("{endpoint}/api/v6/experiments/");
    let z = format!("{}/v1/oauth/token", cfg.auth_server_base_url);
    let ok1 = format!("{}/", cfg.auth_server_base_url);
    let ok2 = format!("{}?{}", url.path(), qs);
    let ok3 = format!("{bucket}/{collection}");
}
"#;
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 4, 5], "{found:#?}");
    }

    #[test]
    fn flags_repeated_endpoints_relative_locations_and_joins() {
        let src = r#"
fn raw(&self) -> &str { "https://firefox.settings.services.mozilla.com/v2" }
fn url(&self) -> Url { Url::parse("https://firefox.settings.services.mozilla.com/v2").unwrap() }
fn redirect(resp: &Response) -> Url { Url::parse(resp.headers.get("location")).unwrap() }
fn ads() -> Url { Url::parse("https://ads.mozilla.org/v1").unwrap().join("ads").unwrap() }
#[cfg(test)]
mod tests { const U: &str = "https://ads.mozilla.org/v1"; }
"#;
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 4, 5], "{found:#?}");
    }
}
