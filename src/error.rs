//! Application error types with user-friendly messages.
//!
//! This module provides a unified error type for the application with
//! automatic conversion from common error types and user-friendly messages.

use std::path::PathBuf;

use thiserror::Error;

/// Application-level error type with user-friendly messages.
#[derive(Debug, Clone, Error)]
pub enum AppError {
    /// Git operation failed.
    #[error("Git operation failed: {0}")]
    Git(String),

    /// Network request failed.
    #[error("Network request failed: {0}")]
    Network(String),

    /// File system operation failed.
    #[error("I/O operation failed: {0}")]
    Io(String),

    /// Input validation failed.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Unexpected error.
    #[error("Unexpected error: {0}")]
    Unknown(String),
}

impl AppError {
    /// Returns a user-friendly message suitable for display.
    pub fn user_message(&self) -> String {
        match self {
            Self::Git(_) => "Git operation failed. Please check the repository status.".to_string(),
            Self::Network(_) => {
                "Network request failed. Check your connection or proxy settings.".to_string()
            }
            Self::Io(_) => {
                "File system operation failed. Verify permissions and disk space.".to_string()
            }
            Self::Validation(_) => {
                "The provided input is not valid. Please double-check and try again.".to_string()
            }
            Self::Config(_) => "Configuration error. Please check your settings.".to_string(),
            Self::Unknown(_) => "An unexpected error occurred.".to_string(),
        }
    }

    /// Returns the technical detail of the error.
    pub fn detail(&self) -> &str {
        match self {
            Self::Git(msg)
            | Self::Network(msg)
            | Self::Io(msg)
            | Self::Validation(msg)
            | Self::Config(msg)
            | Self::Unknown(msg) => msg,
        }
    }
}

impl From<git2::Error> for AppError {
    fn from(value: git2::Error) -> Self {
        Self::Git(value.message().to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        if value.is_status() {
            Self::Network(format!("HTTP error: {}", value))
        } else if value.is_timeout() {
            Self::Network("The request timed out.".to_string())
        } else if value.is_connect() {
            Self::Network("Failed to connect to server.".to_string())
        } else {
            Self::Network(value.to_string())
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Config(format!("JSON parse error: {}", value))
    }
}

impl From<String> for AppError {
    fn from(value: String) -> Self {
        Self::Unknown(value)
    }
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        Self::Unknown(value.to_string())
    }
}

/// Returns the application logs directory, creating it if necessary.
pub fn logs_directory() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = base.join("gitspace").join("logs");
    if let Err(err) = std::fs::create_dir_all(&path) {
        tracing::warn!(error = %err, "failed to create logs directory");
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_detail() {
        let err = AppError::Git("repository not found".to_string());
        let display = format!("{}", err);
        assert!(display.contains("repository not found"));
    }

    #[test]
    fn user_message_is_friendly() {
        let err = AppError::Network("connection refused".to_string());
        let msg = err.user_message();
        assert!(msg.contains("Network"));
        assert!(!msg.contains("connection refused"));
    }

    #[test]
    fn detail_returns_inner_message() {
        let err = AppError::Validation("email is invalid".to_string());
        assert_eq!(err.detail(), "email is invalid");
    }

    #[test]
    fn from_git2_error() {
        let err: AppError = git2::Error::from_str("merge conflict").into();
        assert!(matches!(err, AppError::Git(ref msg) if msg == "merge conflict"));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: AppError = io_err.into();
        assert!(matches!(err, AppError::Io(_)));
    }

    #[test]
    fn from_string() {
        let err: AppError = "something went wrong".into();
        assert!(matches!(err, AppError::Unknown(_)));
    }
}
