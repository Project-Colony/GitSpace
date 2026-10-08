use std::path::Path;
use std::sync::Arc;

use crate::git::repo_cache::with_cached_repo;

/// Contexte d'un depot git actuellement ouvert.
/// Contient le chemin, le nom, la branche courante,
/// le statut dirty/clean et le remote upstream.
/// Le path utilise Arc<str> pour eviter les clones couteux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoContext {
    path: Arc<str>,
    pub name: String,
    /// Branche courante (ex: "main"). None si HEAD detache.
    pub current_branch: Option<String>,
    /// true si le working tree contient des modifications non commitees.
    pub is_dirty: bool,
    /// Nom du remote upstream (ex: "origin"). None si aucun remote.
    pub upstream_remote: Option<String>,
}

impl RepoContext {
    /// Retourne le chemin du depot sous forme de &str.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Retourne une copie du chemin (clone peu couteux grace a Arc).
    pub fn path_arc(&self) -> Arc<str> {
        Arc::clone(&self.path)
    }

    /// Construit un RepoContext a partir d'un chemin.
    /// Retourne None si le chemin n'est pas un depot git valide.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        let path_ref = path.as_ref();
        let path_string: Arc<str> = path_ref.to_string_lossy().into();
        let name = path_ref
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&path_string)
            .to_string();

        // Valider que le chemin est un depot git et extraire les metadonnees
        let (current_branch, is_dirty, upstream_remote) =
            with_cached_repo(path_ref, |repo| {
                // Branche courante
                let branch = repo
                    .head()
                    .ok()
                    .and_then(|head| head.shorthand().map(|s| s.to_string()));

                // Statut dirty : verifier s'il y a des modifications
                let dirty = repo
                    .statuses(Some(
                        git2::StatusOptions::new()
                            .include_untracked(true)
                            .exclude_submodules(true),
                    ))
                    .map(|statuses| !statuses.is_empty())
                    .unwrap_or(false);

                // Remote upstream de la branche courante
                let upstream = branch.as_deref().and_then(|b| {
                    repo.find_branch(b, git2::BranchType::Local)
                        .ok()
                        .and_then(|local| {
                            local
                                .upstream()
                                .ok()
                                .and_then(|u| u.name().ok().flatten().map(|s| {
                                    // Extraire le nom du remote (ex: "origin/main" -> "origin")
                                    s.split('/').next().unwrap_or(s).to_string()
                                }))
                        })
                });

                Ok((branch, dirty, upstream))
            })
            .ok()?;

        Some(Self {
            path: path_string,
            name,
            current_branch,
            is_dirty,
            upstream_remote,
        })
    }
}
