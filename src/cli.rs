use std::fs;
use std::path::Path;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::github::FilterMergedArgs;
use crate::linear::{GetPrsArgs, GetTicketsArgs};
use crate::missing::MissingPrsArgs;
use crate::release_notes::ReleaseNotesArgs;

#[derive(Parser)]
#[command(
    name = "linear-get-ticket-prs",
    about = "Find Linear tickets, fetch their GitHub PRs, and filter merged ones",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub orchestrator: OrchestratorArgs,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Returns ticket identifiers matching specified filters, one per line
    GetTickets(GetTicketsArgs),
    /// Fetches GitHub PR numbers for tickets provided on stdin, one per line
    GetPrs(GetPrsArgs),
    /// Filters stdin PR numbers/URLs to only those in MERGED status
    FilterMerged(FilterMergedArgs),
    /// Find PRs present on main but missing from a release branch
    MissingPrs(MissingPrsArgs),
    /// Generate human-readable release notes between two git refs
    ReleaseNotes(ReleaseNotesArgs),
    /// Generate shell completions and print to stdout
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct CompletionsArgs {
    /// The shell to generate completions for
    pub shell: Shell,
}

#[derive(Args)]
pub struct OrchestratorArgs {
    #[arg(short = 'l', long = "label")]
    pub labels: Vec<String>,

    #[arg(short = 's', long = "status")]
    pub statuses: Vec<String>,

    #[arg(short = 'a', long = "assignee")]
    pub assignees: Vec<String>,

    #[arg(long = "limit-tickets")]
    pub limit_tickets: Option<usize>,

    #[arg(long = "limit-prs")]
    pub limit_prs: Option<usize>,

    #[arg(short = 'r', long = "repo")]
    pub repo: Option<String>,

    #[arg(short = 'k', long = "api-key")]
    pub api_key: Option<String>,

    /// Run missing-prs analysis against this release branch after filter-merged
    #[arg(short = 'b', long = "release-branch")]
    pub release_branch: Option<String>,
}

const SUBCOMMANDS: &[&str] = &[
    "get-tickets",
    "get-prs",
    "filter-merged",
    "missing-prs",
    "release-notes",
    "completions",
];

pub fn generate_docs(output_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(output_dir)?;

    let cmd = Cli::command();

    let help_text = render_help(&cmd);
    let content = format!("# Orchestrator (default mode)\n\n```\n{help_text}\n```\n");
    fs::write(output_dir.join("orchestrator.md"), content)?;

    for name in SUBCOMMANDS {
        let Some(sub) = cmd.find_subcommand(name) else {
            continue;
        };

        let text = render_help(sub);
        let title = titlecase(name);
        let content = format!("# {title}\n\n```\n{text}\n```\n");
        fs::write(output_dir.join(format!("{name}.md")), content)?;
    }

    Ok(())
}

fn render_help(cmd: &clap::Command) -> String {
    cmd.clone().render_help().to_string()
}

fn titlecase(kebab: &str) -> String {
    kebab
        .split('-')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
