//! A CLI toolkit for bridging Linear issue tracking with GitHub pull requests.
//!
//! This crate provides both a library and a binary for querying Linear tickets,
//! fetching associated GitHub PRs, filtering by merge status, comparing release
//! branches, and generating release notes — all from the command line.

#![allow(forbidden_lint_groups)]

use std::io::BufRead;

pub mod cli;
pub mod error;
pub mod github;
pub mod linear;
pub mod missing;
pub mod release_notes;

/// Reads non-empty, trimmed lines from stdin.
///
/// Shared utility used by subcommand execute functions that accept
/// piped input (get-prs, filter-merged, missing-prs).
///
/// # Errors
///
/// Returns an I/O error if stdin cannot be read.
pub fn read_lines_from_stdin() -> error::Result<Vec<String>> {
    let stdin = std::io::stdin();
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
