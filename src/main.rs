use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use varlope::diff::Diff;
use varlope::report::{Report, RuleInfo};
use varlope::rule::{Finding, Rule};
use varlope::workspace::Workspace;

/// An experimental linter for mozilla/application-services, built from what
/// its reviewers kept asking for.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The checkout to lint.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lint the checkout. Exits 1 when anything is found.
    Check {
        /// Only these rules (repeatable).
        #[arg(long = "rule")]
        rules: Vec<String>,
        /// Also run the PR-diff rules against this base (e.g. origin/main).
        #[arg(long)]
        base: Option<String>,
        /// The head of the change (default: the working tree).
        #[arg(long, requires = "base")]
        head: Option<String>,
        /// Also run the rules clippy implements (`cargo clippy`; slow).
        #[arg(long)]
        clippy: bool,
        /// Only report files under these paths (repeatable).
        #[arg(long = "path")]
        paths: Vec<String>,
        /// Apply the suggested fixes, then report what is left.
        #[arg(long)]
        fix: bool,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// List the rules.
    Rules {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Replay the PR-diff rules over the last N commits of HEAD.
    Replay {
        #[arg(long, default_value_t = 50)]
        commits: usize,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Lint, replay, and write everything to a JSON report.
    Report {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 50)]
        commits: usize,
        #[arg(long)]
        clippy: bool,
        /// The repository the root is a checkout of, for links.
        #[arg(long, default_value = "mozilla/application-services")]
        repo: String,
    },
    /// Build the static docs site from the rules, the research and a report.
    Site {
        #[arg(long)]
        report: PathBuf,
        #[arg(long, default_value = "research/rules.json")]
        research: PathBuf,
        #[arg(long, default_value = "rules")]
        docs: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("varlope: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check {
            rules,
            base,
            head,
            clippy,
            paths,
            fix,
            format,
        } => {
            let selected = select(&rules)?;
            let ws = Workspace::load(&cli.root)?;
            let diff = match &base {
                Some(base) => Some(Diff::from_git(&cli.root, base, head.as_deref())?),
                None => None,
            };
            let mut findings = varlope::run::run(&ws, diff.as_ref(), &selected);
            findings.extend(varlope::run::allow_problems(&ws));
            if clippy {
                let from_clippy = varlope::clippy::run(&cli.root, &ws, &selected)?;
                findings.extend(from_clippy.into_iter().filter(|f| {
                    varlope::rule(f.rule).is_some_and(|r| varlope::run::keep(&ws, r, f))
                }));
                varlope::run::sort(&mut findings);
            }
            if !paths.is_empty() {
                findings.retain(|f| {
                    paths
                        .iter()
                        .any(|p| f.path.starts_with(p.trim_start_matches("./")))
                });
            }
            if fix {
                let applied = varlope::fix::apply(&cli.root, &findings)?;
                eprintln!(
                    "fixed {}{}",
                    applied.fixed,
                    if applied.skipped > 0 {
                        format!(", {} overlapped another fix: run again", applied.skipped)
                    } else {
                        String::new()
                    }
                );
                findings.retain(|f| f.fix.is_none());
            }
            print(&findings, format)?;
            Ok(if findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Rules { format } => {
            match format {
                Format::Json => {
                    let infos: Vec<RuleInfo> =
                        varlope::rules().iter().map(|r| RuleInfo::of(r)).collect();
                    println!("{}", serde_json::to_string_pretty(&infos)?);
                }
                Format::Text => {
                    for rule in varlope::rules() {
                        println!("{:<38} {:<22} {}", rule.id, rule.area.title(), rule.title);
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Replay { commits, format } => {
            let ws = Workspace::load(&cli.root)?;
            let (_, findings) = varlope::replay::replay(&cli.root, &ws, varlope::rules(), commits)?;
            print(&findings, format)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Report {
            out,
            commits,
            clippy,
            repo,
        } => {
            let ws = Workspace::load(&cli.root)?;
            let all = varlope::rules();
            let mut findings = varlope::run::run(&ws, None, all);
            if clippy {
                let from_clippy = varlope::clippy::run(&cli.root, &ws, all)?;
                findings.extend(from_clippy.into_iter().filter(|f| {
                    varlope::rule(f.rule).is_some_and(|r| varlope::run::keep(&ws, r, f))
                }));
            }
            // Findings on the code as it is carry the lines around them.
            for f in &mut findings {
                if let Some(file) = ws.file(&f.path) {
                    f.attach_snippet(&file.text);
                }
            }
            let (replayed, diff_findings) = varlope::replay::replay(&cli.root, &ws, all, commits)?;
            findings.extend(diff_findings);
            varlope::run::sort(&mut findings);
            let sha = varlope::diff::git(&cli.root, &["rev-parse", "HEAD"])?
                .trim()
                .to_string();
            let date = varlope::diff::git(&cli.root, &["show", "-s", "--format=%cs", "HEAD"])?
                .trim()
                .to_string();
            let report = Report {
                repo,
                sha,
                date,
                clippy,
                rules: all.iter().map(|r| RuleInfo::of(r)).collect(),
                findings: &findings,
                commits: &replayed,
            };
            std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
            eprintln!(
                "wrote {}: {} findings, {} commits replayed",
                out.display(),
                findings.len(),
                replayed.len()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Site {
            report,
            research,
            docs,
            out,
        } => {
            varlope::site::build(&report, &research, &docs, &out)?;
            eprintln!("wrote {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn select(ids: &[String]) -> Result<Vec<&'static Rule>> {
    if ids.is_empty() {
        return Ok(varlope::rules().to_vec());
    }
    ids.iter()
        .map(|id| match varlope::rule(id) {
            Some(rule) => Ok(rule),
            None => bail!("unknown rule `{id}`; run `varlope rules`"),
        })
        .collect()
}

fn print(findings: &[Finding], format: Format) -> Result<()> {
    match format {
        Format::Text => varlope::report::print_text(findings),
        Format::Json => println!("{}", serde_json::to_string_pretty(findings)?),
    }
    Ok(())
}
