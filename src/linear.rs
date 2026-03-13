//! Linear GraphQL API client for ticket queries and PR extraction.
//!
//! Uses `curl` as a subprocess to call the Linear GraphQL endpoint.
//! Provides paginated ticket fetching with label/status/assignee filters,
//! and batched alias queries to extract GitHub PR numbers from ticket
//! attachments. Warns on stderr when a ticket has no associated PRs.

use std::collections::HashSet;
use std::process::Command;

use clap::Args;
use serde::Deserialize;

use crate::error::{Error, Result};

/// Arguments for the `get-tickets` subcommand.
#[derive(Args)]
pub struct GetTicketsArgs {
    #[arg(short = 'l', long = "label")]
    pub labels: Vec<String>,

    #[arg(short = 's', long = "status")]
    pub statuses: Vec<String>,

    #[arg(short = 'a', long = "assignee")]
    pub assignees: Vec<String>,

    #[arg(short = 'n', long = "limit")]
    pub limit: Option<usize>,

    #[arg(short = 'k', long = "api-key")]
    pub api_key: Option<String>,
}

/// Arguments for the `get-prs` subcommand.
#[derive(Args)]
pub struct GetPrsArgs {
    #[arg(short = 'n', long = "limit")]
    pub limit: Option<usize>,

    #[arg(short = 'k', long = "api-key")]
    pub api_key: Option<String>,
}

/// Executes the `get-tickets` subcommand: queries Linear and prints ticket identifiers.
///
/// # Errors
///
/// Returns an error if the API key is missing or the Linear API call fails.
pub fn execute_get_tickets(args: &GetTicketsArgs) -> Result<()> {
    let api_key = resolve_api_key(args.api_key.as_deref())?;
    let tickets = get_tickets(&GetTicketsParams {
        api_key: &api_key,
        labels: &args.labels,
        statuses: &args.statuses,
        assignees: &args.assignees,
        limit: args.limit,
    })?;
    for ticket in &tickets {
        println!("{ticket}");
    }
    Ok(())
}

/// Executes the `get-prs` subcommand: reads ticket IDs from stdin,
/// queries Linear for PR attachments, and prints PR numbers.
///
/// # Errors
///
/// Returns an error if stdin cannot be read or the Linear API call fails.
pub fn execute_get_prs(args: &GetPrsArgs) -> Result<()> {
    let api_key = resolve_api_key(args.api_key.as_deref())?;
    let ticket_ids = crate::read_lines_from_stdin()?;
    let prs = get_prs_for_tickets(&GetPrsParams {
        api_key: &api_key,
        ticket_ids: &ticket_ids,
        limit: args.limit,
    })?;
    for pr in &prs {
        println!("{pr}");
    }
    Ok(())
}

const LINEAR_GRAPHQL_ENDPOINT: &str = "https://api.linear.app/graphql";
const PAGE_SIZE: usize = 50;
const ALIAS_BATCH_SIZE: usize = 50;

/// Resolves the Linear API key from CLI args or the `LINEAR_API_KEY` environment variable.
///
/// # Errors
///
/// Returns [`Error::ApiKeyNotFound`] if neither source provides a key.
pub fn resolve_api_key(cli_key: Option<&str>) -> Result<String> {
    if let Some(key) = cli_key {
        return Ok(key.to_string());
    }
    std::env::var("LINEAR_API_KEY").map_err(|_| Error::ApiKeyNotFound)
}

fn graphql_request(params: &GraphqlRequestParams) -> Result<serde_json::Value> {
    let body = serde_json::json!({ "query": params.query });
    let body_str = serde_json::to_string(&body)?;

    let output = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!("Authorization: {}", params.api_key),
            "-d",
            &body_str,
            LINEAR_GRAPHQL_ENDPOINT,
        ])
        .output()
        .map_err(|e| Error::SubprocessFailed {
            command: "curl".to_string(),
            stderr: e.to_string(),
            exit_code: None,
        })?;

    if !output.status.success() {
        return Err(Error::SubprocessFailed {
            command: "curl".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        });
    }

    let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;

    if let Some(errors) = response.get("errors")
        && let Some(arr) = errors.as_array()
    {
        let messages: Vec<String> = arr
            .iter()
            .filter_map(|e| e.get("message").and_then(|m| m.as_str()).map(String::from))
            .collect();
        if !messages.is_empty() {
            return Err(Error::GraphqlErrors(messages));
        }
    }

    response
        .get("data")
        .cloned()
        .ok_or_else(|| Error::GraphqlErrors(vec!["response missing 'data' field".to_string()]))
}

