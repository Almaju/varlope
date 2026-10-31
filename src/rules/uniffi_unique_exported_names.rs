//! Exported type names must be unique across the iOS megazord.
//!
//! Docs and examples: `rules/uniffi-unique-exported-names/`.

use std::collections::{BTreeMap, BTreeSet};

use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::uniffi::{self, ApiItem};

pub static RULE: Rule = Rule {
    id: "uniffi-unique-exported-names",
    area: Area::Uniffi,
    title: "Exported names must be unique across the megazord (Swift has one flat module)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Names generic enough that the next component will want them too.
const GENERIC: &[&str] = &[
    "ApiError",
    "Error",
    "Record",
    "Level",
    "Method",
    "Headers",
    "Request",
    "Response",
    "Backend",
    "Attachment",
    "Config",
    "Metadata",
    "Client",
    "Store",
    "Settings",
    "Entry",
    "Item",
    "Status",
    "Callback",
    "Event",
    "Context",
    "initialize",
];

fn check(cx: &Context, out: &mut Findings) {
    // name -> crate -> where it is declared
    let mut owners: BTreeMap<String, BTreeMap<String, ApiItem>> = BTreeMap::new();
    for crate_name in cx.ws.ios_megazord_crates() {
        let Some(package) = cx.ws.package(&crate_name) else {
            continue;
        };
        for item in uniffi::package_api(cx.ws, package) {
            owners
                .entry(item.name.clone())
                .or_default()
                .entry(crate_name.clone())
                .or_insert(item);
        }
    }
    for (name, crates) in &owners {
        if crates.len() > 1 {
            let all: BTreeSet<&str> = crates.keys().map(String::as_str).collect();
            for (krate, item) in crates {
                let others: Vec<&str> = all.iter().copied().filter(|c| *c != krate).collect();
                out.add(
                    &item.path,
                    item.line,
                    format!(
                        "`{name}` is also exported by {}; the iOS megazord puts both in one Swift module",
                        others.join(", ")
                    ),
                );
            }
        } else if GENERIC.contains(&name.as_str()) {
            let (krate, item) = crates.iter().next().expect("at least one owner");
            out.add(
                &item.path,
                item.line,
                format!(
                    "`{name}` is generic enough to collide in the shared Swift module; prefix it (e.g. `{}`)",
                    prefixed(krate, name)
                ),
            );
        }
    }
}

/// `AutofillStore` for a type, `autofill_initialize` for a function.
fn prefixed(krate: &str, name: &str) -> String {
    if name.starts_with(char::is_lowercase) {
        format!("{krate}_{name}")
    } else {
        format!("{}{name}", pascal(krate))
    }
}

fn pascal(snake: &str) -> String {
    snake
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const MEGAZORD: (&str, &str) = (
        "megazords/ios-rust/src/lib.rs",
        "pub use autofill;\npub use logins;\n",
    );
    const AUTOFILL: (&str, &str) = (
        "components/autofill/Cargo.toml",
        "[package]\nname = \"autofill\"\n",
    );
    const LOGINS: (&str, &str) = (
        "components/logins/Cargo.toml",
        "[package]\nname = \"logins\"\n",
    );
    const LOGINS_SRC: (&str, &str) = (
        "components/logins/src/lib.rs",
        "/// Key.\n#[uniffi::export]\npub fn create_key() -> ApiResult<String> { todo!() }\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MEGAZORD,
                AUTOFILL,
                LOGINS,
                LOGINS_SRC,
                (
                    "components/autofill/src/lib.rs",
                    include_str!("../../rules/uniffi-unique-exported-names/dont.rs"),
                ),
            ],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("also exported by logins"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MEGAZORD,
                AUTOFILL,
                LOGINS,
                LOGINS_SRC,
                (
                    "components/autofill/src/lib.rs",
                    include_str!("../../rules/uniffi-unique-exported-names/do.rs"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_generic_names() {
        let found = check(
            &RULE,
            &[
                MEGAZORD,
                AUTOFILL,
                (
                    "components/autofill/src/lib.rs",
                    "#[derive(uniffi::Record)]\npub struct Headers { pub a: u8 }\n",
                ),
            ],
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("AutofillHeaders"));
    }
}
