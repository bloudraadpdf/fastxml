//! File-based caching fetcher wrapper.
//!
//! Wraps any `SchemaFetcher` or `AsyncSchemaFetcher` with a temporary-file cache
//! so each URL is fetched at most once, while keeping memory usage low by storing
//! fetched content on disk instead of in a `DashMap`.

use std::path::{Path, PathBuf};

use dashmap::DashMap;
use xxhash_rust::xxh64;

use crate::error::Result;

use super::result::FetchResult;
use super::traits::SchemaFetcher;

/// Generates a cache file name from a URL using xxhash64.
fn cache_filename(url: &str) -> String {
    let hash = xxh64::xxh64(url.as_bytes(), 0);
    format!("{:016x}.xsd", hash)
}

struct FileCache {
    directory: PathBuf,
    _temp_dir: Option<tempfile::TempDir>,
    index: DashMap<String, PathBuf>,
}

impl FileCache {
    fn temporary() -> Result<Self> {
        Ok(Self::from_temp_dir(tempfile::TempDir::new()?))
    }

    fn persistent(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: directory.as_ref().to_path_buf(),
            _temp_dir: None,
            index: DashMap::new(),
        }
    }

    fn temporary_in(directory: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::from_temp_dir(tempfile::TempDir::new_in(directory)?))
    }

    fn from_temp_dir(temp_dir: tempfile::TempDir) -> Self {
        Self {
            directory: temp_dir.path().to_path_buf(),
            _temp_dir: Some(temp_dir),
            index: DashMap::new(),
        }
    }

    fn path_for(&self, url: &str) -> PathBuf {
        self.directory.join(cache_filename(url))
    }

    fn cached_path(&self, url: &str) -> Option<PathBuf> {
        self.index.get(url).map(|entry| entry.value().clone())
    }

    fn insert(&self, url: &str, path: PathBuf) {
        self.index.insert(url.to_string(), path);
    }

    fn len(&self) -> usize {
        self.index.len()
    }

    fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

macro_rules! file_cache_accessors {
    () => {
        /// Returns the number of cached entries.
        pub fn len(&self) -> usize {
            self.cache.len()
        }

        /// Returns `true` if the cache is empty.
        pub fn is_empty(&self) -> bool {
            self.cache.is_empty()
        }

        /// Returns a reference to the inner fetcher.
        pub fn inner(&self) -> &F {
            &self.inner
        }

        /// Returns the cache directory path.
        pub fn cache_dir(&self) -> &Path {
            &self.cache.directory
        }
    };
}

/// A fetcher wrapper that caches fetch results as files on disk.
///
/// When a URL is requested:
/// 1. Check the index — if present, read the cached file.
/// 2. Otherwise delegate to the inner fetcher.
/// 3. Write the result to a file and register it in the index
///    (under both the requested URL and the final URL if a redirect occurred).
///
/// # Cache directory lifecycle
///
/// - [`FileCachingFetcher::new`] creates a temporary directory that is
///   automatically deleted when the fetcher is dropped.
/// - [`FileCachingFetcher::with_dir`] uses an existing directory and does
///   **not** clean it up on drop (persistent cache).
/// - [`FileCachingFetcher::with_temp_dir`] creates a temporary directory
///   inside the given parent and cleans it up on drop.
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::fetcher::{FileCachingFetcher, DefaultFetcher};
///
/// let fetcher = FileCachingFetcher::new(DefaultFetcher::new())?;
/// let result = fetcher.fetch("http://example.com/schema.xsd")?;
/// // Second call reads from the file cache
/// let cached = fetcher.fetch("http://example.com/schema.xsd")?;
/// ```
pub struct FileCachingFetcher<F: SchemaFetcher> {
    inner: F,
    cache: FileCache,
}

impl<F: SchemaFetcher> FileCachingFetcher<F> {
    /// Creates a new file-caching fetcher with an auto-created temporary directory.
    ///
    /// The temporary directory is deleted when this fetcher is dropped.
    pub fn new(inner: F) -> Result<Self> {
        Ok(Self {
            inner,
            cache: FileCache::temporary()?,
        })
    }

