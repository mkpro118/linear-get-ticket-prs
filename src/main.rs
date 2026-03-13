#![allow(forbidden_lint_groups)]

mod error;
mod github;
mod linear;

use std::io::{self, BufRead};
use std::process;

use clap::{Args, Parser, Subcommand};

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
        None => {
            let orch = &cli.orchestrator;
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

            let pr_strings: Vec<String> =
                prs.iter().map(ToString::to_string).collect();
            let merged = github::filter_merged_prs(&github::FilterMergedParams {
                pr_inputs: &pr_strings,
                repo: orch.repo.as_deref(),
            })?;

            for pr in &merged {
                let url = match &orch.repo {
                    Some(repo) => format!("https://github.com/{repo}/pull/{pr}"),
                    None => pr.to_string(),
                };
                println!("{url}");
            }
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
