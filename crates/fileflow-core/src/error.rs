use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(std::io::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("regex: {0}")]
    Regex(#[from] regex::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("time: {0}")]
    Time(String),

    #[error("safety: {0}")]
    Safety(String),

    #[error("invalid filename: {0}")]
    InvalidFilename(String),

    #[error("invalid rule: {0}")]
    InvalidRule(String),

    #[error("not found: {0}")]
    NotFound(String),

    /// The collision policy said `Skip` and the destination exists.
    #[error("skipped: {0}")]
    Skipped(String),

    /// The collision policy said `Ask`; a human has to decide.
    #[error("needs input: {0}")]
    NeedsInput(String),

    #[error("integrity: {0}")]
    Integrity(String),

    #[error("cancelled")]
    Cancelled,

    #[error("other: {0}")]
    Other(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            Error::Permission(e.to_string())
        } else {
            Error::Io(e)
        }
    }
}
