//! Async local file fetcher implementation.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::FetchResult;
use super::file_common::{FileLocator, fetched_file, impl_file_fetcher_api, request_error};
use super::traits::AsyncSchemaFetcher;

/// An async fetcher that reads schemas from the local filesystem using tokio.
///
/// Supports both `file://` URLs and regular file paths.
/// Can optionally resolve relative paths against a base directory.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::AsyncFileFetcher;
/// use fastxml::schema::fetcher::AsyncSchemaFetcher;
///
/// # async fn example() -> Result<(), fastxml::error::Error> {
/// // Create a fetcher without a base directory
/// let fetcher = AsyncFileFetcher::new();
///
/// // Create a fetcher with a base directory for resolving relative paths
/// let fetcher = AsyncFileFetcher::with_base_dir("/path/to/schemas");
///
/// // Fetch a local file
/// let result = fetcher.fetch("file:///path/to/schema.xsd").await?;
/// # Ok(())
/// # }
/// ```
pub struct AsyncFileFetcher {
    locator: FileLocator,
}

impl_file_fetcher_api!(AsyncFileFetcher);

impl AsyncFileFetcher {
    /// Resolves a URL or path to an absolute file path.
    async fn resolve_path(&self, url: &str) -> Option<PathBuf> {
        if let Some(path) = FileLocator::direct_path(url) {
            return Some(path);
        }
        for path in self.locator.relative_candidates(url) {
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                return Some(path);
            }
        }
        None
    }

    /// Fetches a schema from the local filesystem asynchronously.
    pub async fn fetch(&self, url: &str) -> Result<FetchResult> {
        let path = self
            .resolve_path(url)
            .await
            .ok_or_else(|| request_error(url, "Cannot resolve file path"))?;
        let content = tokio::fs::read(&path)
            .await
            .map_err(|error| request_error(url, error))?;
        Ok(fetched_file(&path, content))
    }
}

#[async_trait::async_trait]
impl AsyncSchemaFetcher for AsyncFileFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        AsyncFileFetcher::fetch(self, url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_async_file_fetcher_absolute_path() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch(path).await.unwrap();

        assert!(result.content.starts_with(b"test content"));
        assert!(result.final_url.starts_with("file://"));
        assert!(!result.redirected);
    }

    #[tokio::test]
    async fn test_async_file_fetcher_file_url() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();
        let file_url = format!("file://{}", path);

        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch(&file_url).await.unwrap();

        assert!(result.content.starts_with(b"test content"));
    }

    #[tokio::test]
    async fn test_async_file_fetcher_with_base_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("schema.xsd");
        std::fs::write(&file_path, "schema content").unwrap();

        let fetcher = AsyncFileFetcher::with_base_dir(temp_dir.path());
        let result = fetcher.fetch("schema.xsd").await.unwrap();

        assert_eq!(result.content, b"schema content");
    }

    #[tokio::test]
    async fn test_async_file_fetcher_not_found() {
        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch("/nonexistent/path/schema.xsd").await;

        assert!(result.is_err());
    }
}
