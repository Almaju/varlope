//! No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`
//!
//! Docs and examples: `rules/expect-with-reason/`.

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::helpers::idioms;
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::RustFile;

pub static RULE: Rule = Rule {
    id: "expect-with-reason",
    area: Area::DeadCode,
    title: "No blanket `#[allow(dead_code|unused)]`; use `#[cfg]` or `#[expect(…, reason = …)]`",
    kind: Kind::Code,
    clippy: &["clippy::allow_attributes_without_reason"],
    in_tests: false,
    check,
};

fn check(cx: &Context, out: &mut Findings) {
    for file in idioms::rust_files(cx.ws).filter(|f| !idioms::is_tooling(&f.path)) {
        let mut visitor = Attrs { hits: Vec::new() };
        visitor.visit_file(&file.ast);
        for hit in visitor.hits {
            let (line, column) = file.pos(hit.attr);
            out.add_at(&file.path, line, column, &hit.message);
            if let (Some(fix), Some(last)) = (fix(file, &hit), out.items.last_mut()) {
                last.fix = Some(fix);
            }
        }
    }
}

struct Hit<'a> {
    attr: &'a syn::Attribute,
    message: String,
    /// `allow` or `expect`.
    level: &'static str,
    lints: Vec<String>,
}

struct Attrs<'a> {
    hits: Vec<Hit<'a>>,
}

impl<'a> Visit<'a> for Attrs<'a> {
    fn visit_attribute(&mut self, attr: &'a syn::Attribute) {
        let name = if attr.path().is_ident("allow") {
            "allow"
        } else if attr.path().is_ident("expect") {
            "expect"
        } else {
            return;
        };
        let mut lints = Vec::new();
        let mut reason = false;
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("reason") {
                reason = true;
                let _ = meta.value()?.parse::<syn::Expr>()?;
            } else {
                lints.push(crate::syntax::path_string(&meta.path));
            }
            Ok(())
        });
        if parsed.is_err() || reason {
            return;
        }
        let bang = if matches!(attr.style, syn::AttrStyle::Inner(_)) {
            "!"
        } else {
            ""
        };
        if name == "expect" {
            self.hits.push(Hit {
                attr,
                message: format!(
                    "`#{bang}[expect({})]` has no `reason = \"…\"`; say why the lint is expected here",
                    lints.join(", ")
                ),
                level: name,
                lints,
            });
            return;
        }
        let Some(lint) = lints.iter().find(|l| silences_unused(l)) else {
            return;
        };
        self.hits.push(Hit {
            attr,
            message: format!(
                "`#{bang}[allow({lint})]` hides unused code for good; delete it, gate it with `#[cfg(..)]`, or write `#[expect({lint}, reason = \"…\")]`"
            ),
            level: name,
            lints,
        });
    }
}

/// The attribute rewritten with a reason: `#[allow(dead_code)]` becomes
/// `#[expect(dead_code, reason = "TODO: why")]`, an `expect` keeps its lints.
/// A comment after the attribute is its reason; one that says the code is
/// used in some builds (a feature, a platform) keeps `allow`, since an
/// `expect` would fail there. No fix for an `allow` that also names other
/// lints (they may be needed), nor for an attribute over several lines.
fn fix(file: &RustFile, hit: &Hit) -> Option<Fix> {
    if hit.level == "allow" && !hit.lints.iter().all(|l| silences_unused(l)) {
        return None;
    }
    let (line, _) = file.pos(hit.attr);
    let range = file.range(hit.attr.span());
    let text = file.line(line);
    let line_start = file.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let (start, end) = (range.start - line_start, range.end - line_start);
    if end > text.len() || text.get(start..end) != Some(file.text_of(hit.attr)) {
        return None; // spans lines
    }
    let (before, after) = (&text[..start], &text[end..]);
    let comment = after
        .trim()
        .strip_prefix("//")
        .map(|c| c.trim().trim_end_matches('.').trim())
        .filter(|c| !c.is_empty() && !c.starts_with('/') && !c.starts_with('!'));
    let (level, reason) = match comment {
        Some(c) => {
            let some_builds = ["feature", "cfg", "platform", "target", "test"]
                .iter()
                .any(|w| c.to_lowercase().contains(w));
            let level = if some_builds && hit.level == "allow" {
                "allow"
            } else {
                "expect"
            };
            (level, c.replace('\\', "\\\\").replace('"', "\\\""))
        }
        None => ("expect", "TODO: why".to_string()),
    };
    let bang = if matches!(hit.attr.style, syn::AttrStyle::Inner(_)) {
        "!"
    } else {
        ""
    };
    let args = format!("{}, reason = \"{reason}\"", hit.lints.join(", "));
    let attr = format!("#{bang}[{level}({args})]");
    // The comment moved into the attribute.
    let after = if comment.is_some() { "" } else { after };
    let replaced = format!("{before}{attr}{after}");
    // rustfmt's `max_width` and `attr_fn_like_width`.
    if replaced.chars().count() <= 100 && args.chars().count() <= 70 {
        return Some(Fix::replace_line(line, replaced));
    }
    // rustfmt puts a long attribute's arguments on lines of their own.
    if !before.trim().is_empty() || !after.trim().is_empty() {
        return None;
    }
    let mut lines = vec![format!("{before}#{bang}[{level}(")];
    lines.extend(hit.lints.iter().map(|l| format!("{before}    {l},")));
    lines.push(format!("{before}    reason = \"{reason}\""));
    lines.push(format!("{before})]"));
    Some(Fix::edit(line, 1, lines))
}

