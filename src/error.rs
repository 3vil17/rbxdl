use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("credential store error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no credentials saved, add an API key or a cookie in Settings")]
    NoCredentials,
    #[error("{0}")]
    Denied(String),
    #[error("rate limited by Roblox, every account is cooling down (retry after {0}s)")]
    RateLimited(u64),
    #[error("{0}")]
    Other(String),
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
