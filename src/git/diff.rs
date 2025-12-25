use std::collections::HashMap;

use git2::{DiffFormat, Oid, Repository};

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub patch: String,
}

pub fn commit_diff(repo_path: &str, oid: &str) -> Result<Vec<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let oid = Oid::from_str(oid)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    let parent_tree = if let Ok(parent) = commit.parent(0) {
        Some(parent.tree()?)
    } else {
        None
    };

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
    let mut files: HashMap<String, FileDiff> = HashMap::new();

    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("(unknown)")
            .to_string();

        let entry = files.entry(path.clone()).or_insert_with(|| FileDiff {
            path: path.clone(),
            additions: 0,
            deletions: 0,
            patch: String::new(),
        });

        match line.origin() {
            '+' => entry.additions += 1,
            '-' => entry.deletions += 1,
            _ => {}
        }

        let content = std::str::from_utf8(line.content()).unwrap_or("");
        match line.origin() {
            '\\' => entry.patch.push(' '),
            other => entry.patch.push(other),
        }
        entry.patch.push_str(content);

        true
    })?;

    // For files without textual output (e.g., binary), gather stats separately.
    let stats = diff.stats()?;
    if stats.files_changed() > files.len() {
        for delta in diff.deltas() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .and_then(|p| p.to_str())
                .unwrap_or("(unknown)")
                .to_string();

            files.entry(path.clone()).or_insert_with(|| FileDiff {
                path: path.clone(),
                additions: 0,
                deletions: 0,
                patch: String::from("Binary file change\n"),
            });
        }
    }

    Ok(files.into_values().collect())
}
