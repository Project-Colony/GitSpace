//! Application error types with user-friendly messages.
//!
//! This module provides a unified error type for the application with
//! automatic conversion from common error types and user-friendly messages.

use std::path::PathBuf;

use thiserror::Error;

use crate::i18n::{t, tf};

/// Application-level error type with user-friendly messages, in the interface language. The
/// detail is GitSpace's own sentence, translated where it is written, or the library's text.
#[derive(Debug, Clone, Error)]
pub enum AppError {
    /// Git operation failed.
    #[error("{}", tf("error_git_detail", &[("detail", .0.as_str())]))]
    Git(String),

    /// Network request failed.
    #[error("{}", tf("error_network_detail", &[("detail", .0.as_str())]))]
    Network(String),

    /// File system operation failed.
    #[error("{}", tf("error_io_detail", &[("detail", .0.as_str())]))]
    Io(String),

    /// Input validation failed.
    #[error("{}", tf("error_validation_detail", &[("detail", .0.as_str())]))]
    Validation(String),

    /// Configuration error.
    #[error("{}", tf("error_config_detail", &[("detail", .0.as_str())]))]
    Config(String),

    /// Unexpected error.
    #[error("{}", tf("error_unknown_detail", &[("detail", .0.as_str())]))]
    Unknown(String),
}

impl AppError {
    /// Returns a user-friendly message suitable for display.
    pub fn user_message(&self) -> String {
        let key = match self {
            Self::Git(_) => "error_git",
            Self::Network(_) => "error_network",
            Self::Io(_) => "error_io",
            Self::Validation(_) => "error_validation",
            Self::Config(_) => "error_config",
            Self::Unknown(_) => "error_unknown",
        };
        t(key).to_string()
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
            Self::Network(tf("error_http_status", &[("error", &value.to_string())]))
        } else if value.is_timeout() {
            Self::Network(t("error_timed_out").to_string())
        } else if value.is_connect() {
            Self::Network(t("error_connect").to_string())
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
        Self::Config(tf("error_json", &[("error", &value.to_string())]))
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
    let path = crate::paths::effective_dirs().1.join("logs");
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
        assert_eq!(msg, t("error_network"));
        assert!(!msg.contains("connection refused"));
    }

    #[test]
    fn messages_come_from_the_label_table() {
        // The French table has the same keys (checked in i18n), so French shows French.
        let err = AppError::Git("libgit2 text".to_string());
        assert_eq!(err.user_message(), t("error_git"));
        assert_eq!(
            err.to_string(),
            tf("error_git_detail", &[("detail", "libgit2 text")])
        );
        assert_ne!(t("error_git_detail"), "error_git_detail");
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
