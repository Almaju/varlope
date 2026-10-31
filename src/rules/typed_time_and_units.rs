//! Typed durations, sizes and timestamps; don't substitute `now` for bad data
//!
//! Docs and examples: `rules/typed-time-and-units/`.

use syn::visit::Visit;

use crate::helpers::api_mobile::{is_production_rust, words};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;

pub static RULE: Rule = Rule {
    id: "typed-time-and-units",
    area: Area::Api,
    title: "Typed durations, sizes and timestamps; don't substitute `now` for bad data",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Words that make a name a span of time.
const DURATION_WORDS: &[&str] = &[
    "timeout", "ttl", "interval", "delay", "duration", "period", "backoff", "lifetime", "elapsed",
    "ago", "cooldown", "debounce",
];
/// Sub-day units: with one of these the name is a duration (`by_seconds`,
/// `max_age_ms`). Days and hours are left out: day counts feed calendar math.
const UNIT_WORDS: &[&str] = &[
    "ms",
    "millis",
    "milliseconds",
    "secs",
    "seconds",
    "mins",
    "minutes",
    "micros",
    "nanos",
];
/// Words that make an `interval` name a count instead (`max_requests_in_interval`).
const COUNT_WORDS: &[&str] = &[
    "requests", "count", "num", "limit", "attempts", "retries", "items", "records", "size",
];
/// Words that make a name a point in time (a different rule of thumb).
const TIMESTAMP_WORDS: &[&str] = &[
    "at",
    "time",
    "date",
    "timestamp",
    "modified",
    "created",
    "since",
    "until",
    "last",
    "epoch",
    "now",
    "when",
    "deadline",
    "expires",
    "expiry",
    "expiration",
    "start",
    "end",
    "updated",
    "used",
];
/// Calls that turn untrusted data into a time, and can fail.
const PARSING: &[&str] = &[
    "ok",
    "parse",
    "from_str",
    "as_u64",
    "as_i64",
    "as_f64",
    "as_str",
    "try_from",
    "try_into",
    "checked_add",
    "checked_sub",
    "from_timestamp",
    "from_timestamp_opt",
    "from_timestamp_millis",
    "timestamp_opt",
    "from_rfc3339",
    "parse_from_rfc3339",
    "from_str_radix",
];

fn check(cx: &Context, out: &mut Findings) {
    for file in cx.ws.rust_files() {
        if !is_production_rust(&file.path) {
            continue;
        }
        let mut v = Visitor {
            file,
            hits: Vec::new(),
        };
        v.visit_file(&file.ast);
        for (line, message) in v.hits {
            out.add(&file.path, line, message);
        }
    }
}

struct Visitor<'a> {
    file: &'a RustFile,
    hits: Vec<(usize, String)>,
}

fn is_exported_fn(attrs: &[syn::Attribute]) -> bool {
    syntax::has_attr(attrs, "uniffi::export")
}