    /// Creates a file-caching fetcher using an existing directory.
    ///
    /// The directory is **not** cleaned up when the fetcher is dropped,
    /// allowing it to serve as a persistent cache across runs.
    pub fn with_dir(inner: F, dir: impl AsRef<Path>) -> Self {
        Self {
            inner,
            cache: FileCache::persistent(dir),
        }
    }

    /// Creates a file-caching fetcher with a temporary directory inside `dir`.
    ///
    /// The temporary sub-directory is deleted when the fetcher is dropped.
    pub fn with_temp_dir(inner: F, dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            inner,
            cache: FileCache::temporary_in(dir)?,
        })
    }

    /// Pre-seeds the cache with content for a given URL.
    pub fn seed(&self, url: &str, content: Vec<u8>) -> Result<()> {
        let path = self.cache.path_for(url);
        std::fs::write(&path, &content)?;
        self.cache.insert(url, path);
        Ok(())
    }

    file_cache_accessors!();

    /// Writes content to a cache file and registers it in the index for the given URL.
    fn write_cache(&self, url: &str, content: &[u8]) -> Result<PathBuf> {
        let path = self.cache.path_for(url);
        std::fs::write(&path, content)?;
        self.cache.insert(url, path.clone());
        Ok(path)
    }
}

impl<F: SchemaFetcher> SchemaFetcher for FileCachingFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check index — read from file cache
        if let Some(path) = self.cache.cached_path(url) {
            let content = std::fs::read(path)?;
            return Ok(FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            });
        }

        // Delegate to inner
        let result = self.inner.fetch(url)?;

        // Write to file cache
        let path = self.write_cache(url, &result.content)?;

        // Also register under the final URL if a redirect occurred
        if result.final_url != url {
            self.cache.insert(&result.final_url, path);
        }

        Ok(result)
    }
}

/// Async version of [`FileCachingFetcher`].
///
/// Uses `tokio::fs` for file I/O so the cache operations don't block the
/// async runtime.
#[cfg(feature = "tokio")]
pub struct AsyncFileCachingFetcher<F: super::traits::AsyncSchemaFetcher> {
    inner: F,
    cache: FileCache,
}

#[cfg(feature = "tokio")]
impl<F: super::traits::AsyncSchemaFetcher> AsyncFileCachingFetcher<F> {
    /// Creates a new async file-caching fetcher with an auto-created temporary directory.
    pub fn new(inner: F) -> Result<Self> {
        Ok(Self {
            inner,
            cache: FileCache::temporary()?,
        })
    }

    /// Creates an async file-caching fetcher using an existing directory (persistent cache).
    pub fn with_dir(inner: F, dir: impl AsRef<Path>) -> Self {
        Self {
            inner,
            cache: FileCache::persistent(dir),
        }
    }

