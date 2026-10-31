//! Pin CI tools, images and actions (no `:latest`, current major of actions)
//!
//! Docs and examples: `rules/pin-ci-tools/`.

use std::collections::BTreeMap;

use crate::helpers::docs_deps::{hash_comment_start, is_out_of_scope, text_of};
use crate::rule::{Area, Context, Findings, Kind, Rule};

pub static RULE: Rule = Rule {
    id: "pin-ci-tools",
    area: Area::Scripts,
    title: "Pin CI tools, images and actions (no `:latest`, no deprecated action versions)",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Image tags that move.
const FLOATING_TAGS: &[&str] = &[
    "latest", "stable", "edge", "nightly", "current", "lts", "main", "master",
];

/// The oldest major of each action that still runs on a supported runtime
/// (older ones use Node 12/16 or were shut down).
const ACTION_MINIMUMS: &[(&str, u32)] = &[
    ("actions/checkout", 4),
    ("actions/upload-artifact", 4),
    ("actions/download-artifact", 4),
    ("actions/cache", 4),
    ("actions/setup-python", 5),
    ("actions/setup-java", 4),
    ("actions/setup-node", 4),
];

/// Archived actions.
const ARCHIVED_OWNERS: &[&str] = &["actions-rs/"];

/// Packaging bootstrap that is fine to take at its latest version.
const BOOTSTRAP: &[&str] = &["pip", "setuptools", "wheel", "six"];

/// Linters and formatters: a new release changes what CI accepts. Only these
/// are checked for `brew install`, which cannot pin versions anyway.
const LINTERS: &[&str] = &[
    "swiftformat",
    "swiftlint",
    "ktlint",
    "detekt",
    "shellcheck",
    "shfmt",
    "ruff",
    "black",
    "clang-format",
    "prettier",
];

fn check(cx: &Context, out: &mut Findings) {
    for (path, text) in ci_files(cx) {
        let lines = logical_lines(&text);
        let mut rustup_line = None;
        for (line, code) in &lines {
            if let Some(message) = floating_image(&path, code) {
                out.add(&path, *line, message);
            }
            if let Some(message) = old_action(code) {
                out.add(&path, *line, message);
            }
            for message in unpinned_installs(code) {
                out.add(&path, *line, message);
            }
            if code.contains("sh.rustup.rs") {
                rustup_line.get_or_insert(*line);
            }
        }
        let installs_toolchain = text.contains("rustup toolchain install")
            || text.contains("rustup show")
            || text.contains("rustup install");
        if let (Some(line), false) = (rustup_line, installs_toolchain) {
            out.add(
                &path,
                line,
                "installs rustup but never runs `rustup toolchain install`, so the build uses whatever stable is current instead of rust-toolchain.toml's",
            );
        }
    }
}

/// CI configs, CI scripts and Dockerfiles: `(path, text)`.
fn ci_files(cx: &Context) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for file in cx.ws.files() {
        let p = file.path.as_str();
        let ci_dir = p.starts_with(".circleci/")
            || p.starts_with(".github/")
            || p.starts_with("taskcluster/")
            || p.starts_with("automation/");
        let wanted = matches!(file.extension(), "yml" | "yaml" | "sh") && ci_dir
            || file.file_name().starts_with("Dockerfile");
        if wanted && !is_out_of_scope(p) {
            files.insert(p.to_string(), file.text.clone());
        }
    }
    // Dockerfiles have no extension, so the workspace does not load them.
    for dir in ["taskcluster/docker", "docker", ".devcontainer"] {
        let root = cx.ws.root.join(dir);
        for entry in walkdir::WalkDir::new(&root)
            .max_depth(4)
            .into_iter()
            .flatten()
        {
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with("Dockerfile")
            {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&cx.ws.root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if let Some(text) = text_of(cx.ws, &rel) {
                files.entry(rel).or_insert(text);
            }
        }
    }
    files
}

/// `(first line, code)` with `#` comments stripped and `\` continuations joined.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, raw) in text.lines().enumerate() {
        let code = match hash_comment_start(raw) {
            Some(at) => &raw[..at],
            None => raw,
        };
        let trimmed = code.trim();
        let (start, mut joined) = pending.take().unwrap_or((index + 1, String::new()));
        if let Some(cont) = trimmed.strip_suffix('\\') {
            joined.push_str(cont);
            joined.push(' ');
            pending = Some((start, joined));
            continue;
        }
        if trimmed.is_empty() && !joined.is_empty() {
            // A comment-only line inside a continued command.
            pending = Some((start, joined));
            continue;
        }
        joined.push_str(trimmed);
        out.push((start, joined));
    }
    if let Some(rest) = pending {
        out.push(rest);
    }
    out
}

