//! UDL and proc-macro declarations agree; new code uses proc-macros
//!
//! Docs and examples: `rules/uniffi-udl-proc-macro-consistency/`.

use std::collections::BTreeMap;

use crate::diff::Status;
use crate::helpers::api_mobile::{is_production_rust, is_uniffi_crate_file, udl_name_line};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::RustFile;
use crate::syntax;
use crate::udl::{self, UdlKind};
use crate::workspace::{is_test_path, Package, Workspace};

pub static RULE: Rule = Rule {
    id: "uniffi-udl-proc-macro-consistency",
    area: Area::Uniffi,
    title: "UDL and proc-macro declarations agree; new code uses proc-macros",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Where something is declared.
#[derive(Clone, Debug)]
struct Decl {
    path: String,
    line: usize,
}

/// One crate's exports, by name, from each side.
#[derive(Default)]
struct Surface {
    /// Proc-macro types and the UDL `typedef` kinds that may name them.
    rust_types: BTreeMap<String, (Decl, &'static [&'static str])>,
    rust_fns: BTreeMap<String, Decl>,
    /// `Type::method` exported from an `#[uniffi::export] impl`.
    rust_methods: BTreeMap<String, Decl>,
    /// UDL declarations (not typedefs) and the typedef kinds that fit them.
    udl_types: BTreeMap<String, (Decl, &'static [&'static str])>,
    udl_fns: BTreeMap<String, Decl>,
    udl_methods: BTreeMap<String, Decl>,
    /// `typedef <kind> <Name>;` lines: (name, kind, external crate, decl).
    typedefs: Vec<(String, String, Option<String>, Decl)>,
    udl_files: Vec<String>,
}

fn check(cx: &Context, out: &mut Findings) {
    let packages: Vec<&Package> = cx
        .ws
        .packages()
        .iter()
        .filter(|p| p.dependencies().iter().any(|d| d.name == "uniffi"))
        .collect();
    let surfaces: BTreeMap<String, Surface> = packages
        .iter()
        .map(|p| (p.lib_name(), surface(cx.ws, p)))
        .collect();
    for (krate, s) in &surfaces {
        duplicates(s, out);
        typedef_kinds(krate, s, &surfaces, out);
    }
    for file in cx.ws.rust_files() {
        if is_production_rust(&file.path) && is_uniffi_crate_file(cx.ws, &file.path) {
            orphan_method_attrs(file, out);
        }
    }
    new_udl_files(cx, out);
}

const RECORD: &[&str] = &["dictionary", "record"];
const ENUM: &[&str] = &["enum"];
const OBJECT: &[&str] = &["interface", "object"];
const TRAIT: &[&str] = &["trait"];
const FOREIGN_TRAIT: &[&str] = &["trait_with_foreign"];
const CALLBACK: &[&str] = &["callback"];
const CUSTOM: &[&str] = &["custom"];

fn surface(ws: &Workspace, package: &Package) -> Surface {
    let mut s = Surface::default();
    for file in ws.package_files(package) {
        if file.extension() == "udl" && !is_test_path(&file.path) {
            read_udl(&file.path, &file.text, &mut s);
        } else if is_production_rust(&file.path) {
            if let Some(rust) = ws.rust(&file.path) {
                read_rust(rust, &mut s);
            }
        }
    }
    s
}

fn read_udl(path: &str, text: &str, s: &mut Surface) {
    s.udl_files.push(path.to_string());
    let lines: Vec<&str> = text.lines().collect();
    for item in udl::parse(text) {
        let decl = Decl {
            path: path.to_string(),
            line: item.line,
        };
        let kinds: &[&str] = match item.kind {
            UdlKind::Namespace => {
                for m in &item.members {
                    s.udl_fns.insert(
                        m.name.clone(),
                        Decl {
                            path: path.to_string(),
                            line: udl_name_line(text, m.line, &m.name),
                        },
                    );
                }
                continue;
            }
            UdlKind::Typedef => {
                if let Some((kind, line)) = typedef_kind(&lines, item.line, &item.name) {
                    let external = item
                        .attrs
                        .split(',')
                        .find_map(|a| a.trim().strip_prefix("External"))
                        .map(|v| {
                            v.trim_start_matches([' ', '='])
                                .trim_matches([' ', '"'])
                                .to_string()
                        });
                    s.typedefs.push((
                        item.name.clone(),
                        kind,
                        external,
                        Decl {
                            path: path.to_string(),
                            line,
                        },
                    ));
                }
                continue;
            }
            UdlKind::Dictionary => RECORD,
            UdlKind::Enum => ENUM,
            UdlKind::Interface if item.has_attr("Error") || item.has_attr("Enum") => ENUM,
            UdlKind::Interface if item.has_attr("Trait") && item.attrs.contains("WithForeign") => {
                FOREIGN_TRAIT
            }
            UdlKind::Interface if item.has_attr("Trait") => TRAIT,
            UdlKind::Interface => {
                for m in item.members.iter().filter(|m| m.signature.contains('(')) {
                    if m.name != "constructor" {
                        s.udl_methods.insert(
                            format!("{}::{}", item.name, m.name),
                            Decl {
                                path: path.to_string(),
                                line: udl_name_line(text, m.line, &m.name),
                            },
                        );
                    }
                }
                OBJECT
            }
            UdlKind::CallbackInterface => CALLBACK,
            UdlKind::Other => continue,
        };
        s.udl_types.insert(item.name.clone(), (decl, kinds));
    }
}

