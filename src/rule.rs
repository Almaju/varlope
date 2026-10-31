//! What every rule declares, and what it reports.

use serde::Serialize;

use crate::diff::Diff;
use crate::workspace::Workspace;

/// The part of the codebase a rule is about. Used to group rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Area {
    Uniffi,
    Errors,
    Sql,
    Remote,
    Concurrency,
    Privacy,
    Docs,
    DeadCode,
    Idioms,
    Api,
    Deps,
    Tests,
    Scripts,
    Mobile,
    NimbusGlean,
}

impl Area {
    pub const ALL: [Area; 15] = [
        Area::Uniffi,
        Area::Errors,
        Area::Sql,
        Area::Remote,
        Area::Concurrency,
        Area::Privacy,
        Area::Docs,
        Area::DeadCode,
        Area::Idioms,
        Area::Api,
        Area::Deps,
        Area::Tests,
        Area::Scripts,
        Area::Mobile,
        Area::NimbusGlean,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Area::Uniffi => "UniFFI & bindings",
            Area::Errors => "Errors & panics",
            Area::Sql => "SQL & storage",
            Area::Remote => "Remote data",
            Area::Concurrency => "Concurrency",
            Area::Privacy => "Logging & privacy",
            Area::Docs => "Docs & CHANGELOG",
            Area::DeadCode => "Dead code",
            Area::Idioms => "Rust idioms",
            Area::Api => "API design & naming",
            Area::Deps => "Dependencies & build",
            Area::Tests => "Tests",
            Area::Scripts => "Scripts & CI",
            Area::Mobile => "Kotlin & Swift",
            Area::NimbusGlean => "Nimbus & Glean",
        }
    }
}

/// How a rule is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// varlope reads the code as it is.
    Code,
    /// varlope reads a change: needs `--base` (a PR diff).
    Diff,
    /// The rule is mostly clippy lints, run by `varlope check --clippy`.
    Clippy,
    /// Heuristics that point a reviewer at something; a human decides.
    Review,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Code => "varlope, on the code",
            Kind::Diff => "varlope, on the PR diff",
            Kind::Clippy => "clippy lints (via varlope)",
            Kind::Review => "varlope hints, for review",
        }
    }
}

/// One rule. Each lives in its own file under `src/rules/`.
pub struct Rule {
    /// Kebab-case id, also the name of its docs folder under `rules/`.
    pub id: &'static str,
    pub area: Area,
    /// One line, imperative.
    pub title: &'static str,
    pub kind: Kind,
    /// Clippy lints that implement (part of) this rule.
    pub clippy: &'static [&'static str],
    /// Report findings inside test code too. Most rules don't.
    pub in_tests: bool,
    pub check: fn(&Context, &mut Findings),
}

/// What a rule can look at.
pub struct Context<'a> {
    pub ws: &'a Workspace,
    /// The change under review, when run with `--base`.
    pub diff: Option<&'a Diff>,
}

/// Where a finding came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Varlope,
    Clippy,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    /// Path relative to the workspace root, with `/` separators.
    pub path: String,
    /// 1-based. 0 when the finding is about a whole file.
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub origin: Origin,
    /// The commit a diff finding was found in (replay only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// The findings of one rule. Rules call [`Findings::add`]; the runner sets
/// the rule id and applies allow comments and test-code filtering.
pub struct Findings {
    rule: &'static str,
    pub(crate) items: Vec<Finding>,
}

impl Findings {
    pub fn new(rule: &'static str) -> Self {
        Self {
            rule,
            items: Vec::new(),
        }
    }

    /// Report `message` at `path:line` (1-based; 0 for the whole file).
    pub fn add(&mut self, path: impl Into<String>, line: usize, message: impl Into<String>) {
        self.add_at(path, line, 1, message);
    }

    pub fn add_at(
        &mut self,
        path: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) {
        self.items.push(Finding {
            rule: self.rule,
            path: path.into(),
            line,
            column,
            message: message.into(),
            origin: Origin::Varlope,
            commit: None,
        });
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn into_vec(self) -> Vec<Finding> {
        self.items
    }

    pub fn iter(&self) -> impl Iterator<Item = &Finding> {
        self.items.iter()
    }
}
