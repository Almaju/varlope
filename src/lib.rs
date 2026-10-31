//! varlope: an experimental linter for mozilla/application-services.
//!
//! Every rule comes from something reviewers kept asking for in PR reviews.
//! See `rules/<id>/` for each rule's examples and `research/` for where it
//! came from.

pub mod allowance;
pub mod clippy;
pub mod comment;
pub mod diff;
pub mod helpers;
pub mod highlight;
pub mod replay;
pub mod report;
pub mod rule;
pub mod rules;
pub mod run;
pub mod site;
pub mod source;
pub mod syntax;
pub mod test_regions;
pub mod testing;
pub mod udl;
pub mod uniffi;
pub mod workspace;

pub use rule::{Area, Context, Edit, Finding, Findings, Fix, Kind, Rule};

/// Every rule, grouped by area.
pub fn rules() -> &'static [&'static Rule] {
    rules::ALL
}

pub fn rule(id: &str) -> Option<&'static Rule> {
    rules::ALL.iter().copied().find(|r| r.id == id)
}