/// `dead_code`, `unused` and the `unused_*` lints.
fn silences_unused(lint: &str) -> bool {
    lint == "dead_code" || lint == "unused" || lint.starts_with("unused_")
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = "components/ads-client/src/http_cache/request_hash.rs";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/expect-with-reason/dont.rs"))],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/expect-with-reason/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn flags_inner_allows_and_expect_without_reason() {
        let src = "#![allow(unused_imports)]\n#[expect(clippy::too_many_arguments)]\nfn f() {}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].message.contains("#![allow(unused_imports)]"));
        assert!(found[1].message.contains("no `reason"));
    }

    #[test]
    fn fixes_with_a_reason_to_fill_in() {
        let src = "#![allow(unused_imports)]\n\
                   #[expect(clippy::too_many_arguments)]\n\
                   fn f() {}\n\
                   impl S {\n    #[allow(dead_code)]\n    fn g() {}\n}\n\
                   #[allow(unused_imports)] // only used with some features.\n\
                   use a::b;\n\
                   #[allow(dead_code)] // kept for the v2 migration\n\
                   fn h() {}\n\
                   #[allow(dead_code, clippy::large_enum_variant)]\n\
                   enum E { A }\n";
        let found = check(&RULE, &[(PATH, src)]);
        let fixes: Vec<(usize, Option<Vec<String>>)> = found
            .iter()
            .map(|f| {
                let fix = f.fix.as_ref().map(|fix| {
                    assert_eq!((fix.edits[0].line, fix.edits[0].remove), (f.line, 1));
                    fix.edits[0].insert.clone()
                });
                (f.line, fix)
            })
            .collect();
        let line = |s: &str| Some(vec![s.to_string()]);
        assert_eq!(
            fixes,
            vec![
                (
                    1,
                    line("#![expect(unused_imports, reason = \"TODO: why\")]")
                ),
                (
                    2,
                    line("#[expect(clippy::too_many_arguments, reason = \"TODO: why\")]")
                ),
                (5, line("    #[expect(dead_code, reason = \"TODO: why\")]")),
                // The comment is the reason; an `expect` would fail with the feature on.
                (
                    8,
                    line("#[allow(unused_imports, reason = \"only used with some features\")]")
                ),
                (
                    10,
                    line("#[expect(dead_code, reason = \"kept for the v2 migration\")]")
                ),
                // `clippy::large_enum_variant` may still be needed.
                (12, None),
            ],
            "{found:#?}"
        );
    }

    #[test]
    fn wraps_a_long_attribute_like_rustfmt() {
        let src = "mod m {\n    #[allow(dead_code)] // read by the sync engine once the v2 schema ships everywhere\n    fn f() {}\n}\n";
        let found = check(&RULE, &[(PATH, src)]);
        let edit = &found[0].fix.as_ref().expect("a fix").edits[0];
        assert_eq!((edit.line, edit.remove), (2, 1));
        assert_eq!(
            edit.insert,
            vec![
                "    #[expect(",
                "        dead_code,",
                "        reason = \"read by the sync engine once the v2 schema ships everywhere\"",
                "    )]",
            ]
        );
    }

    #[test]
    fn ignores_other_lints_reasons_conditional_allows_and_tests() {
        let src = "#[allow(clippy::large_enum_variant)]\nenum E { A }\n\
                   #[allow(dead_code, reason = \"read by the FFI\")]\nstruct S;\n\
                   #[cfg_attr(not(feature = \"x\"), allow(dead_code))]\nfn g() {}\n\
                   #[cfg(test)]\nmod tests {\n#[allow(dead_code)]\nfn h() {}\n}\n";
        assert!(check(&RULE, &[(PATH, src)]).is_empty());
    }
}
