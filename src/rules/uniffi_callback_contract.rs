//! Callback interfaces return `Result`, are released on shutdown, and user-waiting calls are async
//!
//! Docs and examples: `rules/uniffi-callback-contract/`.
//!
//! Checks traits the foreign side implements (`#[uniffi::export(callback_interface)]`,
//! `#[uniffi::export(with_foreign)]`, UDL `callback interface` and
//! `[Trait, WithForeign] interface`):
//! - methods that don't return a `Result`: an exception thrown by the
//!   Kotlin/Swift/JS implementation then panics in Rust (fire-and-forget
//!   telemetry and logging sinks such as `record_*` or `log` are left alone);
//! - traits mixing sync and `async` methods;
//! - a struct that keeps such a callback in a plain `Arc<dyn ..>` field while
//!   offering `shutdown()`/`close(&self)`, which then can't release it.
//!
//! (UniFFI itself requires a callback's error type to implement
//! `From<UnexpectedUniFFICallbackError>`, so that part needs no check.)

use std::collections::BTreeSet;

use crate::helpers::uniffi::{self as api, FnSig, Item, Shape};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::uniffi::MemberKind;

pub static RULE: Rule = Rule {
    id: "uniffi-callback-contract",
    area: Area::Uniffi,
    title: "Callback interfaces return `Result`, are released on shutdown, and user-waiting calls are async",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    let mut foreign: BTreeSet<String> = BTreeSet::new();
    let mut traits: Vec<Item> = Vec::new();
    for file in cx.ws.files() {
        if !api::is_shipped_api_file(&file.path) {
            continue;
        }
        let items = if file.extension() == "udl" {
            api::udl_items(&file.path, &file.text)
        } else if let Some(rust) = cx.ws.rust(&file.path) {
            api::rust_items(rust)
        } else {
            continue;
        };
        for item in items.into_iter().filter(|i| i.foreign && i.decl) {
            foreign.insert(item.name.clone());
            traits.push(item);
        }
    }
    for item in &traits {
        check_trait(item, out);
    }
    for file in cx.ws.rust_files() {
        if api::is_shipped_api_file(&file.path) {
            check_holders(file, &foreign, out);
        }
    }
}

fn check_trait(item: &Item, out: &mut Findings) {
    let methods: Vec<(&crate::helpers::uniffi::Member, &FnSig)> = item
        .members
        .iter()
        .filter(|m| m.kind == MemberKind::Method)
        .filter_map(|m| match &m.shape {
            Shape::Fn(sig) => Some((m, sig)),
            _ => None,
        })
        .collect();
    let infallible: Vec<_> = methods
        .iter()
        .filter(|(m, s)| s.throws.is_none() && !is_sink(&m.name, s))
        .collect();
    if let Some((first, _)) = infallible.first() {
        let names: Vec<String> = infallible
            .iter()
            .map(|(m, _)| format!("`{}`", m.name))
            .collect();
        out.add(
            &first.path,
            first.line,
            format!(
                "callback `{}` method{} {} return{} no `Result`, so an exception in the Kotlin/Swift/JS implementation panics in Rust; return `Result<_, E>` with `E: From<uniffi::UnexpectedUniFFICallbackError>`",
                item.name,
                if names.len() == 1 { "" } else { "s" },
                names.join(", "),
                if names.len() == 1 { "s" } else { "" },
            ),
        );
    }
    let (asynchronous, sync): (Vec<_>, Vec<_>) = methods.iter().partition(|(_, s)| s.is_async);
    if let (Some((a, _)), Some((s, _))) = (asynchronous.first(), sync.first()) {
        out.add(
            &s.path,
            s.line,
            format!(
                "callback `{}` mixes async `{}` with sync `{}`; if one waits on the user or on I/O, make them all async so callers don't block",
                item.name, a.name, s.name
            ),
        );
    }
}

/// Fire-and-forget telemetry and logging methods (`record_*`, `report_*`,
/// `log`, `on_event`...) that return nothing: an implementation can swallow
/// its own errors, and Rust could not act on one anyway.
fn is_sink(name: &str, sig: &FnSig) -> bool {
    const SINKS: &[&str] = &[
        "record", "report", "log", "submit", "on_event", "trace", "emit",
    ];
    sig.ret.is_empty() && SINKS.iter().any(|p| name.starts_with(p))
}

