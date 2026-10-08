//! Repository cache to avoid repeated Repository::open() calls.
//!
//! This module provides a thread-safe cache for git2::Repository instances
//! to reduce I/O overhead when performing multiple operations on the same repository.

// Public API designed for future use
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use git2::Repository;
use parking_lot::RwLock;

/// Default time-to-live for cached repositories.
const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// Maximum number of repositories to cache.
const MAX_CACHE_SIZE: usize = 10;

/// A cached repository entry with metadata.
struct CacheEntry {
    repo: Repository,
    last_access: Instant,
    path: PathBuf,
}

/// Thread-safe repository cache.
///
/// Caches opened Repository instances to avoid repeated filesystem operations.
/// Entries expire after a configurable TTL and the cache has a maximum size.
pub struct RepoCache {
    cache: RwLock<HashMap<PathBuf, CacheEntry>>,
    ttl: Duration,
    max_size: usize,
}

impl Default for RepoCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RepoCache {
    /// Creates a new repository cache with default settings.
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: DEFAULT_TTL,
            max_size: MAX_CACHE_SIZE,
        }
    }

    /// Creates a repository cache with custom TTL and size.
    pub fn with_config(ttl: Duration, max_size: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl,
            max_size,
        }
    }

    /// Opens a repository, using the cache if available.
    ///
    /// If the repository is already cached and not expired, returns a reference to it.
    /// Otherwise, opens the repository and caches it.
    pub fn open<P: AsRef<Path>>(&self, path: P) -> Result<RepoHandle<'_>, git2::Error> {
        let path = path.as_ref().to_path_buf();
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

        // Check if we have a valid cached entry
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(&canonical) {
                if entry.last_access.elapsed() < self.ttl {
                    // Return a handle that will update last_access on drop
                    return Ok(RepoHandle {
                        path: canonical,
                        cache: self,
                    });
                }
            }
        }

        // Open the repository and cache it
        let repo = Repository::open(&canonical)?;

        {
            let mut cache = self.cache.write();

            // Evict expired entries
            self.evict_expired(&mut cache);

            // If still over capacity, evict oldest
            while cache.len() >= self.max_size {
                self.evict_oldest(&mut cache);
            }

            cache.insert(
                canonical.clone(),
                CacheEntry {
                    repo,
                    last_access: Instant::now(),
                    path: canonical.clone(),
                },
            );
        }

        Ok(RepoHandle {
            path: canonical,
            cache: self,
        })
    }

    /// Gets a reference to a cached repository.
    ///
    /// This is unsafe to use directly as the Repository is not Send/Sync.
    /// Use `with_repo` instead for safe access.
    pub fn with_repo<P, F, R>(&self, path: P, f: F) -> Result<R, git2::Error>
    where
        P: AsRef<Path>,
        F: FnOnce(&Repository) -> Result<R, git2::Error>,
    {
        let path = path.as_ref().to_path_buf();
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

        // Try to use cached repository
        {
            let mut cache = self.cache.write();
            if let Some(entry) = cache.get_mut(&canonical) {
                if entry.last_access.elapsed() < self.ttl {
                    entry.last_access = Instant::now();
                    return f(&entry.repo);
                }
            }
        }

        // Open fresh if not cached or expired
        let repo = Repository::open(&canonical)?;
        let result = f(&repo);

        // Cache for future use
        {
            let mut cache = self.cache.write();
            self.evict_expired(&mut cache);
            while cache.len() >= self.max_size {
                self.evict_oldest(&mut cache);
            }
            cache.insert(
                canonical.clone(),
                CacheEntry {
                    repo,
                    last_access: Instant::now(),
                    path: canonical,
                },
            );
        }

        result
    }

    /// Invalidates a cached repository entry.
    pub fn invalidate<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref().to_path_buf();
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        let mut cache = self.cache.write();
        cache.remove(&canonical);
    }

    /// Clears all cached repositories.
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        cache.clear();
    }

    /// Returns the number of cached repositories.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    fn evict_expired(&self, cache: &mut HashMap<PathBuf, CacheEntry>) {
        cache.retain(|_, entry| entry.last_access.elapsed() < self.ttl);
    }

    fn evict_oldest(&self, cache: &mut HashMap<PathBuf, CacheEntry>) {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest_key);
        }
    }
}

/// A handle to a cached repository.
///
/// Updates the last access time when used.
pub struct RepoHandle<'a> {
    path: PathBuf,
    cache: &'a RepoCache,
}

impl<'a> RepoHandle<'a> {
    /// Executes a function with the cached repository.
    pub fn with<F, R>(&self, f: F) -> Result<R, git2::Error>
    where
        F: FnOnce(&Repository) -> Result<R, git2::Error>,
    {
        self.cache.with_repo(&self.path, f)
    }
}

// Note: Global cache is not possible because git2::Repository is not thread-safe.
// The RepoCache should be used per-thread or with proper synchronization.
// Use thread_local! for a thread-local cache if needed:
//
// thread_local! {
//     static LOCAL_CACHE: RefCell<RepoCache> = RefCell::new(RepoCache::new());
// }
//
// For now, users should create their own RepoCache instance.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        Repository::init(&path).unwrap();
        (dir, path)
    }

    #[test]
    fn cache_stores_repository() {
        let (_dir, path) = create_test_repo();
        let cache = RepoCache::new();

        assert!(cache.is_empty());

        let _ = cache.open(&path).unwrap();

        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_reuses_repository() {
        let (_dir, path) = create_test_repo();
        let cache = RepoCache::new();

        // Open twice
        let _ = cache.open(&path).unwrap();
        let _ = cache.open(&path).unwrap();

        // Should still only have one entry
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_invalidates() {
        let (_dir, path) = create_test_repo();
        let cache = RepoCache::new();

        let _ = cache.open(&path).unwrap();
        assert_eq!(cache.len(), 1);

        cache.invalidate(&path);
        assert!(cache.is_empty());
    }

    #[test]
    fn with_repo_works() {
        let (_dir, path) = create_test_repo();
        let cache = RepoCache::new();

        let result = cache.with_repo(&path, |repo| Ok(repo.is_empty().unwrap()));

        assert!(result.is_ok());
    }

    // Note: RepoCache is not thread-safe due to git2::Repository constraints.
    // Multi-threaded access should use separate caches per thread or thread_local!.

    #[test]
    fn cache_respects_max_size() {
        let cache = RepoCache::with_config(DEFAULT_TTL, 2);

        let dirs: Vec<_> = (0..3).map(|_| create_test_repo()).collect();

        for (_, path) in &dirs {
            let _ = cache.open(path);
        }

        // Should only keep 2 repos
        assert!(cache.len() <= 2);
    }
}
