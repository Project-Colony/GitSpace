use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum AppError {
    Git(String),
    Network(String),
    Io(String),
    Validation(String),
    Unknown(String),
}

impl AppError {
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
            Self::Unknown(_) => "An unexpected error occurred.".to_string(),
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Git(msg)
            | Self::Network(msg)
            | Self::Io(msg)
            | Self::Validation(msg)
            | Self::Unknown(msg) => msg,
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.user_message(), self.detail())
    }
}

impl std::error::Error for AppError {}

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

pub fn logs_directory() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = base.join("gitspace").join("logs");
    let _ = std::fs::create_dir_all(&path);
    path
}