/// `image: x:latest`, `image: x` (no tag), `FROM x:stable`.
fn floating_image(path: &str, code: &str) -> Option<String> {
    let code = code.trim_start_matches("- ").trim();
    let image = if path.rsplit('/').next()?.starts_with("Dockerfile") {
        let rest = code.strip_prefix("FROM ")?;
        rest.split_whitespace()
            .find(|w| !w.starts_with("--"))?
            .to_string()
    } else {
        code.strip_prefix("image:")?
            .trim()
            .trim_matches(['"', '\''])
            .to_string()
    };
    if image.is_empty() || image.contains('$') || image.contains('{') || image == "scratch" {
        return None;
    }
    if image.contains("@sha256:") {
        return None;
    }
    let name = image.rsplit('/').next().unwrap_or(&image);
    match name.split_once(':') {
        None => Some(format!(
            "`{image}` has no tag, so it means `:latest`: pin a version (or a digest)"
        )),
        Some((_, tag)) if FLOATING_TAGS.contains(&tag) => Some(format!(
            "`{image}` is a floating tag that can change under CI: pin a version (or a digest)"
        )),
        _ => None,
    }
}

/// `uses: actions/checkout@v3`, `uses: actions-rs/toolchain@v1`.
fn old_action(code: &str) -> Option<String> {
    let code = code.trim_start_matches("- ").trim();
    let spec = code.strip_prefix("uses:")?.trim().trim_matches(['"', '\'']);
    let (action, version) = spec.split_once('@')?;
    if ARCHIVED_OWNERS.iter().any(|o| action.starts_with(o)) {
        return Some(format!(
            "`{action}` is archived and unmaintained: use a maintained action (e.g. run `rustup` directly)"
        ));
    }
    let (_, minimum) = ACTION_MINIMUMS.iter().find(|(a, _)| *a == action)?;
    let major: u32 = version
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()?;
    (major < *minimum).then(|| {
        format!("`{action}@{version}` is an old major on a retired Node runtime: move to the current major (at least `@v{minimum}`)")
    })
}

/// Packages installed at whatever version is latest.
fn unpinned_installs(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for command in code.split(['&', ';', '|']) {
        let words: Vec<&str> = command.split_whitespace().collect();
        let Some(at) = words.iter().position(|w| *w == "install" || *w == "i") else {
            continue;
        };
        let tool = words[..at]
            .iter()
            .rev()
            .find(|w| !w.starts_with('-'))
            .map(|w| w.rsplit('/').next().unwrap_or(w))
            .unwrap_or("");
        let args = &words[at + 1..];
        let unpinned: Vec<&str> = match tool {
            "pip" | "pip3" => pip_unpinned(args),
            "cargo" => cargo_unpinned(args),
            "gem" => gem_unpinned(args),
            "npm" if args.iter().any(|a| *a == "-g" || *a == "--global") => args
                .iter()
                .filter(|a| !a.starts_with('-') && !a[1..].contains('@'))
                .copied()
                .collect(),
            "brew" => args
                .iter()
                .filter(|a| LINTERS.contains(a))
                .copied()
                .collect(),
            _ => Vec::new(),
        };
        if unpinned.is_empty() {
            continue;
        }
        let how = match tool {
            "pip" | "pip3" => "`name==x.y.z`, or a hashed requirements file",
            "cargo" => "`--version x.y.z`",
            "gem" => "`-v x.y.z`",
            "npm" => "`name@x.y.z`",
            _ => "a versioned download or a pinned formula",
        };
        out.push(format!(
            "`{tool} install {}` takes the latest release, which can break CI without a commit: pin it ({how})",
            unpinned.join(" ")
        ));
    }
    out
}

/// pip flags that take a value.
const PIP_VALUE_FLAGS: &[&str] = &[
    "-c",
    "--constraint",
    "-t",
    "--target",
    "-i",
    "--index-url",
    "--extra-index-url",
    "--prefix",
    "--root",
    "--platform",
    "--python-version",
];

