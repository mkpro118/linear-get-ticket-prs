use std::io;
use std::process;

use clap::{CommandFactory, Parser};

use linear_get_ticket_prs::cli::{Cli, Commands, OrchestratorArgs};
use linear_get_ticket_prs::{error, github, linear, missing, release_notes};

fn run() -> error::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::GetTickets(args)) => linear::execute_get_tickets(&args)?,
        Some(Commands::GetPrs(args)) => linear::execute_get_prs(&args)?,
        Some(Commands::FilterMerged(args)) => github::execute_filter_merged(&args)?,
        Some(Commands::MissingPrs(args)) => missing::execute(&args)?,
        Some(Commands::ReleaseNotes(args)) => release_notes::execute(&args)?,
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

fn orchestrate(orch: &OrchestratorArgs) -> error::Result<()> {
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