/// The `<kind>` of `typedef <kind> <name>;`, searched from `line`, and the
/// line it is on. `[Custom] typedef string Url;` (a builtin) is not a kind.
fn typedef_kind(lines: &[&str], line: usize, name: &str) -> Option<(String, usize)> {
    for (index, text) in lines
        .iter()
        .enumerate()
        .skip(line.saturating_sub(1))
        .take(4)
    {
        let Some(rest) = text.split("typedef").nth(1) else {
            continue;
        };
        let mut words = rest
            .split(|c: char| c.is_whitespace() || c == ';')
            .filter(|w| !w.is_empty());
        let kind = words.next()?;
        if words.next() != Some(name) {
            return None;
        }
        let known = [
            "dictionary",
            "record",
            "enum",
            "interface",
            "object",
            "trait",
            "trait_with_foreign",
            "callback",
            "custom",
        ];
        return known.contains(&kind).then(|| (kind.to_string(), index + 1));
    }
    None
}

fn read_rust(file: &RustFile, s: &mut Surface) {
    for item in syntax::all_items(&file.ast) {
        let decl = |line: usize| Decl {
            path: file.path.clone(),
            line,
        };
        match item {
            syn::Item::Struct(st) => {
                let line = file.line_of_node(&st.ident);
                if file.in_test(line) {
                    continue;
                }
                let kinds = if syntax::has_derive(&st.attrs, "uniffi::Record") {
                    RECORD
                } else if syntax::has_derive(&st.attrs, "uniffi::Object")
                    || syntax::has_derive(&st.attrs, "uniffi::Error")
                {
                    OBJECT
                } else {
                    continue;
                };
                s.rust_types
                    .insert(st.ident.to_string(), (decl(line), kinds));
            }
            syn::Item::Enum(e) => {
                let line = file.line_of_node(&e.ident);
                if file.in_test(line) {
                    continue;
                }
                let kinds = if syntax::has_derive(&e.attrs, "uniffi::Enum")
                    || syntax::has_derive(&e.attrs, "uniffi::Error")
                {
                    ENUM
                } else if syntax::has_derive(&e.attrs, "uniffi::Object") {
                    OBJECT
                } else {
                    continue;
                };
                s.rust_types
                    .insert(e.ident.to_string(), (decl(line), kinds));
            }
            syn::Item::Fn(f) if syntax::has_attr(&f.attrs, "uniffi::export") => {
                let line = file.line_of_node(&f.sig.ident);
                if !file.in_test(line) {
                    s.rust_fns.insert(f.sig.ident.to_string(), decl(line));
                }
            }
            syn::Item::Impl(i) if syntax::has_attr(&i.attrs, "uniffi::export") => {
                let Some(ty) = syntax::type_ident(&i.self_ty) else {
                    continue;
                };
                for it in &i.items {
                    let syn::ImplItem::Fn(m) = it else { continue };
                    let line = file.line_of_node(&m.sig.ident);
                    let public = matches!(m.vis, syn::Visibility::Public(_)) || i.trait_.is_some();
                    let ctor = syntax::has_attr(&m.attrs, "uniffi::constructor");
                    if public && !ctor && !file.in_test(line) {
                        s.rust_methods
                            .insert(format!("{ty}::{}", m.sig.ident), decl(line));
                    }
                }
            }
            syn::Item::Trait(t) if syntax::has_attr(&t.attrs, "uniffi::export") => {
                let line = file.line_of_node(&t.ident);
                if file.in_test(line) {
                    continue;
                }
                let args = syntax::find_attr(&t.attrs, "uniffi::export")
                    .map(syntax::attr_args)
                    .unwrap_or_default();
                let kinds = if args.contains("callback_interface") {
                    CALLBACK
                } else if args.contains("with_foreign") {
                    FOREIGN_TRAIT
                } else {
                    TRAIT
                };
                s.rust_types
                    .insert(t.ident.to_string(), (decl(line), kinds));
            }
            syn::Item::Macro(m) => {
                let name = syntax::macro_name(&m.mac);
                if name != "custom_type" && name != "custom_newtype" {
                    continue;
                }
                let line = file.line_of_node(&m.mac);
                if let Some(first) = m.mac.tokens.clone().into_iter().next() {
                    s.rust_types.insert(first.to_string(), (decl(line), CUSTOM));
                }
            }
            _ => {}
        }
    }
}