fn pip_unpinned<'a>(args: &[&'a str]) -> Vec<&'a str> {
    if args.iter().any(|a| {
        matches!(
            *a,
            "-r" | "--requirement" | "-e" | "--editable" | "--require-hashes"
        ) || a.starts_with("--requirement=")
    }) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if arg.starts_with('-') {
            skip = PIP_VALUE_FLAGS.contains(arg);
            continue;
        }
        let local = arg.starts_with(['.', '/', '~', '$', '"', '\''])
            || arg.contains("://")
            || arg.ends_with(".whl")
            || arg.ends_with(".tar.gz");
        let pinned = arg.contains("==") || arg.contains('@');
        let name = arg
            .split(['[', '<', '>', '=', '~', '!'])
            .next()
            .unwrap_or(arg);
        if !local && !pinned && !BOOTSTRAP.contains(&name.to_lowercase().as_str()) {
            out.push(*arg);
        }
    }
    out
}

fn cargo_unpinned<'a>(args: &[&'a str]) -> Vec<&'a str> {
    if args.iter().any(|a| {
        matches!(*a, "--version" | "--vers" | "--path" | "--rev" | "--tag")
            || a.starts_with("--version=")
    }) {
        return Vec::new();
    }
    let value_flags = [
        "--root",
        "--git",
        "--branch",
        "--features",
        "-F",
        "--target",
        "-j",
        "--jobs",
        "--index",
        "--registry",
        "--profile",
        "--bin",
        "--example",
    ];
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if arg.starts_with('-') {
            skip = value_flags.contains(arg);
            continue;
        }
        if !arg.contains('@') {
            out.push(*arg);
        }
    }
    out
}

fn gem_unpinned<'a>(args: &[&'a str]) -> Vec<&'a str> {
    if args
        .iter()
        .any(|a| matches!(*a, "-v" | "--version") || a.starts_with("--version="))
    {
        return Vec::new();
    }
    let value_flags = ["--bindir", "-n", "--install-dir", "-i", "--source", "-s"];
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if arg.starts_with('-') {
            skip = value_flags.contains(arg);
            continue;
        }
        if !arg.contains(':') {
            out.push(*arg);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RULE;
    use crate::testing::check;

    const PATH: &str = ".circleci/config.yml";

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/pin-ci-tools/dont.yaml"))],
        );
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!((found[0].line, found[1].line), (4, 13));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/pin-ci-tools/do.yaml"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn actions() {
        let text = "steps:\n  - uses: actions/checkout@v3\n  - uses: actions/checkout@v4\n  - uses: actions-rs/toolchain@v1\n  - uses: mozilla/probe-scraper/.github/workflows/glean.yaml@main\n  - uses: ruby/setup-ruby@v1\n";
        let found = check(&RULE, &[(".github/workflows/a.yml", text)]);
        assert_eq!(found.len(), 2, "{found:#?}");
    }

    #[test]
    fn installs() {
        let text = "run: |\n  pip install --upgrade pip setuptools six\n  pip install --require-hashes -r requirements.txt\n  pip install .\n  pip install pytest==8.3.3\n  cargo install cargo-audit --locked\n  cargo install mdbook@0.4.40\n  gem install mustache -v 1.1.2 && gem install jazzy\n  brew install ninja\n  brew install swiftformat\n";
        let found = check(&RULE, &[(".github/workflows/a.yml", text)]);
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 3, "{messages:#?}");
        assert!(messages[0].contains("cargo install cargo-audit"));
        assert!(messages[1].contains("gem install jazzy"));
        assert!(messages[2].contains("brew install swiftformat"));
    }

    #[test]
    fn dockerfiles_and_rustup() {
        let docker = "FROM ubuntu:22.04\nRUN pip3 install \\\n    # for taskgraph\n    taskcluster \\\n    pyyaml==6.0.2\n";
        let rustup = "run: |\n  curl https://sh.rustup.rs | sh -s -- -y\n  cargo build\n";
        let found = check(
            &RULE,
            &[
                ("taskcluster/docker/linux/Dockerfile", docker),
                (".github/workflows/b.yml", rustup),
            ],
        );
        let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 2, "{messages:#?}");
        assert!(messages[0].contains("rustup toolchain install"));
        assert!(messages[1].contains("pip3 install taskcluster"));
    }
}
