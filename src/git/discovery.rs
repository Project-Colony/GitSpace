use std::path::{Path, PathBuf};

use git2::{Error, ErrorCode, Repository};
// Utilise le cache thread-local pour les opérations en lecture seule
use crate::git::repo_cache::with_cached_repo;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleEntry {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
}

#[allow(dead_code)]
pub fn is_git_repo<P: AsRef<Path>>(path: P) -> bool {
    Repository::discover(path).is_ok()
}

#[allow(dead_code)]
pub fn find_repo_root<P: AsRef<Path>>(path: P) -> Result<Option<PathBuf>, Error> {
    // Découvre la racine du dépôt puis utilise le cache thread-local
    let discovered = match Repository::discover(path) {
        Ok(repo) => repo,
        Err(err) => {
            if err.code() == ErrorCode::NotFound {
                return Ok(None);
            } else {
                return Err(err);
            }
        }
    };

    let git_dir = discovered.path().to_path_buf();
    let root = discovered
        .workdir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| git_dir);

    // Valide via le cache pour rester cohérent avec les autres opérations
    with_cached_repo(&root, |_repo| Ok(Some(root.clone())))
}

#[allow(dead_code)]
pub fn list_worktrees<P: AsRef<Path>>(repo_path: P) -> Result<Vec<String>, Error> {
    // Lecture seule des worktrees — utilise le cache thread-local
    with_cached_repo(repo_path, |repo| {
        let names = repo.worktrees()?;
        let mut entries = Vec::new();

        for name in names.iter().flatten() {
            if let Ok(worktree) = repo.find_worktree(name) {
                let path = worktree.path();
                entries.push(path.to_string_lossy().to_string());
            }
        }

        entries.sort();
        Ok(entries)
    })
}

#[allow(dead_code)]
pub fn list_submodules<P: AsRef<Path>>(repo_path: P) -> Result<Vec<SubmoduleEntry>, Error> {
    // Lecture seule des sous-modules — utilise le cache thread-local
    with_cached_repo(repo_path, |repo| {
        let submodules = repo.submodules()?;
        let mut entries = Vec::new();

        for submodule in submodules {
            let name = submodule
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    submodule
                        .path()
                        .to_string_lossy()
                        .to_string()
                });
            let path = submodule.path().to_string_lossy().to_string();
            let url = submodule.url().map(str::to_string);
            entries.push(SubmoduleEntry { name, path, url });
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    })
}
