//! Asynchronous schema resolver.
//!
//! This module provides the async implementation of schema resolution
//! for import/include chains.

use std::collections::VecDeque;

use crate::error::Result;
use crate::schema::fetcher::AsyncSchemaFetcher;

use super::super::parser::parse_xsd_ast;
use super::super::types::XsdSchema;
use super::common::{ResolutionState, impl_resolution_outputs, resolve_uri};

/// Async schema resolver that handles import/include chains.
pub struct AsyncSchemaResolver<'a, F: AsyncSchemaFetcher> {
    fetcher: &'a F,
    state: ResolutionState,
}

impl<'a, F: AsyncSchemaFetcher> AsyncSchemaResolver<'a, F> {
    /// Creates a new async schema resolver.
    pub fn new(fetcher: &'a F) -> Self {
        Self {
            fetcher,
            state: ResolutionState::new(),
        }
    }

    /// Resolves all dependencies starting from an entry schema.
    ///
    /// Returns all resolved schemas in dependency order (dependencies first).
    pub async fn resolve_all(
        &mut self,
        entry_content: &[u8],
        entry_uri: &str,
    ) -> Result<Vec<XsdSchema>> {
        self.state.insert_entry(entry_content, entry_uri)?;
        self.resolve_dependencies(entry_uri).await?;
        Ok(self.state.take_entry_last(entry_uri))
    }

    /// Fetches a schema via the fetcher (caching is handled by the fetcher).
    async fn fetch_schema(&self, uri: &str) -> Result<Vec<u8>> {
        let result = self.fetcher.fetch(uri).await?;
        Ok(result.content)
    }

    async fn resolve_dependencies(&mut self, entry_uri: &str) -> Result<()> {
        let mut queue = VecDeque::from([entry_uri.to_string()]);

        while let Some(current_uri) = queue.pop_front() {
            for location in self.state.begin(&current_uri)? {
                self.resolve_dependency(&current_uri, &location, &mut queue)
                    .await?;
            }
            self.state.finish(&current_uri);
        }

        Ok(())
    }

    async fn resolve_dependency(
        &mut self,
        current_uri: &str,
        location: &str,
        queue: &mut VecDeque<String>,
    ) -> Result<()> {
        let resolved_uri = resolve_uri(current_uri, location)?;
        if self.state.contains(&resolved_uri) {
            return Ok(());
        }

        let content = self.fetch_schema(&resolved_uri).await?;
        let schema = parse_xsd_ast(&content)?;
        self.state.insert_dependency(resolved_uri.clone(), schema);
        queue.push_back(resolved_uri);
        Ok(())
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
    pub async fn resolve_entry(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<()> {
        if self.state.contains(entry_uri) {
            return Ok(());
        }

        self.state.insert_entry(entry_content, entry_uri)?;
        self.resolve_dependencies(entry_uri).await
    }

    impl_resolution_outputs!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::test_support::AsyncTrackingFetcher;

    #[tokio::test]
    async fn test_async_resolve_simple() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let fetcher = AsyncTrackingFetcher::new();

        let mut resolver = AsyncSchemaResolver::new(&fetcher);
        let schemas = resolver
            .resolve_all(xsd.as_bytes(), "http://example.com/test.xsd")
            .await
            .unwrap();

        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].elements.len(), 1);
    }

    #[tokio::test]
    async fn test_async_resolve_with_import() {
        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/types">
            <xs:simpleType name="NameType">
                <xs:restriction base="xs:string">
                    <xs:maxLength value="100"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:t="http://example.com/types"
                   targetNamespace="http://example.com/main">
            <xs:import namespace="http://example.com/types" schemaLocation="types.xsd"/>
            <xs:element name="person">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="name" type="t:NameType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let fetcher = AsyncTrackingFetcher::new();
        fetcher.add_response("http://example.com/types.xsd", types_xsd.as_bytes());

        let mut resolver = AsyncSchemaResolver::new(&fetcher);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        // Should have 2 schemas: types.xsd and main.xsd
        assert_eq!(schemas.len(), 2);
    }

    #[tokio::test]
    async fn test_async_resolve_with_include() {
        let common_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="IDType">
                <xs:restriction base="xs:string">
                    <xs:pattern value="[A-Z]{2}[0-9]{4}"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:include schemaLocation="common.xsd"/>
            <xs:element name="item">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="id" type="IDType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let fetcher = AsyncTrackingFetcher::new();
        fetcher.add_response("http://example.com/common.xsd", common_xsd.as_bytes());

        let mut resolver = AsyncSchemaResolver::new(&fetcher);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        assert_eq!(schemas.len(), 2);
    }

    #[tokio::test]
    async fn test_async_resolve_uses_cache() {
        use crate::schema::fetcher::AsyncCachingFetcher;

        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="CachedType">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:import schemaLocation="types.xsd"/>
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let fetcher = AsyncTrackingFetcher::new();
        // Don't add to fetcher - it should be fetched from caching fetcher's seed

        let caching = AsyncCachingFetcher::new(fetcher);
        // Pre-populate the cache
        caching.seed(
            "http://example.com/types.xsd",
            types_xsd.as_bytes().to_vec(),
        );

        let mut resolver = AsyncSchemaResolver::new(&caching);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        // Should succeed even though inner fetcher doesn't have types.xsd
        assert_eq!(schemas.len(), 2);
    }
}
