use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

// Utilise le cache thread-local pour les opérations en lecture seule
use crate::git::repo_cache::with_cached_repo;

/// Default timeout for git operations (5 minutes).
const GIT_OPERATION_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Merge,
    Rebase,
}

#[derive(Debug, Clone)]
pub struct MergeOutcome {
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

    Ok(MergeOutcome {
        had_conflicts: !conflicts.is_empty(),
        conflicts,
    })
}

pub fn detect_conflicts<P: AsRef<Path>>(repo_path: P) -> Result<Vec<String>, git2::Error> {
    // Détection des conflits en lecture seule — utilise le cache thread-local
    with_cached_repo(repo_path, |repo| {
        let mut conflicts = Vec::new();
        if let Ok(mut index) = repo.index() {
            // The cached repository keeps its index: reload it after `git merge` wrote one.
            index.read(false)?;
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
    })
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

    // Drain both pipes while git runs: a full pipe buffer would block it forever.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let collect = |pipe: Option<JoinHandle<Vec<u8>>>| {
        pipe.and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    };

    // Wait for the process with timeout
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(std::process::Output {
                    status,
                    stdout: collect(stdout),
                    stderr: collect(stderr),
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

/// Reads `pipe` to the end on a separate thread.
fn drain(mut pipe: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_larger_than_a_pipe_buffer_does_not_block_git() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let big = "x".repeat(200 * 1024);
        let blob = repo.blob(big.as_bytes()).unwrap().to_string();

        let output = run_git_command_with_timeout(
            dir.path(),
            "cat-file",
            &["-p", &blob],
            Duration::from_secs(30),
        )
        .expect("git cat-file");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), big.len());
    }
}
