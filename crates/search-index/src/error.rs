use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchIndexError {
    #[error("index not found at {0}")]
    MissingIndex(PathBuf),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("command failed: {0}")]
    Command(String),
}

impl From<bincode::error::EncodeError> for SearchIndexError {
    fn from(value: bincode::error::EncodeError) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<bincode::error::DecodeError> for SearchIndexError {
    fn from(value: bincode::error::DecodeError) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<serde_json::Error> for SearchIndexError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<regex::Error> for SearchIndexError {
    fn from(value: regex::Error) -> Self {
        Self::InvalidQuery(value.to_string())
    }
}
