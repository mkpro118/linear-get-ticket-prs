#![allow(forbidden_lint_groups)]

mod error;
mod github;
mod linear;
mod missing;
mod release_notes;

use std::io::{self, BufRead};
use std::process;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use error::Result;

#[derive(Parser)]
#[command(
    name = "linear-get-ticket-prs",
    about = "Find Linear tickets, fetch their GitHub PRs, and filter merged ones",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    orchestrator: OrchestratorArgs,
}

#[derive(Subcommand)]
enum Commands {
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
struct GetTicketsArgs {
    #[arg(short = 'l', long = "label")]
    labels: Vec<String>,

    #[arg(short = 's', long = "status")]
    statuses: Vec<String>,

    #[arg(short = 'a', long = "assignee")]
    assignees: Vec<String>,

    #[arg(short = 'n', long = "limit")]
    limit: Option<usize>,

    #[arg(short = 'k', long = "api-key")]
    api_key: Option<String>,
}

#[derive(Args)]
struct GetPrsArgs {
    #[arg(short = 'n', long = "limit")]
    limit: Option<usize>,

    #[arg(short = 'k', long = "api-key")]
    api_key: Option<String>,
}

#[derive(Args)]
struct FilterMergedArgs {
    #[arg(short = 'r', long = "repo")]
    repo: Option<String>,
}

#[derive(Args)]
struct MissingPrsArgs {
    /// The release branch to compare against (auto-detected from current branch if omitted)
    #[arg(short = 'b', long = "release-branch")]
    release_branch: Option<String>,
}

#[derive(Args)]
struct ReleaseNotesArgs {
    /// The base git ref (must be an ancestor of head)
    #[arg(long)]
    base: String,

    /// The head git ref
    #[arg(long)]
    head: String,

    /// Git config key prefix that maps branch namespaces to GitHub handles
    #[arg(long)]
    config_key: String,

    /// Repository/org name to match in merge commit subjects
    #[arg(long)]
    repo_name: String,
}

#[derive(Args)]
struct CompletionsArgs {
    /// The shell to generate completions for
    shell: Shell,
}

#[derive(Args)]
struct OrchestratorArgs {
    #[arg(short = 'l', long = "label")]
    labels: Vec<String>,

    #[arg(short = 's', long = "status")]
    statuses: Vec<String>,

    #[arg(short = 'a', long = "assignee")]
    assignees: Vec<String>,

    #[arg(long = "limit-tickets")]
    limit_tickets: Option<usize>,

    #[arg(long = "limit-prs")]
    limit_prs: Option<usize>,

    #[arg(short = 'r', long = "repo")]
    repo: Option<String>,

    #[arg(short = 'k', long = "api-key")]
    api_key: Option<String>,

    /// Run missing-prs analysis against this release branch after filter-merged
    #[arg(short = 'b', long = "release-branch")]
    release_branch: Option<String>,
}

fn read_lines_from_stdin() -> Result<Vec<String>> {
    let stdin = io::stdin();
    let lines: Vec<String> = stdin
        .lock()
        .lines()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(lines)
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::GetTickets(args)) => {
            let api_key = linear::resolve_api_key(args.api_key.as_deref())?;
            let tickets = linear::get_tickets(&linear::GetTicketsParams {
                api_key: &api_key,
                labels: &args.labels,
                statuses: &args.statuses,
                assignees: &args.assignees,
                limit: args.limit,
            })?;
            for ticket in &tickets {
                println!("{ticket}");
            }
        }
        Some(Commands::GetPrs(args)) => {
            let api_key = linear::resolve_api_key(args.api_key.as_deref())?;
            let ticket_ids = read_lines_from_stdin()?;
            let prs = linear::get_prs_for_tickets(&linear::GetPrsParams {
                api_key: &api_key,
                ticket_ids: &ticket_ids,
                limit: args.limit,
            })?;
            for pr in &prs {
                println!("{pr}");
            }
        }
        Some(Commands::FilterMerged(args)) => {
            let pr_inputs = read_lines_from_stdin()?;
            let merged = github::filter_merged_prs(&github::FilterMergedParams {
                pr_inputs: &pr_inputs,
                repo: args.repo.as_deref(),
            })?;
            for pr in &merged {
                println!("{pr}");
            }
        }
        Some(Commands::MissingPrs(args)) => {
            let pr_lines = read_lines_from_stdin()?;
            missing::run(&missing::MissingPrsParams {
                pr_lines: &pr_lines,
                release_branch: args.release_branch.as_deref(),
            })?;
        }
        Some(Commands::ReleaseNotes(args)) => {
            release_notes::run(&release_notes::ReleaseNotesParams {
                base: &args.base,
                head: &args.head,
                config_key: &args.config_key,
                repo_name: &args.repo_name,
            })?;
        }
        Some(Commands::Completions(args)) => {
            clap_complete::generate(
                args.shell,
                &mut Cli::command(),
                "linear-get-ticket-prs",
                &mut io::stdout(),
            );
        }
        None => orchestrate(&cli.orchestrator)?,
    }

    Ok(())
}

fn orchestrate(orch: &OrchestratorArgs) -> Result<()> {
    let api_key = linear::resolve_api_key(orch.api_key.as_deref())?;

    let tickets = linear::get_tickets(&linear::GetTicketsParams {
        api_key: &api_key,
        labels: &orch.labels,
        statuses: &orch.statuses,
        assignees: &orch.assignees,
        limit: orch.limit_tickets,
    })?;

    if tickets.is_empty() {
        return Ok(());
    }

    let prs = linear::get_prs_for_tickets(&linear::GetPrsParams {
        api_key: &api_key,
        ticket_ids: &tickets,
        limit: orch.limit_prs,
    })?;

    if prs.is_empty() {
        return Ok(());
    }

    let pr_strings: Vec<String> = prs.iter().map(ToString::to_string).collect();
    let merged = github::filter_merged_prs(&github::FilterMergedParams {
        pr_inputs: &pr_strings,
        repo: orch.repo.as_deref(),
    })?;

    if merged.is_empty() {
        return Ok(());
    }

    if orch.release_branch.is_some() || missing::on_release_branch() {
        let merged_strings: Vec<String> =
            merged.iter().map(ToString::to_string).collect();
        missing::run(&missing::MissingPrsParams {
            pr_lines: &merged_strings,
            release_branch: orch.release_branch.as_deref(),
        })?;
    } else {
        for pr in &merged {
            let url = match &orch.repo {
                Some(repo) => format!("https://github.com/{repo}/pull/{pr}"),
                None => pr.to_string(),
            };
            println!("{url}");
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
