#![allow(forbidden_lint_groups)]
#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

use std::io::BufRead;

pub mod cli;
pub mod error;
pub mod github;
pub mod linear;
pub mod missing;
pub mod release_notes;

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
