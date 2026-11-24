use std::path::Path;
use std::sync::Arc;

use crate::git::repo_cache::with_cached_repo;

/// Contexte d'un depot git actuellement ouvert : son chemin et son nom.
/// Le path utilise Arc<str> pour eviter les clones couteux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoContext {
    path: Arc<str>,
    pub name: String,
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

        // Ouvrir le depot suffit a valider le chemin.
        with_cached_repo(path_ref, |_| Ok(())).ok()?;

        Some(Self {
            path: path_string,
            name,
        })
    }
}