struct GraphqlRequestParams<'a> {
    api_key: &'a str,
    query: &'a str,
}

// --- get_tickets ---

#[derive(Deserialize)]
struct IssuesResponse {
    issues: IssuesConnection,
}

#[derive(Deserialize)]
struct IssuesConnection {
    nodes: Vec<IssueNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct IssueNode {
    identifier: String,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

/// Parameters for [`get_tickets`].
pub struct GetTicketsParams<'a> {
    pub api_key: &'a str,
    pub labels: &'a [String],
    pub statuses: &'a [String],
    pub assignees: &'a [String],
    pub limit: Option<usize>,
}

fn build_issue_filter(params: &GetTicketsParams) -> String {
    let mut parts = Vec::new();

    if !params.labels.is_empty() {
        let values = json_string_array(params.labels);
        parts.push(format!("labels: {{ name: {{ in: {values} }} }}"));
    }
    if !params.statuses.is_empty() {
        let values = json_string_array(params.statuses);
        parts.push(format!("state: {{ name: {{ in: {values} }} }}"));
    }
    if !params.assignees.is_empty() {
        let values = json_string_array(params.assignees);
        parts.push(format!("assignee: {{ name: {{ in: {values} }} }}"));
    }

    if parts.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", parts.join(", "))
    }
}

fn json_string_array(values: &[String]) -> String {
    let escaped: Vec<String> = values
        .iter()
        .map(|v| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", escaped.join(", "))
}

/// Queries Linear for issues matching the given filters, returning their identifiers.
///
/// Handles pagination automatically; respects the optional limit.
///
/// # Errors
///
/// Returns an error if the API call fails or the response cannot be parsed.
pub fn get_tickets(params: &GetTicketsParams) -> Result<Vec<String>> {
    let filter = build_issue_filter(params);
    let mut all_identifiers = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let remaining = params.limit.map(|l| l.saturating_sub(all_identifiers.len()));
        if remaining == Some(0) {
            break;
        }

        let fetch_count = match remaining {
            Some(r) => r.min(PAGE_SIZE),
            None => PAGE_SIZE,
        };

        let after_clause = match &cursor {
            Some(c) => format!(", after: \"{}\"", c.replace('"', "\\\"")),
            None => String::new(),
        };

        let query = format!(
            r"{{ issues(filter: {filter}, first: {fetch_count}{after_clause}) {{ nodes {{ identifier }} pageInfo {{ hasNextPage endCursor }} }} }}"
        );

        let data = graphql_request(&GraphqlRequestParams {
            api_key: params.api_key,
            query: &query,
        })?;

        let response: IssuesResponse = serde_json::from_value(data)?;

        for node in &response.issues.nodes {
            all_identifiers.push(node.identifier.clone());
            if let Some(limit) = params.limit
                && all_identifiers.len() >= limit
            {
                return Ok(all_identifiers);
            }
        }

        if !response.issues.page_info.has_next_page {
            break;
        }
        cursor = response.issues.page_info.end_cursor;
    }

    Ok(all_identifiers)
}

// --- get_prs_for_tickets ---

/// Parameters for [`get_prs_for_tickets`].
pub struct GetPrsParams<'a> {
    pub api_key: &'a str,
    pub ticket_ids: &'a [String],
    pub limit: Option<usize>,
}

/// Fetches GitHub PR numbers for the given Linear ticket IDs.
///
/// Uses batched GraphQL alias queries. Warns on stderr for tickets
/// with no PR attachments (including the assignee name when available).
///
/// # Errors
///
/// Returns an error if the API call fails or the response cannot be parsed.
pub fn get_prs_for_tickets(params: &GetPrsParams) -> Result<Vec<u64>> {
    if params.ticket_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for chunk in params.ticket_ids.chunks(ALIAS_BATCH_SIZE) {
        let query = build_alias_query(chunk);

        let data = graphql_request(&GraphqlRequestParams {
            api_key: params.api_key,
            query: &query,
        })?;

        let obj = data
            .as_object()
            .ok_or_else(|| Error::GraphqlErrors(vec!["expected object in data".to_string()]))?;

        for (alias, issue_val) in obj {
            let ticket_id = alias
                .strip_prefix('i')
                .and_then(|idx| idx.parse::<usize>().ok())
                .and_then(|idx| chunk.get(idx));

            if issue_val.is_null() {
                if let Some(id) = ticket_id {
                    eprintln!("warning: {id} — ticket not found or inaccessible");
                }
                continue;
            }

            let assignee = issue_val
                .get("assignee")
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str());

            let mut has_pr = false;
            if let Some(attachments) = issue_val.get("attachments")
                && let Some(nodes) = attachments.get("nodes").and_then(|n| n.as_array())
            {
                for node in nodes {
                    if let Some(url) = node.get("url").and_then(|u| u.as_str())
                        && let Some(pr_number) = extract_pr_number(url)
                    {
                        has_pr = true;
                        if seen.insert(pr_number) {
                            result.push(pr_number);
                            if let Some(limit) = params.limit
                                && result.len() >= limit
                            {
                                return Ok(result);
                            }
                        }
                    }
                }
            }

            if !has_pr
                && let Some(id) = ticket_id
            {
                match assignee {
                    Some(name) => eprintln!("warning: {id} — no associated PRs found (assignee: {name})"),
                    None => eprintln!("warning: {id} — no associated PRs found (unassigned)"),
                }
            }
        }
    }

    Ok(result)
}

