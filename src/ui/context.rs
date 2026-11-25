use std::path::Path;
use std::sync::Arc;

use iced::Task;

use crate::git::repo_cache::with_cached_repo;

/// Runs `job` for the repository at `repo` on the blocking thread pool and passes its result,
/// with the repository path, to `done`. Panels compare that path with their current repository
/// and drop results that arrive after a switch. Shared by the changes, branch and history panels.
pub(crate) fn git_task<M, T>(
    repo: Arc<str>,
    job: impl FnOnce(&str) -> Result<T, String> + Send + 'static,
    done: impl Fn(Arc<str>, Result<T, String>) -> M + Send + 'static,
) -> Task<M>
where
    M: Send + 'static,
    T: Send + 'static,
{
    Task::perform(
        async move {
            let path = Arc::clone(&repo);
            let result = tokio::task::spawn_blocking(move || job(&path))
                .await
                .unwrap_or_else(|err| Err(err.to_string()));
            (repo, result)
        },
        move |(repo, result)| done(repo, result),
    )
}

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
