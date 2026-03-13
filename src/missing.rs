//! Release branch analysis engine for finding missing PRs.
//!
//! Compares the first-parent history of `main` against a release branch
//! to find PRs that are "effectively present" on main but absent from the
//! release. Uses `git log | rg` pipelines and the odd/even mention counting
//! rule to determine effective presence. Outputs formatted tables and a
//! ready-to-paste `git cherry-pick` command.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Stdio};

use clap::Args;

use crate::error::{Error, Result};

/// Arguments for the `missing-prs` subcommand.
#[derive(Args)]
pub struct MissingPrsArgs {
    /// The release branch to compare against (auto-detected from current branch if omitted)
    #[arg(short = 'b', long = "release-branch")]
    pub release_branch: Option<String>,
}

/// Executes the `missing-prs` subcommand: reads PR numbers from stdin
/// and runs the full analysis pipeline.
///
/// # Errors
///
/// Returns an error if stdin cannot be read or the git/rg pipeline fails.
pub fn execute(args: &MissingPrsArgs) -> Result<()> {
    let pr_lines = crate::read_lines_from_stdin()?;
    run(&MissingPrsParams {
        pr_lines: &pr_lines,
        release_branch: args.release_branch.as_deref(),
    })
}

/// Parameters for [`run`].
pub struct MissingPrsParams<'a> {
    pub pr_lines: &'a [String],
    pub release_branch: Option<&'a str>,
}

struct ResolvedPr {
    pr: u64,
    sha: String,
    subject: String,
}

struct ExtractMentionsParams<'a> {
    branch: &'a str,
    pattern: &'a str,
}

struct ResolveShasParams<'a> {
    prs: &'a [u64],
    pattern: &'a str,
}

/// Runs the full missing-PRs analysis pipeline.
///
/// # Errors
///
/// Returns an error if required tools are missing, the release branch is
/// invalid, or the git/rg pipeline fails.
pub fn run(params: &MissingPrsParams) -> Result<()> {
    check_required_tools()?;

    // Step 1
    let tracked_prs = parse_pr_numbers(params.pr_lines)?;

    // Resolve release branch
    let release_branch = if let Some(b) = params.release_branch {
        validate_branch(b)?;
        b.to_string()
    } else {
        let detected = detect_release_branch()?;
        eprintln!("info: auto-detected release branch: {detected}");
        detected
    };

    // Step 2
    let pattern = build_rg_pattern(&tracked_prs);

    // Steps 3 & 4
    let main_mentions = extract_mentions(&ExtractMentionsParams {
        branch: "main",
        pattern: &pattern,
    })?;
    let release_mentions = extract_mentions(&ExtractMentionsParams {
        branch: &release_branch,
        pattern: &pattern,
    })?;

    // Step 5
    let main_effective = compute_effective(&main_mentions);
    let release_effective = compute_effective(&release_mentions);

    // Step 6
    let missing_set: HashSet<u64> = main_effective.difference(&release_effective).copied().collect();

    // Step 7 — order by first appearance in main_mentions (newest-first)
    let mut seen = HashSet::new();
    let missing_ordered: Vec<u64> = main_mentions
        .iter()
        .filter(|pr| missing_set.contains(pr) && seen.insert(**pr))
        .copied()
        .collect();

    // Step 8
    let tracked_set: HashSet<u64> = tracked_prs.iter().copied().collect();
    let mut not_found: Vec<u64> = tracked_set
        .iter()
        .filter(|pr| !main_effective.contains(pr) && !release_effective.contains(pr))
        .copied()
        .collect();
    not_found.sort_unstable();

    // Step 9
    let missing_pattern = build_rg_pattern(&missing_ordered);
    let resolved = if missing_ordered.is_empty() {
        Vec::new()
    } else {
        resolve_shas(&ResolveShasParams {
            prs: &missing_ordered,
            pattern: &missing_pattern,
        })?
    };

    // Build a lookup from PR number to resolved info
    let resolved_map: HashMap<u64, &ResolvedPr> =
        resolved.iter().map(|r| (r.pr, r)).collect();

    // --- Output ---

    // Missing PRs table
    let mut table = String::from("PR\tSHA\tSUBJECT\n");
    for &pr in &missing_ordered {
        if let Some(r) = resolved_map.get(&pr) {
            let sha_short = if r.sha.len() >= 10 { &r.sha[..10] } else { &r.sha };
            let _ = writeln!(table, "#{pr}\t{sha_short}\t{}", r.subject);
        } else {
            let _ = writeln!(table, "#{pr}\t\t");
        }
    }
    print_columnar(&table)?;

    // Not found table
    if !not_found.is_empty() {
        println!();
        let mut nf_table = String::from("NOT FOUND\n");
        for pr in &not_found {
            let _ = writeln!(nf_table, "#{pr}");
        }
        print_columnar(&nf_table)?;
    }

    // Summary
    println!();
    let summary = format!(
        "Tracked\t{}\nmain\t{}\nRelease\t{}\nMissing\t{}\nNot found\t{}\n",
        tracked_prs.len(),
        main_effective.len(),
        release_effective.len(),
        missing_ordered.len(),
        not_found.len(),
    );
    print_columnar(&summary)?;

    // Cherry-pick command
    // Collect resolved SHAs in oldest-first order (reverse of missing_ordered)
    let cherry_pick_entries: Vec<(&ResolvedPr, u64)> = missing_ordered
        .iter()
        .rev()
        .filter_map(|&pr| resolved_map.get(&pr).map(|r| (*r, pr)))
        .collect();

    if !cherry_pick_entries.is_empty() {
        println!();
        let shas: Vec<&str> = cherry_pick_entries.iter().map(|(r, _)| r.sha.as_str()).collect();
        println!("git cherry-pick -x --mainline=1 {}", shas.join(" "));
    }

    Ok(())
}

