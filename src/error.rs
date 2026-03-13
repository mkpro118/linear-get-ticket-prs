use std::fmt;

#[derive(Debug)]
pub enum Error {
    ApiKeyNotFound,
    SubprocessFailed {
        command: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    JsonParse(serde_json::Error),
    GraphqlErrors(Vec<String>),
    #[allow(dead_code)]
    InvalidTicketId(String),
    InvalidPrNumber(String),
    EmptyInput,
    InvalidBranch(String),
    NoBranchDetected,
    MissingTool(String),
    NotAncestor { base: String, head: String },
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ApiKeyNotFound => write!(
                f,
                "Linear API key not found. Provide --api-key or set LINEAR_API_KEY"
            ),
            Error::SubprocessFailed {
                command,
                stderr,
                exit_code,
            } => {
                let code = exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                write!(f, "{command} exited with code {code}: {stderr}")
            }
            Error::JsonParse(e) => write!(f, "failed to parse JSON response: {e}"),
            Error::GraphqlErrors(errors) => {
                write!(f, "GraphQL errors: {}", errors.join("; "))
            }
            Error::InvalidTicketId(id) => {
                write!(f, "invalid ticket identifier: {id}")
            }
            Error::InvalidPrNumber(line) => {
                write!(f, "invalid PR number (expected positive integer): {line}")
            }
            Error::EmptyInput => write!(f, "no PR numbers provided on stdin"),
            Error::InvalidBranch(branch) => {
                write!(f, "branch does not exist as a local git ref: {branch}")
            }
            Error::NoBranchDetected => write!(
                f,
                "no --release-branch provided and current branch is not a release/* branch"
            ),
            Error::MissingTool(tool) => {
                write!(f, "required tool not found on PATH: {tool}")
            }
            Error::NotAncestor { base, head } => {
                write!(f, "{base} is not an ancestor of {head} — refusing to continue (will not silently swap arguments)")
            }
            Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::JsonParse(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
