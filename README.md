# linear-get-ticket-prs

A CLI toolkit for bridging [Linear](https://linear.app) issue tracking with GitHub pull requests.

Queries Linear tickets, fetches their associated GitHub PRs, filters by merge status, compares release branches, and generates release notes — all composable via Unix pipes.

## Prerequisites

- `curl` — Linear API calls
- [`gh`](https://cli.github.com) — GitHub PR status checks
- `git` — branch analysis and release notes
- [`rg`](https://github.com/BurntSushi/ripgrep) (ripgrep) — `missing-prs` pipeline
- `column` — aligned table output for `missing-prs`

## Installation

```sh
cargo install --git https://github.com/mkpro118/linear-get-ticket-prs
```

## Usage

### Orchestrator (default mode)

Chains the full pipeline in one command: get-tickets → get-prs → filter-merged, and optionally missing-prs if a release branch is provided or auto-detected.

```sh
linear-get-ticket-prs \
  --label "my-release-label" \
  --status "Done" \
  --repo my-org/my-repo

# With release branch analysis
linear-get-ticket-prs \
  --label "my-release-label" \
  --release-branch release/v1.2.3
```

### Subcommands

Each subcommand is also pipe-friendly on its own.

#### `get-tickets`

Queries Linear for issues matching filters and prints ticket identifiers.

```sh
linear-get-ticket-prs get-tickets --label "my-label" --status "Done"
# Output: ENG-123, ENG-456, ...
```

Requires a Linear API key via `--api-key` or the `LINEAR_API_KEY` environment variable.

#### `get-prs`

Reads Linear ticket identifiers from stdin and prints associated GitHub PR numbers.

```sh
echo "ENG-123" | linear-get-ticket-prs get-prs
# Output: 456, 789, ...

# Warns on stderr for tickets with no PRs:
# warning: ENG-999 — no associated PRs found (assignee: Jane Doe)
```

#### `filter-merged`

Reads PR numbers or GitHub PR URLs from stdin and prints only the merged ones.

```sh
echo "456" | linear-get-ticket-prs filter-merged --repo my-org/my-repo
```

#### Full pipeline via pipes

```sh
linear-get-ticket-prs get-tickets --label "v1.2" \
  | linear-get-ticket-prs get-prs \
  | linear-get-ticket-prs filter-merged --repo my-org/my-repo
```

To audit which tickets have no PRs (stderr only):

```sh
linear-get-ticket-prs get-tickets --label "v1.2" \
  | linear-get-ticket-prs get-prs 2>&1 1>/dev/null | pbcopy
```

#### `missing-prs`

Compares the first-parent history of `main` against a release branch to find PRs that are effectively present on main but absent from the release. Outputs a table and a ready-to-paste `git cherry-pick` command.

```sh
echo "456 789 101" | tr ' ' '\n' \
  | linear-get-ticket-prs missing-prs --release-branch release/v1.2.3
```

If `--release-branch` is omitted, the current branch is used if it matches `release/v*`.

#### `release-notes`

Generates Markdown-style release notes from the git first-parent log between two refs. Runs fully offline with no API calls.

```sh
linear-get-ticket-prs release-notes \
  --base v1.1.0 \
  --head v1.2.0 \
  --config-key rn.authors \
  --repo-name my-org
```

Output format:

```
* Fix login timeout by @johndoe in #789
* Add dark mode support by @janedoe in #456
```

Author handles are resolved from `git config` using the `--config-key` namespace:

```sh
git config rn.authors.john johndoe
git config rn.authors.jane janedoe
```

#### `completions`

Generates shell completion scripts.

```sh
# Zsh
linear-get-ticket-prs completions zsh \
  > /opt/homebrew/share/zsh/site-functions/_linear-get-ticket-prs

# Bash, fish, elvish, powershell also supported
linear-get-ticket-prs completions bash > ~/.bash_completion.d/linear-get-ticket-prs
```

## Documentation

### API docs

```sh
cargo doc --no-deps --open
```

### Subcommand reference

Markdown docs for each subcommand live in [`docs/`](docs/):

- [Orchestrator](docs/orchestrator.md)
- [get-tickets](docs/get-tickets.md)
- [get-prs](docs/get-prs.md)
- [filter-merged](docs/filter-merged.md)
- [missing-prs](docs/missing-prs.md)
- [release-notes](docs/release-notes.md)
- [completions](docs/completions.md)

To regenerate them after code changes:

```sh
cargo run --bin gen_docs
```