fn check_required_tools() -> Result<()> {
    for tool in &["git", "rg"] {
        let status = Command::new(tool)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => return Err(Error::MissingTool((*tool).to_string())),
        }
    }
    Ok(())
}

fn parse_pr_numbers(lines: &[String]) -> Result<Vec<u64>> {
    if lines.is_empty() {
        return Err(Error::EmptyInput);
    }

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for line in lines {
        let n: u64 = line
            .parse()
            .map_err(|_| Error::InvalidPrNumber(line.clone()))?;
        if n == 0 {
            return Err(Error::InvalidPrNumber(line.clone()));
        }
        if seen.insert(n) {
            result.push(n);
        }
    }

    Ok(result)
}

fn build_rg_pattern(prs: &[u64]) -> String {
    let alternation: Vec<String> = prs.iter().map(ToString::to_string).collect();
    format!("#({}){}", alternation.join("|"), r"\b")
}

fn extract_mentions(params: &ExtractMentionsParams) -> Result<Vec<u64>> {
    let mut git = Command::new("git")
        .args(["log", "--oneline", "--first-parent", params.branch])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let git_stdout = git.stdout.take().expect("piped stdout");

    let rg_output = Command::new("rg")
        .args(["-oN", params.pattern])
        .stdin(Stdio::from(git_stdout))
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "rg".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let git_status = git.wait()?;
    if !git_status.success() {
        return Err(Error::SubprocessFailed {
            command: "git log".to_string(),
            stderr: String::new(),
            exit_code: git_status.code(),
        });
    }

    // rg exits 1 when no matches found — that's not an error
    if !rg_output.status.success() && rg_output.status.code() != Some(1) {
        return Err(Error::SubprocessFailed {
            command: "rg".to_string(),
            stderr: String::from_utf8_lossy(&rg_output.stderr).to_string(),
            exit_code: rg_output.status.code(),
        });
    }

    let stdout = String::from_utf8_lossy(&rg_output.stdout);
    let mentions: Vec<u64> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix('#').and_then(|s| s.parse().ok()))
        .collect();

    Ok(mentions)
}