/// Things declared in the crate's UDL *and* with proc-macros.
fn duplicates(s: &Surface, out: &mut Findings) {
    for (name, udl) in &s.udl_fns {
        if let Some(rust) = s.rust_fns.get(name) {
            out.add(
                &udl.path,
                udl.line,
                format!(
                    "`{name}` is declared here and also exported with `#[uniffi::export]` ({}:{}); delete the UDL copy",
                    rust.path, rust.line
                ),
            );
        }
    }
    for (name, (udl, _)) in &s.udl_types {
        if let Some((rust, _)) = s.rust_types.get(name) {
            out.add(
                &udl.path,
                udl.line,
                format!(
                    "`{name}` is declared here and also derived with UniFFI proc-macros ({}:{}); delete the UDL copy",
                    rust.path, rust.line
                ),
            );
        }
    }
    for (name, udl) in &s.udl_methods {
        if let Some(rust) = s.rust_methods.get(name) {
            out.add(
                &udl.path,
                udl.line,
                format!(
                    "`{name}` is declared here and also in an `#[uniffi::export] impl` ({}:{}); delete the UDL copy",
                    rust.path, rust.line
                ),
            );
        }
    }
}

/// `typedef <kind> Name;` whose kind doesn't match the real declaration, and
/// typedefs nothing in the UDL uses.
fn typedef_kinds(
    krate: &str,
    s: &Surface,
    surfaces: &BTreeMap<String, Surface>,
    out: &mut Findings,
) {
    for (name, kind, external, decl) in &s.typedefs {
        let home = match external {
            Some(other) => surfaces.get(&other.replace('-', "_")),
            None => surfaces.get(krate),
        };
        let Some(home) = home else { continue };
        let found = home
            .rust_types
            .get(name)
            .map(|(_, k)| *k)
            .or_else(|| home.udl_types.get(name).map(|(_, k)| *k));
        let Some(kinds) = found else { continue };
        if !kinds.contains(&kind.as_str()) {
            let owner = external.as_deref().unwrap_or(krate);
            out.add(
                &decl.path,
                decl.line,
                format!(
                    "`typedef {kind} {name}` doesn't match how {owner} declares it; write `typedef {} {name};`",
                    kinds[0]
                ),
            );
        }
    }
}

/// `#[uniffi::constructor]`/`#[uniffi::method]` in an impl that isn't exported.
fn orphan_method_attrs(file: &RustFile, out: &mut Findings) {
    for item in syntax::all_items(&file.ast) {
        let syn::Item::Impl(i) = item else { continue };
        let exported = syntax::has_attr(&i.attrs, "uniffi::export")
            || i.attrs.iter().any(|a| {
                syntax::attr_name(a) == "cfg_attr" && syntax::attr_args(a).contains("export")
            });
        if exported {
            continue;
        }
        for it in &i.items {
            let syn::ImplItem::Fn(m) = it else { continue };
            for attr in ["uniffi::constructor", "uniffi::method"] {
                if syntax::has_attr(&m.attrs, attr) {
                    out.add(
                        &file.path,
                        file.line_of_node(&m.sig.ident),
                        format!(
                            "`#[{attr}]` on `{}` does nothing: its `impl` lacks `#[uniffi::export]`",
                            m.sig.ident
                        ),
                    );
                }
            }
        }
    }
}

