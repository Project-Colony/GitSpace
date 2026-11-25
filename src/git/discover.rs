use std::path::{Path, PathBuf};

/// Direct sub-folders of `root` that hold a `.git` directory or file, sorted by name.
/// Nested folders are not searched. An unreadable `root` gives an empty list.
#[cfg_attr(not(test), expect(dead_code, reason = "the sidebar scan will call it"))]
pub fn discover_repositories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir() && path.join(".git").exists())
        .collect();
    found.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    found
}
