#![allow(dead_code)]

use crate::error::AppError;
use crate::telemetry::{log_dotnet_helper_launch_failure, log_dotnet_json_parse_error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize)]
pub struct DotnetRequest {
    pub id: String,
    pub command: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct DotnetResponse {
    pub id: String,
    pub status: String,
    pub payload: Option<Value>,
    pub error: Option<DotnetError>,
}

#[derive(Debug, Deserialize)]
pub struct DotnetError {
    pub category: String,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct DialogOpenRequest {
    pub kind: String,
    pub title: Option<String>,
    pub filters: Vec<DialogFilter>,
    pub options: DialogOptions,
}

#[derive(Debug, Serialize)]
pub struct DialogFilter {
    pub label: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DialogOptions {
    pub multi_select: bool,
    pub show_hidden: bool,
}

#[derive(Debug, Deserialize)]
pub struct DialogOpenResponse {
    pub selected_paths: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Serialize)]
pub struct CredentialRequest {
    pub service: String,
    pub account: Option<String>,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub struct CredentialResponse {
    pub username: Option<String>,
    pub secret: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct LibraryCallRequest {
    pub name: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct LibraryCallResponse {
    pub payload: Value,
}

pub struct DotnetClient {
    program: PathBuf,
    args: Vec<String>,
}

impl DotnetClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn helper() -> Self {
        Self::new("dotnet").with_args([
            "run",
            "--project",
            "dotnet/GitSpace.Helper/GitSpace.Helper.csproj",
        ])
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(|arg| arg.into()).collect();
        self
    }

    pub fn send_request(&self, request: &DotnetRequest) -> Result<DotnetResponse, AppError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                log_dotnet_helper_launch_failure(&err);
                AppError::from(err)
            })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Unknown("failed to open stdin".to_string()))?;
        serde_json::to_writer(&mut stdin, request)
            .map_err(|err| AppError::Unknown(err.to_string()))?;
        stdin.write_all(b"\n")?;
        drop(stdin);

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Unknown(format!(
                "dotnet process failed: {}",
                stderr.trim()
            )));
        }

        serde_json::from_slice(&output.stdout).map_err(|err| {
            log_dotnet_json_parse_error(&err, "dotnet_response");
            AppError::Unknown(err.to_string())
        })
    }

    pub fn dialog_open(
        &self,
        payload: DialogOpenRequest,
    ) -> Result<DialogOpenResponse, AppError> {
        let request = DotnetRequest {
            id: next_request_id(),
            command: "dialog.open".to_string(),
            payload: serde_json::to_value(payload)
                .map_err(|err| AppError::Unknown(err.to_string()))?,
        };
        let response = self.send_request(&request)?;
        let payload = response_payload(response, "dialog response payload")?;
        serde_json::from_value(payload).map_err(|err| {
            log_dotnet_json_parse_error(&err, "dialog_open_payload");
            AppError::Unknown(err.to_string())
        })
    }

    pub fn credential_request(
        &self,
        payload: CredentialRequest,
    ) -> Result<CredentialResponse, AppError> {
        let request = DotnetRequest {
            id: next_request_id(),
            command: "credential.request".to_string(),
            payload: serde_json::to_value(payload)
                .map_err(|err| AppError::Unknown(err.to_string()))?,
        };
        let response = self.send_request(&request)?;
        let payload = response_payload(response, "credential response payload")?;
        serde_json::from_value(payload).map_err(|err| {
            log_dotnet_json_parse_error(&err, "credential_payload");
            AppError::Unknown(err.to_string())
        })
    }

    pub fn library_call(
        &self,
        payload: LibraryCallRequest,
    ) -> Result<LibraryCallResponse, AppError> {
        let request = DotnetRequest {
            id: next_request_id(),
            command: "library.call".to_string(),
            payload: serde_json::to_value(payload)
                .map_err(|err| AppError::Unknown(err.to_string()))?,
        };
        let response = self.send_request(&request)?;
        let payload = response_payload(response, "library response payload")?;
        Ok(LibraryCallResponse { payload })
    }
}

static REQUEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn next_request_id() -> String {
    let id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{id:04}")
}

fn map_dotnet_error(error: &DotnetError) -> AppError {
    let mut message = format!("{}: {}", error.category, error.message);
    if let Some(details) = &error.details {
        message = format!("{message} ({details})");
    }

    match error.category.as_str() {
        "InvalidRequest" => AppError::Validation(message),
        "Internal" => AppError::Unknown(message),
        _ => AppError::Unknown(message),
    }
}

fn response_payload(response: DotnetResponse, context: &str) -> Result<Value, AppError> {
    match response.status.as_str() {
        "ok" => response.payload.ok_or_else(|| {
            AppError::Unknown(format!("Missing {context}"))
        }),
        "error" => Err(response
            .error
            .as_ref()
            .map(map_dotnet_error)
            .unwrap_or_else(|| AppError::Unknown("Unknown .NET error".to_string()))),
        _ => Err(AppError::Unknown(format!(
            "Unexpected .NET status '{}' for {context}",
            response.status
        ))),
    }
}

#[cfg(test)]
mod tests;
