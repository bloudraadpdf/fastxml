//! Element lookup and type resolution for DOM validation.

use std::sync::Arc;

use crate::schema::types::{ElementDef, FlattenedChildren, TypeDef};

use super::super::lookup::flatten_complex_type;
use super::DomSchemaValidator;

impl DomSchemaValidator {
    /// Looks up an element definition in the schema.
    pub(crate) fn lookup_element(&self, name: &str, prefix: Option<&str>) -> Option<&ElementDef> {
        // Try local name first
        if let Some(elem) = self.schema.get_element(name) {
            return Some(elem);
        }

        // Try with prefix
        if let Some(p) = prefix {
            if !p.is_empty() {
                let qname = format!("{}:{}", p, name);
                if let Some(elem) = self.schema.get_element(&qname) {
                    return Some(elem);
                }
            }
        }

        None
    }

    /// Gets flattened children for an element from the schema cache.
    pub(crate) fn get_flattened_children_for_element(
        &self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // Try type reference first
        if let Some(ref type_ref) = elem.type_ref {
            if let Some(cached) = self.schema.type_children_cache.get(type_ref) {
                return Some(Arc::clone(cached));
            }

            // Try without prefix
            if let Some((_prefix, local)) = type_ref.split_once(':') {
                if let Some(cached) = self.schema.type_children_cache.get(local) {
                    return Some(Arc::clone(cached));
                }
            }

            // Compute at runtime if not cached
            if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
                return Some(Arc::new(flatten_complex_type(&self.schema, complex)));
            }
        }

        // Try inline type
        if let Some(ref inline_type) = elem.inline_type {
            if let TypeDef::Complex(complex) = inline_type {
                return Some(Arc::new(flatten_complex_type(&self.schema, complex)));
            }
        }

        None
    }
}
