//! Local file fetcher implementation.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::file_common::{FileLocator, fetched_file, impl_file_fetcher_api, request_error};
use super::{FetchResult, SchemaFetcher};

/// A fetcher that reads schemas from the local filesystem.
///
/// Supports both `file://` URLs and regular file paths.
/// Can optionally resolve relative paths against a base directory.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{FileFetcher, SchemaFetcher};
///
/// // Create a fetcher without a base directory
/// let fetcher = FileFetcher::new();
///
/// // Create a fetcher with a base directory for resolving relative paths
/// let fetcher = FileFetcher::with_base_dir("/path/to/schemas");
///
/// // Fetch a local file
/// let result = fetcher.fetch("file:///path/to/schema.xsd");
/// ```
pub struct FileFetcher {
    locator: FileLocator,
}

impl_file_fetcher_api!(FileFetcher);

impl FileFetcher {
    /// Resolves a URL or path to an absolute file path.
    fn resolve_path(&self, url: &str) -> Option<PathBuf> {
        FileLocator::direct_path(url).or_else(|| {
            self.locator
                .relative_candidates(url)
                .into_iter()
                .find(|path| path.exists())
        })
    }
}

impl SchemaFetcher for FileFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let path = self
            .resolve_path(url)
            .ok_or_else(|| request_error(url, "Cannot resolve file path"))?;
        let content = std::fs::read(&path).map_err(|error| request_error(url, error))?;
        Ok(fetched_file(&path, content))
    }
}
