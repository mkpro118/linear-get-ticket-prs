# Orchestrator (default mode)

```
Find Linear tickets, fetch their GitHub PRs, and filter merged ones

Usage: linear-get-ticket-prs [OPTIONS]
       linear-get-ticket-prs <COMMAND>

Commands:
  get-tickets    Returns ticket identifiers matching specified filters, one per line
  get-prs        Fetches GitHub PR numbers for tickets provided on stdin, one per line
  filter-merged  Filters stdin PR numbers/URLs to only those in MERGED status
  missing-prs    Find PRs present on main but missing from a release branch
  release-notes  Generate human-readable release notes between two git refs
  completions    Generate shell completions and print to stdout
  help           Print this message or the help of the given subcommand(s)

Options:
  -l, --label <LABELS>
          
  -s, --status <STATUSES>
          
  -a, --assignee <ASSIGNEES>
          
      --limit-tickets <LIMIT_TICKETS>
          
      --limit-prs <LIMIT_PRS>
          
  -r, --repo <REPO>
          
  -k, --api-key <API_KEY>
          
  -b, --release-branch <RELEASE_BRANCH>
          Run missing-prs analysis against this release branch after filter-merged
  -h, --help
          Print help (see more with '--help')

```
