#![allow(dead_code)]

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize)]
pub struct DotnetRequest {
    pub command: String,
}

#[derive(Debug, Deserialize)]
pub struct DotnetResponse {
    pub status: String,
    pub message: String,
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
}
