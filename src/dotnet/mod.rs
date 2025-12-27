#![allow(dead_code)]

use crate::error::AppError;
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
            .spawn()?;

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

        serde_json::from_slice(&output.stdout).map_err(|err| AppError::Unknown(err.to_string()))
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
        if response.status != "ok" {
            let message = response
                .error
                .as_ref()
                .map(|error| format!("{}: {}", error.category, error.message))
                .unwrap_or_else(|| "Unknown .NET error".to_string());
            return Err(AppError::Unknown(message));
        }
        let payload = response
            .payload
            .ok_or_else(|| AppError::Unknown("Missing dialog response payload".to_string()))?;
        serde_json::from_value(payload).map_err(|err| AppError::Unknown(err.to_string()))
    }
}

static REQUEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn next_request_id() -> String {
    let id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{id:04}")
}
