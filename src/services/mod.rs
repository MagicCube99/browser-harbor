//! Application services and their stable error boundary.

pub mod archive;
pub mod download;
pub mod health;
pub mod http;
pub mod install;
pub mod trusted_metadata;
pub mod version_probe;

use std::path::PathBuf;

use thiserror::Error;

use crate::domain::release::Integrity;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("network request failed")]
    Network(#[source] reqwest::Error),
    #[error("remote server returned HTTP {status} for {host}")]
    Status { status: u16, host: String },
    #[error("response exceeds the allowed metadata size")]
    ResponseTooLarge,
    #[error("response body was invalid")]
    InvalidResponse(#[source] serde_json::Error),
    #[error("URL is invalid")]
    InvalidUrl,
    #[error("no download candidate succeeded")]
    AllCandidatesFailed,
    #[error("downloaded size does not match the release descriptor")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("downloaded artifact hash does not match")]
    HashMismatch { expected: String, actual: String },
    #[error("download was cancelled")]
    Cancelled,
    #[error("file operation failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("release cache is invalid")]
    Cache(#[source] serde_json::Error),
}

impl ServiceError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn integrity_mismatch(expected: &Integrity, actual: String) -> Self {
        Self::HashMismatch {
            expected: expected.value().to_owned(),
            actual,
        }
    }
}
