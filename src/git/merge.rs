use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use git2::Repository;

/// Default timeout for git operations (5 minutes).
const GIT_OPERATION_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Merge,
    Rebase,
}

#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub message: String,
    pub had_conflicts: bool,
    pub conflicts: Vec<String>,
}

pub fn merge_branch<P: AsRef<Path>>(
    repo_path: P,
    target: &str,
    strategy: MergeStrategy,
) -> Result<MergeOutcome, String> {
    let repo_path_ref = repo_path.as_ref();
    let (command, args) = match strategy {
        MergeStrategy::Merge => ("merge", vec!["--no-ff", "--no-edit", target]),
        MergeStrategy::Rebase => ("rebase", vec![target]),
    };

    let output =
        run_git_command_with_timeout(repo_path_ref, command, &args, GIT_OPERATION_TIMEOUT)?;

    let conflicts = detect_conflicts(repo_path_ref).map_err(|err| err.to_string())?;

    if !output.status.success() && conflicts.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Err(message);
    }

    let message = if output.status.success() {
        format!(
            "{} completed",
            match strategy {
                MergeStrategy::Merge => "Merge",
                MergeStrategy::Rebase => "Rebase",
            }
        )
    } else {
        String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()
            .if_empty_then(|| "Operation completed with conflicts".to_string())
    };

    Ok(MergeOutcome {
        message,
        had_conflicts: !conflicts.is_empty(),
        conflicts,
    })
}

pub fn detect_conflicts<P: AsRef<Path>>(repo_path: P) -> Result<Vec<String>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut conflicts = Vec::new();
    if let Ok(index) = repo.index() {
        if index.has_conflicts() {
            for conflict in index.conflicts()?.flatten() {
                if let Some(name) = conflict
                    .our
                    .as_ref()
                    .or(conflict.their.as_ref())
                    .or(conflict.ancestor.as_ref())
                    .and_then(|entry| std::str::from_utf8(&entry.path).ok())
                {
                    conflicts.push(name.to_string());
                }
            }
        }
    }
    Ok(conflicts)
}

trait EmptyStringExt {
    fn if_empty_then(self, alt: impl FnOnce() -> String) -> String;
}

impl EmptyStringExt for String {
    fn if_empty_then(self, alt: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            alt()
        } else {
            self
        }
    }
}

/// Runs a git command with a timeout.
///
/// Returns the command output or an error if the command fails or times out.
fn run_git_command_with_timeout(
    repo_path: &Path,
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = Command::new("git")
        .arg(command)
        .args(args)
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start git {command}: {err}"))?;

    // Wait for the process with timeout
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited, collect output
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                // Still running, check timeout
                if start.elapsed() >= timeout {
                    // Kill the process
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Git {command} timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                // Sleep briefly before checking again
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                return Err(format!("Failed to wait for git {command}: {err}"));
            }
        }
    }
}
