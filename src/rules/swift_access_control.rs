//! Swift: explicit access control for consumer-facing API
//!
//! Docs and examples: `rules/swift-access-control/`.

use crate::helpers::api_mobile::{shipped_mobile_files, Code, Lang};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "swift-access-control",
    area: Area::Mobile,
    title: "Swift: explicit access control for consumer-facing API",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const ACCESS: &[&str] = &["public", "open", "internal", "fileprivate", "private"];
const MODIFIERS: &[&str] = &[
    "final",
    "static",
    "override",
    "convenience",
    "required",
    "lazy",
    "weak",
    "unowned",
    "mutating",
    "nonmutating",
    "dynamic",
    "indirect",
    "nonisolated",
];
const TYPES: &[&str] = &["class", "struct", "enum", "protocol", "extension", "actor"];

fn check(cx: &Context, out: &mut Findings) {
    for (path, lang, code) in shipped_mobile_files(cx.ws) {
        if lang != Lang::Swift {
            continue;
        }
        for (line, message) in scan(&code) {
            out.add(&path, line, message);
        }
    }
}

/// A declaration at the start of a line.
struct Decl {
    keyword: &'static str,
    name: String,
    access: Option<String>,
    /// `extension Foo: Proto` (which can't take an access modifier).
    conformance: bool,
}

/// A type body we are inside of.
struct Frame {
    /// Brace depth of the body's direct members.
    depth: usize,
    /// A `public`/`open` class, struct, enum or actor: its members are API.
    public_type: bool,
    name: String,
}

fn scan(code: &Code) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut frames: Vec<Frame> = Vec::new();
    // A type declaration waiting for its `{`: (depth it was declared at, frame).
    let mut pending: Option<(usize, Frame)> = None;
    for (line_no, line) in code.lines() {
        if let Some(decl) = declaration(line) {
            if depth == 0 {
                let exempt = decl.keyword == "extension" && decl.conformance;
                if decl.access.is_none() && !exempt {
                    out.push((
                        line_no,
                        format!(
                            "`{} {}` has no access modifier, so it is `internal` and the app can't see it; mark it `public`, or say `internal`/`private` explicitly",
                            decl.keyword, decl.name
                        ),
                    ));
                }
            } else if let Some(frame) = frames.last() {
                let member = matches!(decl.keyword, "init" | "func" | "var" | "let" | "subscript");
                if depth == frame.depth && frame.public_type && member && decl.access.is_none() {
                    out.push((
                        line_no,
                        format!(
                            "`{}` in public `{}` has no access modifier, so the app can't use it; mark it `public`, or say `internal`/`private` explicitly",
                            if decl.name.is_empty() { decl.keyword.to_string() } else { format!("{} {}", decl.keyword, decl.name) },
                            frame.name
                        ),
                    ));
                }
            }
            if TYPES.contains(&decl.keyword) {
                let public = matches!(decl.access.as_deref(), Some("public" | "open"));
                let public_type = public && !matches!(decl.keyword, "extension" | "protocol");
                pending = Some((
                    depth,
                    Frame {
                        depth: depth + 1,
                        public_type,
                        name: decl.name,
                    },
                ));
            }
        }
        for b in line.bytes() {
            match b {
                b'{' => {
                    if let Some((at, frame)) = pending.take() {
                        if at == depth {
                            frames.push(frame);
                        }
                    }
                    depth += 1;
                }
                b'}' => {
                    depth = depth.saturating_sub(1);
                    while frames.last().is_some_and(|f| f.depth > depth) {
                        frames.pop();
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Parse `@objc public final class Foo: Bar {` into a [`Decl`].
fn declaration(line: &str) -> Option<Decl> {
    let mut rest = line.trim_start();
    let mut access = None;
    loop {
        if let Some(attr) = rest.strip_prefix('@') {
            // `@objc`, `@available(...)`, `@MainActor`.
            let end = attr
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(attr.len());
            rest = &attr[end..];
            if rest.starts_with('(') {
                let close = rest.find(')')?;
                rest = &rest[close + 1..];
            }
            rest = rest.trim_start();
            continue;
        }
        let word_end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let word = &rest[..word_end];
        if word.is_empty() {
            return None;
        }
        let after = &rest[word_end..];
        if ACCESS.contains(&word) {
            // `private(set)` only narrows the setter.
            if let Some(set) = after.strip_prefix("(set)") {
                rest = set.trim_start();
                continue;
            }
            access = Some(word.to_string());
            rest = after.trim_start();
            continue;
        }
        let next_word = after
            .trim_start()
            .split(|c: char| !c.is_alphanumeric())
            .next();
        if word == "class" && matches!(next_word, Some("func" | "var" | "let")) {
            rest = after.trim_start(); // `class func` is a modifier
            continue;
        }
        if MODIFIERS.contains(&word) {
            rest = after.trim_start();
            continue;
        }
        let keyword = TYPES
            .iter()
            .chain(&["func", "init", "var", "let", "typealias", "subscript"])
            .find(|k| **k == word)?;
        let tail = after.trim_start();
        let name: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        let conformance =
            *keyword == "extension" && tail[name.len()..].trim_start().starts_with(':');
        return Some(Decl {
            keyword,
            name,
            access,
            conformance,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str =
        "components/support/nimbus-fml/src/backends/swift/templates/FeatureManifestTemplate.swift";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/swift-access-control/dont.swift"),
            )],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 5, 10], "{found:#?}");
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(
                PATH,
                include_str!("../../rules/swift-access-control/do.swift"),
            )],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn ignores_bodies_nested_types_and_conformances() {
        let src = r#"
public class A {
    private let x: Int
    struct Defaults { let y: Int }
    enum CodingKeys: String, CodingKey { case y }
    public func f() {
        let local = 1
        var other = { let z = 2 }
    }
}
extension A: Equatable {
    public static func == (l: A, r: A) -> Bool { true }
}
public extension A {
    func g() {}
}
class Internal {
    func h() {}
}
"#;
        let found = check(&RULE, &[(PATH, src)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![17], "{found:#?}");
    }

    #[test]
    fn reads_templates() {
        let src = "{%- macro render(inner) %}\npublic class {{ name }} {\n    init() {}\n}\n{%- endmacro %}}\n";
        let found = check(&RULE, &[(PATH, src)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn ignores_swift_tests() {
        let path = "components/nimbus/ios/Tests/NimbusTests.swift";
        assert!(check(&RULE, &[(path, "class T {}\n")]).is_empty());
    }
}
