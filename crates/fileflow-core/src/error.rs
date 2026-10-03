use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

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

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("integrity: {0}")]
    Integrity(String),

    #[error("cancelled")]
    Cancelled,

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("other: {0}")]
    Other(String),
}

impl From<time::error::Format> for Error {
    fn from(e: time::error::Format) -> Self {
        Self::Time(e.to_string())
    }
}