/// Structs that keep a foreign callback in a plain `Arc<dyn T>` / `Box<dyn T>`
/// field and have a `shutdown(&self)` / `close(&self)` that can't drop it.
fn check_holders(file: &RustFile, foreign: &BTreeSet<String>, out: &mut Findings) {
    let items = syntax::all_items(&file.ast);
    for item in &items {
        let syn::Item::Struct(s) = item else { continue };
        let Some(shutdown) = shutdown_method(&items, &s.ident) else {
            continue;
        };
        for field in &s.fields {
            let Some(name) = held_callback(&field.ty) else {
                continue;
            };
            if !foreign.contains(&name) {
                continue;
            }
            let field_name = field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "0".into());
            out.add(
                &file.path,
                file.line_of_node(field),
                format!(
                    "`{}::{shutdown}` can't release the foreign `{name}` held in `{field_name}`, so the Kotlin/Swift/JS object outlives shutdown; keep it in a `Mutex<Option<..>>` that `{shutdown}` clears",
                    s.ident
                ),
            );
        }
    }
}

/// `shutdown` or `close` taking `&self` / `&mut self` in an inherent impl.
fn shutdown_method(items: &[&syn::Item], ty: &syn::Ident) -> Option<String> {
    for item in items {
        let syn::Item::Impl(i) = item else { continue };
        if i.trait_.is_some() || syntax::type_ident(&i.self_ty).as_ref() != Some(&ty.to_string()) {
            continue;
        }
        for it in &i.items {
            let syn::ImplItem::Fn(f) = it else { continue };
            let name = f.sig.ident.to_string();
            let by_ref = matches!(
                f.sig.inputs.first(),
                Some(syn::FnArg::Receiver(r)) if r.reference.is_some()
            );
            if (name == "shutdown" || name == "close") && by_ref {
                return Some(name);
            }
        }
    }
    None
}

/// `T` for a field typed `Arc<dyn T>` or `Box<dyn T>` (not inside an
/// `Option` or a lock, which shutdown could clear).
fn held_callback(ty: &syn::Type) -> Option<String> {
    let outer = syntax::type_ident(ty)?;
    if outer != "Arc" && outer != "Box" {
        return None;
    }
    let inner = *syntax::type_args(ty).first()?;
    match inner {
        syn::Type::TraitObject(_) => Some(api::rust_type(inner)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/logins/src/lib.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/uniffi-callback-contract/dont.rs"),
            )],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![5, 5, 9], "{found:#?}");
        assert!(found
            .iter()
            .any(|f| f.message.contains("returns no `Result`")));
        assert!(found.iter().any(|f| f.message.contains("mixes async")));
        assert!(found[2].message.contains("`Store::shutdown` can't release"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/uniffi-callback-contract/do.rs"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_udl_foreign_traits() {
        let udl = (
            "components/nimbus/src/nimbus.udl",
            "[Trait, WithForeign]\ninterface Context {\n    void set_prefs(string prefs);\n    [Throws=NimbusError]\n    string to_json();\n};\ninterface Plain {\n    void f();\n};\n",
        );
        let found = check(&RULE, &[udl]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
        assert!(found[0].message.contains("`set_prefs` returns no `Result`"));
    }

    #[test]
    fn ignores_telemetry_and_logging_sinks() {
        let src = "#[uniffi::export(callback_interface)]\npub trait Telemetry: Send + Sync {\n    fn record_error(&self, label: String);\n    fn log(&self, message: String);\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
        let src = "#[uniffi::export(callback_interface)]\npub trait Telemetry: Send + Sync {\n    fn record_count(&self) -> u32;\n}\n";
        assert_eq!(check(&RULE, &[(PATH, src)]).len(), 1);
    }

    #[test]
    fn ignores_rust_only_traits_and_clearable_holders() {
        let src = "#[uniffi::export]\npub trait Engine: Send + Sync { fn sync(&self); }\n\
                   #[uniffi::export(callback_interface)]\npub trait Sink: Send + Sync { fn on(&self) -> Result<(), E>; }\n\
                   pub struct A { sink: Mutex<Option<Arc<dyn Sink>>> }\nimpl A { pub fn shutdown(&self) {} }\n\
                   pub struct B { sink: Arc<dyn Sink> }\nimpl B { pub fn shutdown(self) {} }\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert!(found.is_empty(), "{found:#?}");
    }
}
