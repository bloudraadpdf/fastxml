//! Synchronous schema resolver.
//!
//! This module provides the synchronous implementation of schema resolution
//! for import/include chains.

use std::collections::VecDeque;

use crate::error::Result;
use crate::schema::fetcher::SchemaFetcher;

use super::super::parser::parse_xsd_ast;
use super::super::types::XsdSchema;
use super::common::{ResolutionState, impl_resolution_outputs, resolve_uri};

/// Schema resolver that handles import/include chains.
pub struct SchemaResolver<'a, F: SchemaFetcher> {
    fetcher: &'a F,
    state: ResolutionState,
}

impl<'a, F: SchemaFetcher> SchemaResolver<'a, F> {
    /// Creates a new schema resolver.
    pub fn new(fetcher: &'a F) -> Self {
        Self {
            fetcher,
            state: ResolutionState::new(),
        }
    }

    /// Resolves all dependencies starting from an entry schema.
    ///
    /// Returns all resolved schemas in dependency order (dependencies first).
    pub fn resolve_all(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<Vec<XsdSchema>> {
        self.insert_and_resolve(entry_content, entry_uri)?;
        Ok(self.state.take_entry_last(entry_uri))
    }

    /// Fetches a schema via the fetcher (caching is handled by the fetcher).
    fn fetch_schema(&self, uri: &str) -> Result<Vec<u8>> {
        let result = self.fetcher.fetch(uri)?;
        Ok(result.content)
    }

    /// Resolves an entry schema and accumulates it along with its dependencies.
    ///
    /// Unlike [`Self::resolve_all`], this method does not return schemas immediately.
    /// Instead, it accumulates them internally so that multiple entry schemas
    /// can share resolved dependencies (avoiding duplicate fetches).
    ///
    /// Call [`Self::take_all_schemas`] after all entries have been resolved.
    ///
    /// # Arguments
    ///
    /// * `entry_content` - The entry XSD file content as bytes
    /// * `entry_uri` - URI for the entry schema (used for resolving relative imports)
    pub fn resolve_entry(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<()> {
        // Skip if already resolved
        if self.state.contains(entry_uri) {
            return Ok(());
        }
        self.insert_and_resolve(entry_content, entry_uri)
    }

    fn insert_and_resolve(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<()> {
        self.state.insert_entry(entry_content, entry_uri)?;
        let mut queue = VecDeque::from([entry_uri.to_string()]);

        while let Some(current_uri) = queue.pop_front() {
            for location in self.state.begin(&current_uri)? {
                let resolved_uri = resolve_uri(&current_uri, &location)?;
                if !self.state.contains(&resolved_uri) {
                    let content = self.fetch_schema(&resolved_uri)?;
                    let schema = parse_xsd_ast(&content)?;
                    self.state.insert_dependency(resolved_uri.clone(), schema);
                    queue.push_back(resolved_uri);
                }
            }
            self.state.finish(&current_uri);
        }

        Ok(())
    }

    impl_resolution_outputs!();
}
