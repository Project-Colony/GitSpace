use std::path::Path;
use std::process::Command;

use git2::Repository;

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

    let output = Command::new("git")
        .arg(command)
        .args(args)
        .current_dir(repo_path_ref)
        .output()
        .map_err(|err| err.to_string())?;

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
    if let Ok(index) = repo.index()
        && index.has_conflicts()
    {
        for conflict in index.conflicts()? {
            if let Ok(conflict) = conflict {
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
        if self.is_empty() { alt() } else { self }
    }
}
