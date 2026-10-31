//! Every PR compiles all targets that a change can break (wrappers, examples, tests, taskgraph)
//!
//! Docs and examples: `rules/compile-everything-on-pr/`.

use crate::helpers::docs_deps::{hash_comment_start, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Kind, Rule};
use crate::source::TextFile;

pub static RULE: Rule = Rule {
    id: "compile-everything-on-pr",
    area: Area::Deps,
    title: "Every PR compiles all targets that a change can break (wrappers, examples, tests, taskgraph)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

const PARAMS_DIR: &str = "taskcluster/test/params/";

fn check(cx: &Context, out: &mut Findings) {
    kotlin_on_every_pr(cx, out);
    kotlin_warnings(cx, out);
    taskgraph_params(cx, out);
}

/// Taskcluster kinds that run a Gradle build of this repo's modules.
fn builds_kotlin(kind: &TextFile) -> bool {
    kind.text.contains("gradlew")
        && ["assemble", "testDebug", "testRelease", "compile"]
            .iter()
            .any(|t| kind.text.contains(t))
        && !kind.text.contains("branch-build:")
}

/// A YAML line without its `#` comment, trimmed.
fn yaml_code(line: &str) -> &str {
    match hash_comment_start(line) {
        Some(at) => line[..at].trim(),
        None => line.trim(),
    }
}

/// Whether a kind's tasks run on ordinary pull requests: they list
/// `github-pull-request` and are not limited to full-CI runs.
fn runs_on_every_pr(kind: &TextFile) -> bool {
    let for_prs = kind
        .lines()
        .any(|(_, l)| l.contains("run-on-tasks-for") && l.contains("github-pull-request"));
    let pr_types: Vec<&str> = kind
        .lines()
        .filter_map(|(_, l)| yaml_code(l).strip_prefix("run-on-pr-type:"))
        .map(str::trim)
        .collect();
    for_prs && (pr_types.is_empty() || pr_types.iter().any(|t| *t != "full-ci"))
}

/// If every task that compiles the Kotlin modules is full-CI only, a normal
/// PR lands Kotlin that was never compiled.
fn kotlin_on_every_pr(cx: &Context, out: &mut Findings) {
    let kinds: Vec<&TextFile> = cx
        .ws
        .files_with_ext(&["yml", "yaml"])
        .filter(|f| f.path.starts_with("taskcluster/kinds/") && builds_kotlin(f))
        .collect();
    if kinds.is_empty() || kinds.iter().any(|k| runs_on_every_pr(k)) {
        return;
    }
    for kind in kinds {
        let for_prs = kind
            .lines()
            .any(|(_, l)| l.contains("run-on-tasks-for") && l.contains("github-pull-request"));
        if !for_prs {
            continue;
        }
        if let Some((line, _)) = kind
            .lines()
            .find(|(_, l)| yaml_code(l) == "run-on-pr-type: full-ci")
        {
            out.add(
                &kind.path,
                line,
                "no task compiles the Kotlin modules on ordinary PRs, so Kotlin that doesn't build can land: run this build (or a cheaper `assembleDebug`) with `run-on-pr-type: all`",
            );
        }
    }
}

/// Kotlin warnings are errors in this repo; a module must not opt out.
fn kotlin_warnings(cx: &Context, out: &mut Findings) {
    for file in cx.ws.files_with_ext(&["gradle", "kts"]) {
        if is_out_of_scope(&file.path) {
            continue;
        }
        for (line, text) in file.lines() {
            let t = text.split("//").next().unwrap_or("");
            if t.contains("allWarningsAsErrors") && t.contains("false") {
                out.add(
                    &file.path,
                    line,
                    "turns off Kotlin's warnings-as-errors, so wrapper warnings reach consumer apps: fix the warnings instead",
                );
            }
        }
    }
}

/// Parameter sets for taskgraph that no CI job ever generates a graph for.
fn taskgraph_params(cx: &Context, out: &mut Findings) {
    let params: Vec<&TextFile> = cx
        .ws
        .files_with_ext(&["yml", "yaml"])
        .filter(|f| f.path.starts_with(PARAMS_DIR))
        .collect();
    let Some(first) = params.first() else {
        return;
    };
    let used = cx.ws.files().any(|f| {
        let p = f.path.as_str();
        let ci = p == ".taskcluster.yml"
            || p.starts_with(".circleci/")
            || p.starts_with(".github/")
            || p.starts_with("automation/")
            || (p.starts_with("taskcluster/") && !p.starts_with(PARAMS_DIR));
        ci && f.text.contains("test/params")
    });
    if !used {
        out.add(
            &first.path,
            0,
            format!(
                "no CI job generates the taskgraph for the {} parameter sets in {PARAMS_DIR}, so a transform change can break the nightly or release graph unnoticed: run `taskgraph full -p` over them on PRs that touch taskcluster/",
                params.len()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const KIND: &str = "taskcluster/kinds/android-build/kind.yml";
    const MODULES: (&str, &str) = (
        "taskcluster/kinds/module-build/kind.yml",
        "task-defaults:\n  attributes:\n    run-on-pr-type: full-ci\n  run-on-tasks-for: [github-pull-request, github-push]\n  run:\n    gradlew:\n      - ':{module_name}:assembleRelease'\n",
    );

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[
                MODULES,
                (
                    KIND,
                    include_str!("../../rules/compile-everything-on-pr/dont.yml"),
                ),
            ],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!((found[0].path.as_str(), found[0].line), (KIND, 4));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[
                MODULES,
                (
                    KIND,
                    include_str!("../../rules/compile-everything-on-pr/do.yml"),
                ),
            ],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn kotlin_warnings_opt_out() {
        let gradle = "android {\n  kotlinOptions {\n    allWarningsAsErrors = false\n  }\n}\n// allWarningsAsErrors = false in a comment\n";
        let found = check(&RULE, &[("components/x/android/build.gradle", gradle)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn taskgraph_params_need_a_ci_job() {
        let params = ("taskcluster/test/params/pull-request.yml", "level: '1'\n");
        assert_eq!(check(&RULE, &[params]).len(), 1);
        let job = (
            ".github/workflows/taskgraph.yml",
            "run: taskgraph full -p taskcluster/test/params\n",
        );
        assert!(check(&RULE, &[params, job]).is_empty());
    }
}
