# varlope

An experimental linter for [mozilla/application-services](https://github.com/mozilla/application-services).
Each of its 91 rules is something reviewers kept asking for in PR reviews between 2022 and 2026. Every rule
links to the review comments it came from and lists what it finds in application-services today.

**Rules site: https://almaju.github.io/varlope/** (rebuilt on every push to `main`; locally:
`cargo run --release -- site --report report/findings.json --out site`).

## Use

```sh
cargo install --git https://github.com/Almaju/varlope
cd application-services
varlope check                      # rules that read the code
varlope check --base origin/main   # plus the PR rules: CHANGELOG, API breaks, migrations…
varlope check --clippy             # plus the rules clippy implements (builds the workspace)
varlope check --path components/ads-client --fix   # apply the suggested fixes there
varlope rules                      # list the rules
```

`--clippy` runs `cargo clippy` on the workspace, so `NSS_DIR` must point at NSS headers.

An exception is written where it happens, with a reason:

```rust
let re = Regex::new(PATTERN).unwrap(); // varlope: allow(no-unwrap-in-component-code) literal, covered by tests
```

## How it is built

- The rules come from reading every review thread on application-services PRs since 2022 (4,333 threads).
  The research is in [review-insights](https://github.com/Almaju/application-services/tree/claude/elegant-euler-vnouf3/tools/review-insights).
- One rule per file in `src/rules/`. Its docs are in `rules/<id>/`: `why.md`, and Don't/Do examples that are
  also the rule's tests. The review comments behind it are in `research/rules.json`.
- `report/findings.json` is a run on application-services `main` (native rules, clippy, and the PR rules
  replayed over the last 300 merged commits). The site is built from it.
- Dependencies are limited to crates already in application-services' `Cargo.lock`, at the same versions
  (`scripts/check-deps.sh`, run in CI).
- The comment scanner and test-code detection are adapted from [rabot](https://github.com/Almaju/rabot).

## Status

An experiment. Precision matters more than coverage: a noisy rule should be fixed or dropped. Feedback welcome.