fn compute_effective(mentions: &[u64]) -> HashSet<u64> {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for &pr in mentions {
        *counts.entry(pr).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| count % 2 == 1)
        .map(|(pr, _)| pr)
        .collect()
}

fn resolve_shas(params: &ResolveShasParams) -> Result<Vec<ResolvedPr>> {
    let mut git = Command::new("git")
        .args(["log", "--format=%H %s", "--first-parent", "main"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let git_stdout = git.stdout.take().expect("piped stdout");

    let rg_output = Command::new("rg")
        .args(["-N", params.pattern])
        .stdin(Stdio::from(git_stdout))
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "rg".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let git_status = git.wait()?;
    if !git_status.success() {
        return Err(Error::SubprocessFailed {
            command: "git log".to_string(),
            stderr: String::new(),
            exit_code: git_status.code(),
        });
    }

    if !rg_output.status.success() && rg_output.status.code() != Some(1) {
        return Err(Error::SubprocessFailed {
            command: "rg".to_string(),
            stderr: String::from_utf8_lossy(&rg_output.stderr).to_string(),
            exit_code: rg_output.status.code(),
        });
    }

    let target_set: HashSet<u64> = params.prs.iter().copied().collect();
    let mut resolved_map: HashMap<u64, ResolvedPr> = HashMap::new();
    let stdout = String::from_utf8_lossy(&rg_output.stdout);

    for line in stdout.lines() {
        let Some((sha, subject)) = line.split_once(' ') else {
            continue;
        };

        for pr in extract_pr_refs_from_subject(subject) {
            if target_set.contains(&pr) && !resolved_map.contains_key(&pr) {
                resolved_map.insert(
                    pr,
                    ResolvedPr {
                        pr,
                        sha: sha.to_string(),
                        subject: subject.to_string(),
                    },
                );
            }
        }
    }

    // Fetch %b (body) for each resolved commit to use as the display subject
    for resolved in resolved_map.values_mut() {
        if let Ok(body) = fetch_commit_body(&resolved.sha)
            && !body.is_empty()
        {
            resolved.subject = body;
        }
    }

    // Return in the same order as params.prs
    let result: Vec<ResolvedPr> = params
        .prs
        .iter()
        .filter_map(|pr| resolved_map.remove(pr))
        .collect();

    Ok(result)
}

fn fetch_commit_body(sha: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%b", sha])
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let body = String::from_utf8_lossy(&output.stdout);
    let first_line = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    Ok(first_line.trim().to_string())
}

fn extract_pr_refs_from_subject(subject: &str) -> Vec<u64> {
    let mut result = Vec::new();
    let bytes = subject.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'#' {
            let start = i + 1;
            let mut end = start;
            while end < len && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                let at_word_boundary =
                    end >= len || !bytes[end].is_ascii_alphanumeric();
                if at_word_boundary
                    && let Ok(n) = subject[start..end].parse::<u64>()
                {
                    result.push(n);
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }

    result
}

/// Returns `true` if the current git branch looks like a release branch (`release/v*`).
///
/// Used by the orchestrator to decide whether to auto-engage the missing-prs stage.
#[must_use]
pub fn on_release_branch() -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok();
    output
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|b| b.trim().starts_with("release/v"))
}

fn detect_release_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if !output.status.success() {
        return Err(Error::NoBranchDetected);
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if branch.starts_with("release/v") {
        Ok(branch)
    } else {
        Err(Error::NoBranchDetected)
    }
}

fn validate_branch(branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if output.success() {
        Ok(())
    } else {
        Err(Error::InvalidBranch(branch.to_string()))
    }
}

fn print_columnar(data: &str) -> Result<()> {
    let mut column = Command::new("column")
        .args(["-t", "-s", "\t"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| Error::SubprocessFailed {
            command: "column".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if let Some(mut stdin) = column.stdin.take() {
        stdin.write_all(data.as_bytes())?;
    }

    let output = column.wait_with_output().map_err(|e| Error::SubprocessFailed {
        command: "column".to_string(),
        stderr: e.to_string(),
        exit_code: None,
    })?;

    std::io::stdout().write_all(&output.stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_numbers_valid() {
        let lines = vec!["123".to_string(), "456".to_string(), "789".to_string()];
        let result = parse_pr_numbers(&lines).unwrap();
        assert_eq!(result, vec![123, 456, 789]);
    }

    #[test]
    fn test_parse_pr_numbers_dedup() {
        let lines = vec!["100".to_string(), "200".to_string(), "100".to_string()];
        let result = parse_pr_numbers(&lines).unwrap();
        assert_eq!(result, vec![100, 200]);
    }

    #[test]
    fn test_parse_pr_numbers_invalid() {
        let lines = vec!["123".to_string(), "abc".to_string()];
        let result = parse_pr_numbers(&lines);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pr_numbers_zero_rejected() {
        let lines = vec!["0".to_string()];
        let result = parse_pr_numbers(&lines);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pr_numbers_empty() {
        let lines: Vec<String> = vec![];
        let result = parse_pr_numbers(&lines);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_rg_pattern_single() {
        let prs = vec![42];
        assert_eq!(build_rg_pattern(&prs), r"#(42)\b");
    }

    #[test]
    fn test_build_rg_pattern_multiple() {
        let prs = vec![123, 456, 789];
        assert_eq!(build_rg_pattern(&prs), r"#(123|456|789)\b");
    }

    #[test]
    fn test_compute_effective_odd_present() {
        // 10 appears 3 times (odd → present), 20 appears 1 time (odd → present)
        let mentions = vec![10, 20, 10, 10];
        let effective = compute_effective(&mentions);
        assert!(effective.contains(&10));
        assert!(effective.contains(&20));
    }

    #[test]
    fn test_compute_effective_even_absent() {
        let mentions = vec![5, 5];
        let effective = compute_effective(&mentions);
        assert!(!effective.contains(&5));
    }

    #[test]
    fn test_compute_effective_mixed() {
        // 1 appears 3 times (odd → present), 2 appears 2 times (even → absent), 3 appears 1 time (odd → present)
        let mentions = vec![1, 2, 3, 1, 2, 1];
        let effective = compute_effective(&mentions);
        assert!(effective.contains(&1));
        assert!(!effective.contains(&2));
        assert!(effective.contains(&3));
    }

    #[test]
    fn test_compute_effective_empty() {
        let mentions: Vec<u64> = vec![];
        let effective = compute_effective(&mentions);
        assert!(effective.is_empty());
    }

    #[test]
    fn test_extract_pr_refs_from_subject_single() {
        let refs = extract_pr_refs_from_subject("Merge pull request #123 from org/branch");
        assert_eq!(refs, vec![123]);
    }

    #[test]
    fn test_extract_pr_refs_from_subject_multiple() {
        let refs = extract_pr_refs_from_subject("fix #10 and #20");
        assert_eq!(refs, vec![10, 20]);
    }

    #[test]
    fn test_extract_pr_refs_word_boundary() {
        // #12 must not match inside #123
        let refs = extract_pr_refs_from_subject("issue #123 resolved");
        assert_eq!(refs, vec![123]);
        assert!(!refs.contains(&12));
    }

    #[test]
    fn test_extract_pr_refs_no_match() {
        let refs = extract_pr_refs_from_subject("no references here");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_pr_refs_at_end_of_string() {
        let refs = extract_pr_refs_from_subject("fixed #99");
        assert_eq!(refs, vec![99]);
    }

    #[test]
    fn test_extract_pr_refs_followed_by_punctuation() {
        let refs = extract_pr_refs_from_subject("see #55, #66.");
        assert_eq!(refs, vec![55, 66]);
    }
}
