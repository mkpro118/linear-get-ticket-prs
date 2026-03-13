//! GitHub CLI wrapper for PR merge status filtering.
//!
//! Uses `gh pr view` to check the merge status of each PR number or URL.
//! Accepts both bare PR numbers and full GitHub PR URLs on stdin.

use std::process::Command;

use clap::Args;
use serde::Deserialize;

use crate::error::{Error, Result};

/// Arguments for the `filter-merged` subcommand.
#[derive(Args)]
pub struct FilterMergedArgs {
    #[arg(short = 'r', long = "repo")]
    pub repo: Option<String>,
}

/// Executes the `filter-merged` subcommand: reads PR inputs from stdin,
/// checks merge status via `gh`, and prints only merged PR numbers.
///
/// # Errors
///
/// Returns an error if stdin cannot be read or `gh` fails unexpectedly.
pub fn execute_filter_merged(args: &FilterMergedArgs) -> Result<()> {
    let pr_inputs = crate::read_lines_from_stdin()?;
    let merged = filter_merged_prs(&FilterMergedParams {
        pr_inputs: &pr_inputs,
        repo: args.repo.as_deref(),
    })?;
    for pr in &merged {
        println!("{pr}");
    }
    Ok(())
}

#[derive(Deserialize)]
struct PrState {
    state: String,
}

/// Parameters for [`filter_merged_prs`].
pub struct FilterMergedParams<'a> {
    pub pr_inputs: &'a [String],
    pub repo: Option<&'a str>,
}

/// Accepts PR numbers or full GitHub PR URLs. Returns the PR numbers that are merged.
///
/// # Errors
///
/// Returns an error if `gh` cannot be invoked or JSON parsing fails.
pub fn filter_merged_prs(params: &FilterMergedParams) -> Result<Vec<u64>> {
    let mut merged = Vec::new();

    for input in params.pr_inputs {
        let pr_number = parse_pr_input(input);
        let pr_ref = pr_number.as_deref().unwrap_or(input.as_str());

        let mut cmd = Command::new("gh");
        cmd.args(["pr", "view", pr_ref, "--json", "state"]);

        if let Some(repo) = params.repo {
            cmd.args(["--repo", repo]);
        }

        let output = cmd.output().map_err(|e| Error::SubprocessFailed {
            command: "gh".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "warning: gh pr view failed for {pr_ref}: {}",
                stderr.trim()
            );
            continue;
        }

        let pr_state: PrState = serde_json::from_slice(&output.stdout)?;

        if pr_state.state == "MERGED" {
            let number = pr_ref
                .parse::<u64>()
                .or_else(|_| extract_number_from_url(input))
                .unwrap_or(0);
            if number > 0 {
                merged.push(number);
            }
        }
    }

    Ok(merged)
}

/// If the input is a full GitHub URL, extract the PR number portion.
/// Otherwise return the input as-is (assumed to already be a number string).
fn parse_pr_input(input: &str) -> Option<String> {
    if input.contains("github.com") && input.contains("/pull/") {
        let parts: Vec<&str> = input.split('/').collect();
        if let (Some("pull"), Some(num)) = (
            parts.iter().rev().nth(1).copied(),
            parts.last().copied(),
        )
            && num.parse::<u64>().is_ok()
        {
            return Some(num.to_string());
        }
    }
    None
}

fn extract_number_from_url(url: &str) -> std::result::Result<u64, std::num::ParseIntError> {
    let parts: Vec<&str> = url.split('/').collect();
    parts
        .last()
        .unwrap_or(&"0")
        .parse::<u64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_input_number_string() {
        assert_eq!(parse_pr_input("123"), None);
    }

    #[test]
    fn test_parse_pr_input_full_url() {
        let url = "https://github.com/acme/repo/pull/456";
        assert_eq!(parse_pr_input(url), Some("456".to_string()));
    }

    #[test]
    fn test_parse_pr_input_not_a_pr_url() {
        let url = "https://github.com/acme/repo/issues/789";
        assert_eq!(parse_pr_input(url), None);
    }

    #[test]
    fn test_extract_number_from_url() {
        assert_eq!(
            extract_number_from_url("https://github.com/org/repo/pull/42"),
            Ok(42)
        );
    }
}
