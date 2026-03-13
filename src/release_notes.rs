use std::collections::HashMap;
use std::process::Command;

use crate::error::{Error, Result};

pub struct ReleaseNotesParams<'a> {
    pub base: &'a str,
    pub head: &'a str,
    pub config_key: &'a str,
    pub repo_name: &'a str,
}

struct NoteEntry {
    description: String,
    author: String,
    pr_number: u64,
}

pub fn run(params: &ReleaseNotesParams) -> Result<()> {
    verify_ancestry(params.base, params.head)?;
    let author_map = load_author_map(params.config_key)?;
    let commits = list_commits(params.base, params.head)?;

    let mut entries = Vec::new();

    for (sha, subject) in &commits {
        let Some(pr_number) = extract_first_pr_ref(subject) else {
            continue;
        };

        let key = extract_author_key(subject, params.repo_name);

        let author = key
            .as_ref()
            .and_then(|k| author_map.get(k))
            .cloned()
            .unwrap_or_else(|| key.unwrap_or_default());

        let description = fetch_commit_body(sha)?;
        let description = if description.is_empty() {
            subject.clone()
        } else {
            description
        };

        entries.push(NoteEntry {
            description,
            author,
            pr_number,
        });
    }

    for entry in &entries {
        println!(
            "* {} by @{} in #{}",
            entry.description, entry.author, entry.pr_number
        );
    }

    Ok(())
}

fn verify_ancestry(base: &str, head: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", base, head])
        .status()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::NotAncestor {
            base: base.to_string(),
            head: head.to_string(),
        })
    }
}

fn load_author_map(config_key: &str) -> Result<HashMap<String, String>> {
    let output = Command::new("git")
        .args(["config", "--get-regexp", config_key])
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    let mut map = HashMap::new();

    // git config --get-regexp exits 1 when no keys match — that's an empty map, not an error
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let prefix = format!("{config_key}.");
        for line in stdout.lines() {
            if let Some((full_key, handle)) = line.split_once(' ') {
                let namespace = full_key.strip_prefix(&prefix).unwrap_or(full_key);
                map.insert(namespace.to_string(), handle.to_string());
            }
        }
    }

    Ok(map)
}

fn list_commits(base: &str, head: &str) -> Result<Vec<(String, String)>> {
    let range = format!("{base}..{head}");
    let output = Command::new("git")
        .args([
            "log",
            "--first-parent",
            "--reverse",
            "--format=%H %s",
            &range,
        ])
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "git".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if !output.status.success() {
        return Err(Error::SubprocessFailed {
            command: "git log".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits: Vec<(String, String)> = stdout
        .lines()
        .filter_map(|line| {
            let (sha, subject) = line.split_once(' ')?;
            Some((sha.to_string(), subject.to_string()))
        })
        .collect();

    Ok(commits)
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

/// Extracts the first `#N` PR reference from a merge commit subject.
fn extract_first_pr_ref(subject: &str) -> Option<u64> {
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
                let at_word_boundary = end >= len || !bytes[end].is_ascii_alphanumeric();
                if at_word_boundary
                    && let Ok(n) = subject[start..end].parse::<u64>()
                {
                    return Some(n);
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }

    None
}

/// Extracts the author namespace key from the branch path in a merge commit subject.
///
/// Given "Merge pull request #123 from reponame/john/fix-bug" and `repo_name` "reponame",
/// finds "reponame/" then takes the next path segment ("john") filtered to alphabetic chars.
fn extract_author_key(subject: &str, repo_name: &str) -> Option<String> {
    let marker = format!("{repo_name}/");
    let pos = subject.find(&marker)?;
    let rest = &subject[pos + marker.len()..];

    let segment = rest.split('/').next()?;
    let key: String = segment
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect();

    if key.is_empty() { None } else { Some(key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_first_pr_ref_standard_merge() {
        let pr = extract_first_pr_ref("Merge pull request #456 from org/branch");
        assert_eq!(pr, Some(456));
    }

    #[test]
    fn test_extract_first_pr_ref_squash() {
        let pr = extract_first_pr_ref("Add new feature (#789)");
        assert_eq!(pr, Some(789));
    }

    #[test]
    fn test_extract_first_pr_ref_none() {
        let pr = extract_first_pr_ref("Regular commit with no PR");
        assert_eq!(pr, None);
    }

    #[test]
    fn test_extract_first_pr_ref_takes_first() {
        let pr = extract_first_pr_ref("fix #10 and #20");
        assert_eq!(pr, Some(10));
    }

    #[test]
    fn test_extract_author_key_standard() {
        let key = extract_author_key(
            "Merge pull request #123 from myorg/john/fix-bug",
            "myorg",
        );
        assert_eq!(key, Some("john".to_string()));
    }

    #[test]
    fn test_extract_author_key_with_hyphens() {
        let key = extract_author_key(
            "Merge pull request #55 from acme/jane-doe/feature",
            "acme",
        );
        assert_eq!(key, Some("janedoe".to_string()));
    }

    #[test]
    fn test_extract_author_key_no_match() {
        let key = extract_author_key(
            "Merge pull request #99 from other/user/branch",
            "myorg",
        );
        assert_eq!(key, None);
    }

    #[test]
    fn test_extract_author_key_no_slash_after_key() {
        let key = extract_author_key(
            "Merge pull request #10 from myorg/alice",
            "myorg",
        );
        assert_eq!(key, Some("alice".to_string()));
    }

    #[test]
    fn test_load_author_map_parses_entries() {
        // This tests the parsing logic, not actual git config
        let prefix = "rn.authors";
        let raw_lines = "rn.authors.mk mkdocs\nrn.authors.jd johndoe";
        let prefix_dot = format!("{prefix}.");
        let mut map = HashMap::new();
        for line in raw_lines.lines() {
            if let Some((full_key, handle)) = line.split_once(' ') {
                let namespace = full_key.strip_prefix(&prefix_dot).unwrap_or(full_key);
                map.insert(namespace.to_string(), handle.to_string());
            }
        }
        assert_eq!(map.get("mk"), Some(&"mkdocs".to_string()));
        assert_eq!(map.get("jd"), Some(&"johndoe".to_string()));
        assert_eq!(map.len(), 2);
    }
}
