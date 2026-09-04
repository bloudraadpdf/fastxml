//! Caching fetcher wrapper.
//!
//! Wraps any `SchemaFetcher` or `AsyncSchemaFetcher` with an in-memory cache
//! backed by `DashMap` so each URL is fetched at most once.

use dashmap::DashMap;

use crate::error::Result;

use super::result::FetchResult;
use super::traits::SchemaFetcher;

struct CacheState<F> {
    inner: F,
    entries: DashMap<String, FetchResult>,
}

impl<F> CacheState<F> {
    fn new(inner: F) -> Self {
        Self {
            inner,
            entries: DashMap::new(),
        }
    }

    fn seed(&self, url: &str, content: Vec<u8>) {
        self.entries.insert(
            url.to_string(),
            FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            },
        );
    }

    fn cached(&self, url: &str) -> Option<FetchResult> {
        self.entries.get(url).map(|entry| entry.value().clone())
    }

    fn cached_result(&self, url: &str) -> Option<Result<FetchResult>> {
        self.cached(url).map(Ok)
    }

    fn store(&self, url: &str, result: &FetchResult) {
        self.entries.insert(url.to_string(), result.clone());
        if result.final_url != url {
            self.entries
                .insert(result.final_url.clone(), result.clone());
        }
    }

    fn finish_fetch(&self, url: &str, result: FetchResult) -> FetchResult {
        self.store(url, &result);
        result
    }
}

macro_rules! impl_cache_api {
    ($(#[$attribute:meta])* $name:ident, $bound:path) => {
        $(#[$attribute])*
        impl<F: $bound> $name<F> {
            /// Creates a caching fetcher around the given inner fetcher.
            pub fn new(inner: F) -> Self {
                Self {
                    state: CacheState::new(inner),
                }
            }

            /// Pre-seeds the cache with content for a given URL.
            pub fn seed(&self, url: &str, content: Vec<u8>) {
                self.state.seed(url, content);
            }

            /// Returns the number of cached entries.
            pub fn len(&self) -> usize {
                self.state.entries.len()
            }

            /// Returns `true` if the cache is empty.
            pub fn is_empty(&self) -> bool {
                self.state.entries.is_empty()
            }

            /// Returns a reference to the inner fetcher.
            pub fn inner(&self) -> &F {
                &self.state.inner
            }
        }
    };
}

/// A fetcher wrapper that caches fetch results in memory.
///
/// When a URL is requested:
/// 1. Check the cache — if present, return the cached result.
/// 2. Otherwise delegate to the inner fetcher.
/// 3. Store the result under both the requested URL and the final URL
///    (if a redirect occurred).
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::fetcher::{CachingFetcher, NoopFetcher};
///
/// // Wrap any SchemaFetcher with caching (note: DefaultFetcher already has built-in caching)
/// let fetcher = CachingFetcher::new(NoopFetcher);
/// fetcher.seed("http://example.com/schema.xsd", b"<schema/>".to_vec());
/// let result = fetcher.fetch("http://example.com/schema.xsd")?;
/// ```
pub struct CachingFetcher<F: SchemaFetcher> {
    state: CacheState<F>,
}

impl_cache_api!(CachingFetcher, SchemaFetcher);

impl<F: SchemaFetcher> SchemaFetcher for CachingFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        if let Some(result) = self.state.cached_result(url) {
            return result;
        }

        self.state
            .inner
            .fetch(url)
            .map(|result| self.state.finish_fetch(url, result))
    }
}

/// Async version of [`CachingFetcher`].
#[cfg(feature = "tokio")]
pub struct AsyncCachingFetcher<F: super::traits::AsyncSchemaFetcher> {
    state: CacheState<F>,
}

impl_cache_api!(
    #[cfg(feature = "tokio")]
    AsyncCachingFetcher,
    super::traits::AsyncSchemaFetcher
);

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl<F: super::traits::AsyncSchemaFetcher> super::traits::AsyncSchemaFetcher
    for AsyncCachingFetcher<F>
{
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        if let Some(result) = self.state.cached_result(url) {
            return result;
        }

        self.state
            .inner
            .fetch(url)
            .await
            .map(|result| self.state.finish_fetch(url, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::{NoopFetcher, test_support::TrackingFetcher};
    use std::collections::HashMap;

    #[test]
    fn test_caching_fetcher_caches_result() {
        let mut responses = HashMap::new();
        responses.insert(
            "http://example.com/a.xsd".to_string(),
            b"<schema/>".to_vec(),
        );
        let inner = TrackingFetcher::new(responses);

        let fetcher = CachingFetcher::new(inner);

        // First fetch
        let r1 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r1.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1);

        // Second fetch should come from cache
        let r2 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r2.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1); // still 1
    }

    #[test]
    fn test_caching_fetcher_seed() {
        let fetcher = CachingFetcher::new(NoopFetcher);
        fetcher.seed("http://example.com/test.xsd", b"<seeded/>".to_vec());

        let result = fetcher.fetch("http://example.com/test.xsd").unwrap();
        assert_eq!(result.content, b"<seeded/>");
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_caching_fetcher_len_is_empty() {
        let fetcher = CachingFetcher::new(NoopFetcher);
        assert!(fetcher.is_empty());
        assert_eq!(fetcher.len(), 0);

        fetcher.seed("http://example.com/a.xsd", b"a".to_vec());
        assert!(!fetcher.is_empty());
        assert_eq!(fetcher.len(), 1);
    }
}