    /// Creates an async file-caching fetcher with a temporary directory inside `dir`.
    pub fn with_temp_dir(inner: F, dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            inner,
            cache: FileCache::temporary_in(dir)?,
        })
    }

    /// Pre-seeds the cache with content for a given URL.
    pub async fn seed(&self, url: &str, content: Vec<u8>) -> Result<()> {
        let path = self.cache.path_for(url);
        tokio::fs::write(&path, &content).await?;
        self.cache.insert(url, path);
        Ok(())
    }

    file_cache_accessors!();
}

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl<F: super::traits::AsyncSchemaFetcher> super::traits::AsyncSchemaFetcher
    for AsyncFileCachingFetcher<F>
{
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check index — read from file cache
        if let Some(path) = self.cache.cached_path(url) {
            let content = tokio::fs::read(path).await?;
            return Ok(FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            });
        }

        // Delegate to inner
        let result = self.inner.fetch(url).await?;

        // Write to file cache
        let path = self.cache.path_for(url);
        tokio::fs::write(&path, &result.content).await?;
        self.cache.insert(url, path.clone());

        // Also register under the final URL if a redirect occurred
        if result.final_url != url {
            self.cache.insert(&result.final_url, path);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::{NoopFetcher, test_support::TrackingFetcher};
    use std::collections::HashMap;

    /// A mock fetcher that simulates redirects.
    struct RedirectFetcher {
        content: Vec<u8>,
        final_url: String,
    }

    impl SchemaFetcher for RedirectFetcher {
        fn fetch(&self, _url: &str) -> Result<FetchResult> {
            Ok(FetchResult {
                content: self.content.clone(),
                final_url: self.final_url.clone(),
                redirected: true,
            })
        }
    }

    #[test]
    fn test_file_caching_fetcher_caches_result() {
        let mut responses = HashMap::new();
        responses.insert(
            "http://example.com/a.xsd".to_string(),
            b"<schema/>".to_vec(),
        );
        let inner = TrackingFetcher::new(responses);

        let fetcher = FileCachingFetcher::new(inner).unwrap();

        // First fetch
        let r1 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r1.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1);

        // Second fetch should come from file cache
        let r2 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r2.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1); // still 1
    }

    #[test]
    fn test_file_caching_fetcher_seed() {
        let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
        fetcher
            .seed("http://example.com/test.xsd", b"<seeded/>".to_vec())
            .unwrap();

        let result = fetcher.fetch("http://example.com/test.xsd").unwrap();
        assert_eq!(result.content, b"<seeded/>");
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_file_caching_fetcher_len_is_empty() {
        let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
        assert!(fetcher.is_empty());
        assert_eq!(fetcher.len(), 0);

        fetcher
            .seed("http://example.com/a.xsd", b"a".to_vec())
            .unwrap();
        assert!(!fetcher.is_empty());
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_file_caching_fetcher_with_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let fetcher = FileCachingFetcher::with_dir(NoopFetcher, dir.path());
        assert_eq!(fetcher.cache_dir(), dir.path());
    }

    #[test]
    fn test_file_caching_fetcher_with_temp_dir() {
        let parent = tempfile::TempDir::new().unwrap();
        let fetcher = FileCachingFetcher::with_temp_dir(NoopFetcher, parent.path()).unwrap();
        assert!(fetcher.cache_dir().starts_with(parent.path()));
    }

    #[test]
    fn test_file_caching_fetcher_redirect_caches_both_urls() {
        let inner = RedirectFetcher {
            content: b"<redirected/>".to_vec(),
            final_url: "http://example.com/final.xsd".to_string(),
        };
        let fetcher = FileCachingFetcher::new(inner).unwrap();

        let r = fetcher.fetch("http://example.com/original.xsd").unwrap();
        assert_eq!(r.content, b"<redirected/>");

        // Both URLs should be in the index
        assert_eq!(fetcher.len(), 2);

        // Fetching by final URL should hit the cache
        let r2 = fetcher.fetch("http://example.com/final.xsd").unwrap();
        assert_eq!(r2.content, b"<redirected/>");
    }

    #[test]
    fn test_file_caching_fetcher_temp_dir_cleanup() {
        let cache_dir;
        {
            let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
            fetcher
                .seed("http://example.com/a.xsd", b"data".to_vec())
                .unwrap();
            cache_dir = fetcher.cache_dir().to_path_buf();
            assert!(cache_dir.exists());
        }
        // After drop, the temp dir should be cleaned up
        assert!(!cache_dir.exists());
    }

    #[test]
    fn test_file_caching_fetcher_persistent_dir_not_cleaned() {
        let dir = tempfile::TempDir::new().unwrap();
        let dir_path = dir.path().to_path_buf();
        {
            let fetcher = FileCachingFetcher::with_dir(NoopFetcher, &dir_path);
            fetcher
                .seed("http://example.com/a.xsd", b"data".to_vec())
                .unwrap();
        }
        // After drop, the persistent dir should still exist
        assert!(dir_path.exists());
    }

    #[test]
    fn test_cache_filename_deterministic() {
        let a = cache_filename("http://example.com/schema.xsd");
        let b = cache_filename("http://example.com/schema.xsd");
        assert_eq!(a, b);
        assert!(a.ends_with(".xsd"));
    }

    #[test]
    fn test_cache_filename_different_urls() {
        let a = cache_filename("http://example.com/a.xsd");
        let b = cache_filename("http://example.com/b.xsd");
        assert_ne!(a, b);
    }
}
