//! Every rule has a docs folder with a reason and a Don't/Do pair.

use std::path::Path;

#[test]
fn every_rule_is_documented() {
    let mut ids = std::collections::BTreeSet::new();
    for rule in varlope::rules() {
        assert!(ids.insert(rule.id), "duplicate rule id {}", rule.id);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("rules")
            .join(rule.id);
        let why = std::fs::read_to_string(dir.join("why.md")).unwrap_or_default();
        assert!(
            !why.trim().is_empty(),
            "{}: rules/{}/why.md is missing or empty",
            rule.id,
            rule.id
        );
        for part in ["dont", "do"] {
            let found = std::fs::read_dir(&dir)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with(&format!("{part}."))
                    })
                })
                .unwrap_or(false);
            assert!(found, "{}: rules/{}/{part}.* is missing", rule.id, rule.id);
        }
    }
    assert_eq!(ids.len(), 91);
}