fn build_alias_query(ticket_ids: &[String]) -> String {
    let fields: Vec<String> = ticket_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let escaped_id = id.replace('"', "\\\"");
            format!(
                r#"i{i}: issue(id: "{escaped_id}") {{ assignee {{ name }} attachments {{ nodes {{ url }} }} }}"#
            )
        })
        .collect();

    format!("{{ {} }}", fields.join(" "))
}

fn extract_pr_number(url: &str) -> Option<u64> {
    let parts: Vec<&str> = url.split('/').collect();
    // Expected: https://github.com/{owner}/{repo}/pull/{number}
    if parts.len() >= 2 {
        let second_last = parts[parts.len() - 2];
        if second_last == "pull" {
            return parts.last().and_then(|s| s.parse::<u64>().ok());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pr_number_full_url() {
        assert_eq!(
            extract_pr_number("https://github.com/acme/widgets/pull/42"),
            Some(42)
        );
    }

    #[test]
    fn test_extract_pr_number_trailing_slash() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/999/"),
            None
        );
    }

    #[test]
    fn test_extract_pr_number_not_a_pr() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/issues/10"),
            None
        );
    }

    #[test]
    fn test_extract_pr_number_non_numeric() {
        assert_eq!(
            extract_pr_number("https://github.com/org/repo/pull/abc"),
            None
        );
    }

    #[test]
    fn test_build_issue_filter_empty() {
        let params = GetTicketsParams {
            api_key: "",
            labels: &[],
            statuses: &[],
            assignees: &[],
            limit: None,
        };
        assert_eq!(build_issue_filter(&params), "{}");
    }

    #[test]
    fn test_build_issue_filter_labels_only() {
        let labels = vec!["v1.0".to_string(), "rc-2026".to_string()];
        let params = GetTicketsParams {
            api_key: "",
            labels: &labels,
            statuses: &[],
            assignees: &[],
            limit: None,
        };
        let filter = build_issue_filter(&params);
        assert!(filter.contains(r#""v1.0""#));
        assert!(filter.contains(r#""rc-2026""#));
        assert!(filter.contains("labels:"));
        assert!(!filter.contains("state:"));
    }

    #[test]
    fn test_build_issue_filter_all_fields() {
        let labels = vec!["bug".to_string()];
        let statuses = vec!["Done".to_string()];
        let assignees = vec!["Alice".to_string()];
        let params = GetTicketsParams {
            api_key: "",
            labels: &labels,
            statuses: &statuses,
            assignees: &assignees,
            limit: None,
        };
        let filter = build_issue_filter(&params);
        assert!(filter.contains("labels:"));
        assert!(filter.contains("state:"));
        assert!(filter.contains("assignee:"));
    }

    #[test]
    fn test_json_string_array() {
        let values = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(json_string_array(&values), r#"["hello", "world"]"#);
    }

    #[test]
    fn test_json_string_array_with_quotes() {
        let values = vec![r#"say "hi""#.to_string()];
        assert_eq!(json_string_array(&values), r#"["say \"hi\""]"#);
    }

    #[test]
    fn test_build_alias_query_single() {
        let ids = vec!["ABC-123".to_string()];
        let query = build_alias_query(&ids);
        assert!(query.contains(r#"i0: issue(id: "ABC-123")"#));
        assert!(query.contains("attachments"));
    }

    #[test]
    fn test_build_alias_query_multiple() {
        let ids = vec!["ABC-1".to_string(), "DEF-2".to_string()];
        let query = build_alias_query(&ids);
        assert!(query.contains("i0:"));
        assert!(query.contains("i1:"));
        assert!(query.contains("ABC-1"));
        assert!(query.contains("DEF-2"));
    }
}