fn is_integer(ty: &str) -> bool {
    let inner = ty
        .strip_prefix("Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(ty);
    matches!(
        inner,
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
    )
}

/// `Some(message)` if an integer named `name` should be a `Duration`. A name
/// that states its unit (`ttl_seconds`) is only reported on exported API,
/// where `Duration` also documents the unit for Kotlin and Swift.
fn duration_message(name: &str, ty: &str, exported: bool) -> Option<String> {
    if !is_integer(ty) {
        return None;
    }
    let w = words(name);
    let has = |list: &[&str]| w.iter().any(|x| list.contains(&x.as_str()));
    if has(TIMESTAMP_WORDS) || has(COUNT_WORDS) || w.len() < 2 && !has(DURATION_WORDS) {
        return None;
    }
    match (has(DURATION_WORDS), has(UNIT_WORDS)) {
        (true, false) => Some(format!(
            "`{name}: {ty}` is a duration with no unit; use `std::time::Duration` (or at least name the unit)"
        )),
        (_, true) if exported => Some(format!(
            "exported `{name}: {ty}` is a duration in a bare integer; `std::time::Duration` crosses UniFFI and carries its unit"
        )),
        _ => None,
    }
}

fn derives_serde(attrs: &[syn::Attribute]) -> bool {
    syntax::has_derive(attrs, "Serialize") || syntax::has_derive(attrs, "Deserialize")
}

impl Visitor<'_> {
    fn sig(&mut self, sig: &syn::Signature, exported: bool) {
        for input in &sig.inputs {
            let syn::FnArg::Typed(t) = input else {
                continue;
            };
            let syn::Pat::Ident(p) = &*t.pat else {
                continue;
            };
            let name = p.ident.to_string();
            if name.starts_with('_') {
                continue;
            }
            if let Some(message) = duration_message(&name, &syntax::type_string(&t.ty), exported) {
                self.hits.push((p.ident.span().start().line, message));
            }
        }
    }
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.sig(&f.sig, is_exported_fn(&f.attrs));
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let exported = is_exported_fn(&i.attrs);
        for item in &i.items {
            if let syn::ImplItem::Fn(f) = item {
                if i.trait_.is_none() {
                    let public = matches!(f.vis, syn::Visibility::Public(_));
                    self.sig(&f.sig, exported && public); // a trait impl can't pick its types
                }
                self.visit_block(&f.block);
            }
        }
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        let exported = is_exported_fn(&t.attrs);
        for item in &t.items {
            if let syn::TraitItem::Fn(f) = item {
                if !syntax::is_test_gated(&f.attrs) {
                    self.sig(&f.sig, exported);
                }
            }
        }
    }

    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        // Serde types mirror a wire or storage format whose names are fixed.
        if derives_serde(&s.attrs) {
            return;
        }
        let exported = syntax::has_derive(&s.attrs, "uniffi::Record");
        for field in &s.fields {
            let Some(ident) = &field.ident else { continue };
            if let Some(message) = duration_message(
                &ident.to_string(),
                &syntax::type_string(&field.ty),
                exported,
            ) {
                self.hits.push((ident.span().start().line, message));
            }
        }
    }

    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        let name = c.ident.to_string().to_lowercase();
        if let Some(message) = duration_message(&name, &syntax::type_string(&c.ty), false) {
            self.hits.push((
                c.ident.span().start().line,
                message.replace(&format!("`{name}:"), &format!("`{}:", c.ident)),
            ));
        }
    }

    /// `parsed.unwrap_or(now)`, `.unwrap_or_else(SystemTime::now)`.
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if method == "unwrap_or" || method == "unwrap_or_else" {
            let arg = call.args.first().map(syntax::tokens).unwrap_or_default();
            // `now`, `SystemTime::now`, `|| Timestamp::now()`, `|_| Utc::now()`.
            let is_now = words(&arg).last().is_some_and(|w| w == "now");
            if is_now && parses(&call.receiver) {
                self.hits.push((
                    call.method.span().start().line,
                    "unparseable or missing time silently becomes \"now\"; return an error (or skip the record) instead".to_string(),
                ));
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !self.file.in_test(m.ident.span().start().line) {
            syn::visit::visit_item_mod(self, m);
        }
    }
}

/// Whether a receiver chain parses or converts untrusted data.
fn parses(expr: &syn::Expr) -> bool {
    let mut e = expr;
    loop {
        match e {
            syn::Expr::MethodCall(m) => {
                if PARSING.contains(&m.method.to_string().as_str()) {
                    return true;
                }
                e = &m.receiver;
            }
            syn::Expr::Call(c) => {
                return matches!(&*c.func, syn::Expr::Path(p) if p.path.segments.last()
                    .is_some_and(|s| PARSING.contains(&s.ident.to_string().as_str())));
            }
            syn::Expr::Paren(p) => e = &p.expr,
            syn::Expr::Try(t) => e = &t.expr,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/remote_settings/src/cache.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/typed-time-and-units/dont.rs"),
            )],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 9], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/typed-time-and-units/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_params_fields_and_consts() {
        let src = r#"
pub struct ClientSettings { pub timeout: u32, pub redirect_limit: u32 }
pub fn new(op_name: &str, periodic_interval: u64, max_requests_in_interval: u16) {}
const CLIENTS_TTL: u32 = 1_814_400;
const DEBOUNCE_WINDOW_MS: i64 = 500;
#[uniffi::export]
pub fn advance_event_time(by_seconds: i64) {}
pub fn record_past_event(seconds_ago: i64, millis: i64, rotation_days: u8) {}
"#;
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 3, 4, 7], "{found:#?}");
        assert!(found[2].message.contains("`CLIENTS_TTL: u32`"));
    }

    #[test]
    fn ignores_timestamps_wire_types_trait_impls_and_defaults() {
        let src = r#"
#[derive(Serialize, Deserialize)]
pub struct Bso { pub ttl: Option<u32> }
pub struct Login { pub time_created: i64, pub server_modified_millis: i64 }
impl Clock for C { fn advance(&self, secs: i64) {} }
pub fn timeout(d: Duration) {}
pub trait Clock {
    #[cfg(test)]
    fn advance(&self, secs: i64);
}
fn insert(bm: &Bookmark) { let at = bm.date_added().unwrap_or_else(Timestamp::now); }
"#;
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_now_for_unparseable_times() {
        let src = r#"
fn a(v: &Value) -> SystemTime { v.as_u64().map(from_ms).unwrap_or_else(SystemTime::now) }
fn b(s: &str, now: Timestamp) -> Timestamp { s.parse().ok().unwrap_or(now) }
fn c(s: &str) -> DateTime<Utc> { DateTime::parse_from_rfc3339(s).map(Into::into).unwrap_or_else(|_| Utc::now()) }
"#;
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 3, 4], "{found:#?}");
    }
}
