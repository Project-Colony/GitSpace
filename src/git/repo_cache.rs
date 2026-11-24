//! Thread-local cache of opened repositories, to avoid repeated `Repository::open()` calls.
//!
//! `git2::Repository` is not `Sync`, so each thread (the UI thread and every blocking task)
//! keeps its own cache. libgit2 reads refs and HEAD from disk on each lookup, but a cached
//! repository keeps its index object: code that reads the index calls `index.read(false)`
//! first so it sees changes made by other processes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use git2::Repository;

/// How long an unused repository stays cached.
const TTL: Duration = Duration::from_secs(60);

/// Maximum number of repositories cached per thread.
const MAX_ENTRIES: usize = 10;

type Cache = HashMap<PathBuf, (Rc<Repository>, Instant)>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// Calls `f` with the cached repository at `path`, opening it on first use.
pub fn with_cached_repo<P, F, R>(path: P, f: F) -> Result<R, git2::Error>
where
    P: AsRef<Path>,
    F: FnOnce(&Repository) -> Result<R, git2::Error>,
{
    let path = path.as_ref();
    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // The cache is only borrowed while looking up, so `f` can call this function again.
    let repo = CACHE.with(|cache| cached_or_open(&mut cache.borrow_mut(), key))?;
    f(&repo)
}

fn cached_or_open(cache: &mut Cache, key: PathBuf) -> Result<Rc<Repository>, git2::Error> {
    cache.retain(|_, (_, used)| used.elapsed() < TTL);
    if let Some((repo, used)) = cache.get_mut(&key) {
        *used = Instant::now();
        return Ok(Rc::clone(repo));
    }

    let repo = Rc::new(Repository::open(&key)?);
    if cache.len() >= MAX_ENTRIES {
        let oldest = cache
            .iter()
            .min_by_key(|(_, (_, used))| *used)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            cache.remove(&oldest);
        }
    }
    cache.insert(key, (Rc::clone(&repo), Instant::now()));
    Ok(repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_calls_reuse_the_cached_repository() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();

        let same = with_cached_repo(dir.path(), |outer| {
            with_cached_repo(dir.path(), |inner| Ok(std::ptr::eq(outer, inner)))
        })
        .unwrap();
        assert!(same);
    }

    #[test]
    fn the_cache_is_bounded() {
        let dirs: Vec<_> = (0..MAX_ENTRIES + 2)
            .map(|_| tempfile::tempdir().unwrap())
            .collect();
        for dir in &dirs {
            Repository::init(dir.path()).unwrap();
            with_cached_repo(dir.path(), |_| Ok(())).unwrap();
        }
        assert!(CACHE.with(|cache| cache.borrow().len()) <= MAX_ENTRIES);
    }
}