/// A PR adding a `.udl` file to a crate that had none.
fn new_udl_files(cx: &Context, out: &mut Findings) {
    let Some(diff) = cx.diff else { return };
    for change in &diff.files {
        if change.status != Status::Added
            || !change.path.ends_with(".udl")
            || is_test_path(&change.path)
        {
            continue;
        }
        let Some(package) = cx.ws.package_of(&change.path) else {
            continue;
        };
        let had_udl = cx.ws.package_files(package).any(|f| {
            f.extension() == "udl"
                && f.path != change.path
                && diff.file(&f.path).is_none_or(|c| c.status != Status::Added)
        });
        if !had_udl {
            out.add(
                &change.path,
                1,
                "new UDL file: new UniFFI code uses proc-macros (`#[uniffi::export]`, `#[derive(uniffi::Record)]`) instead",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::{check, check_unified};

    const LOGINS: (&str, &str) = (
        "components/logins/Cargo.toml",
        "[package]\nname = \"logins\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const DB_CRYPTO: (&str, &str) = (
        "components/support/db-crypto/Cargo.toml",
        "[package]\nname = \"db-crypto\"\n[dependencies]\nuniffi = \"0.29\"\n",
    );
    const DB_CRYPTO_SRC: (&str, &str) = (
        "components/support/db-crypto/src/lib.rs",
        "#[uniffi::export(with_foreign)]\npub trait EncryptorDecryptor: Send + Sync {}\n",
    );
    const LOGINS_SRC: (&str, &str) = (
        "components/logins/src/encryption.rs",
        "/// A key.\n#[uniffi::export]\npub fn create_key() -> ApiResult<String> { todo!() }\n",
    );
    const UDL: &str = "components/logins/src/logins.udl";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                LOGINS,
                DB_CRYPTO,
                DB_CRYPTO_SRC,
                LOGINS_SRC,
                (
                    UDL,
                    include_str!("../../rules/uniffi-udl-proc-macro-consistency/dont.udl"),
                ),
            ],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![3, 7], "{found:#?}");
        assert!(found[1].message.contains("typedef trait_with_foreign"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                LOGINS,
                DB_CRYPTO,
                DB_CRYPTO_SRC,
                LOGINS_SRC,
                (
                    UDL,
                    include_str!("../../rules/uniffi-udl-proc-macro-consistency/do.udl"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_duplicate_types_and_methods() {
        let udl = "dictionary Login { string id; };\ninterface LoginStore {\n    void wipe_local();\n    void touch(string id);\n};\n";
        let src = "#[derive(uniffi::Record)]\npub struct Login { pub id: String }\n#[uniffi::export]\nimpl LoginStore {\n    pub fn wipe_local(&self) {}\n}\n";
        let found = check(
            &RULE,
            &[LOGINS, (UDL, udl), ("components/logins/src/store.rs", src)],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 3], "{found:#?}");
    }

    #[test]
    fn flags_constructors_outside_exported_impls() {
        let src = "impl Store {\n    #[uniffi::constructor]\n    pub fn new() -> Self { todo!() }\n}\n#[cfg_attr(feature = \"keydb\", uniffi::export)]\nimpl Other {\n    #[uniffi::constructor]\n    pub fn new() -> Self { todo!() }\n}\n";
        let found = check(&RULE, &[LOGINS, ("components/logins/src/store.rs", src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_typedefs_that_point_at_proc_macro_types() {
        let udl = "typedef dictionary RemoteTabRecord;\n[Custom]\ntypedef i64 Timestamp;\n";
        let src = "#[derive(uniffi::Record)]\npub struct RemoteTabRecord { pub url: String }\n";
        let found = check(
            &RULE,
            &[LOGINS, (UDL, udl), ("components/logins/src/tabs.rs", src)],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_new_udl_files_in_a_pr() {
        let diff = "--- /dev/null\n+++ b/components/relay/src/relay.udl\n@@ -0,0 +1 @@\n+namespace relay {};\n";
        let relay = (
            "components/relay/Cargo.toml",
            "[package]\nname = \"relay\"\n[dependencies]\nuniffi = \"0.29\"\n",
        );
        let found = check_unified(
            &RULE,
            &[
                relay,
                ("components/relay/src/relay.udl", "namespace relay {};\n"),
            ],
            diff,
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        let existing = check_unified(
            &RULE,
            &[
                relay,
                ("components/relay/src/relay.udl", "namespace relay {};\n"),
                ("components/relay/src/old.udl", "namespace old {};\n"),
            ],
            diff,
        );
        assert!(existing.is_empty(), "{existing:#?}");
    }
}
