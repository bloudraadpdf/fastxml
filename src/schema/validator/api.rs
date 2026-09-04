//! Public API functions for schema validation.

use crate::document::XmlDocument;
use crate::error::{Error, ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::xsd::XsdSchema;

use super::context::XmlSchemaValidationContext;
use super::lazy::LazySchemaValidatorWithSharedErrors;

fn referenced_schema_locations(doc: &XmlDocument) -> Result<Option<Vec<(String, String)>>> {
    let locations = crate::parser::parse_schema_locations(doc)?;
    Ok((!locations.is_empty()).then_some(locations))
}

fn validate_with_builtin_schema(doc: &XmlDocument) -> Result<Vec<StructuredError>> {
    XmlSchemaValidationContext::new(crate::schema::xsd::create_builtin_schema()).validate(doc)
}

fn schema_not_found(location: &str) -> Error {
    crate::schema::error::SchemaError::SchemaNotFound {
        uri: location.to_owned(),
    }
    .into()
}

fn record_schema_resolution(
    location: &str,
    resolution: Result<()>,
    errors: &mut Vec<StructuredError>,
) -> bool {
    match resolution {
        Ok(()) => true,
        Err(error) => {
            errors.push(
                StructuredError::new(
                    format!("Failed to parse schema {location}: {error}"),
                    ValidationErrorType::SchemaNotFound,
                )
                .with_level(ErrorLevel::Warning),
            );
            false
        }
    }
}

fn validate_resolved_schemas(
    doc: &XmlDocument,
    schemas: Vec<XsdSchema>,
    mut errors: Vec<StructuredError>,
) -> Result<Vec<StructuredError>> {
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);

    match XmlSchemaValidationContext::new(schema).validate(doc) {
        Ok(validation_errors) => errors.extend(validation_errors),
        Err(error) => errors.push(
            StructuredError::new(
                format!("Validation error: {error}"),
                ValidationErrorType::Other,
            )
            .with_level(ErrorLevel::Error),
        ),
    }
    Ok(errors)
}

/// Validates a document using schemas referenced in xsi:schemaLocation.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas, and validates the document.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location};
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let errors = validate_with_schema_location(&doc)?;
/// ```
#[cfg(feature = "ureq")]
#[doc(hidden)]
pub fn validate_with_schema_location(doc: &XmlDocument) -> Result<Vec<StructuredError>> {
    validate_with_schema_location_and_fetcher(doc, &crate::schema::fetcher::DefaultFetcher::new())
}

/// Validates a document using schemas referenced in xsi:schemaLocation with a custom fetcher.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas using the provided fetcher,
/// and validates the document.
///
/// # Arguments
///
/// * `doc` - The XML document to validate
/// * `fetcher` - A schema fetcher implementation for downloading schemas
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location_and_fetcher};
/// use fastxml::schema::UreqFetcher;
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let fetcher = UreqFetcher::new().timeout(60);
/// let errors = validate_with_schema_location_and_fetcher(&doc, &fetcher)?;
/// ```
#[doc(hidden)]
pub fn validate_with_schema_location_and_fetcher<F: SchemaFetcher>(
    doc: &XmlDocument,
    fetcher: &F,
) -> Result<Vec<StructuredError>> {
    let Some(locations) = referenced_schema_locations(doc)? else {
        return validate_with_builtin_schema(doc);
    };

    // Use a single resolver to avoid duplicate dependency fetches
    let mut resolver = crate::schema::xsd::SchemaResolver::new(fetcher);
    let mut all_errors = Vec::new();
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                loaded_any |= record_schema_resolution(
                    location,
                    resolver.resolve_entry(&fetch_result.content, &fetch_result.final_url),
                    &mut all_errors,
                );
            }
            Err(_) => return Err(schema_not_found(location)),
        }
    }

    if !loaded_any {
        return validate_with_builtin_schema(doc);
    }

    validate_resolved_schemas(doc, resolver.take_all_schemas(), all_errors)
}

/// Validates XML from a reader using streaming parser with schemas from xsi:schemaLocation.
///
/// This performs true single-pass streaming validation:
/// 1. On the first StartElement, extracts xsi:schemaLocation
/// 2. Fetches and compiles the referenced schemas
/// 3. Continues streaming validation with the fetched schema
///
/// # Example
///
/// ```ignore
/// use fastxml::streaming_validate_with_schema_location;
/// use std::fs::File;
/// use std::io::BufReader;
///
/// let file = File::open("large_document.xml")?;
/// let errors = streaming_validate_with_schema_location(BufReader::new(file))?;
///
/// if errors.is_empty() {
///     println!("Document is valid!");
/// }
/// ```
#[cfg(feature = "ureq")]
#[doc(hidden)]
pub fn streaming_validate_with_schema_location<R: std::io::BufRead>(
    reader: R,
) -> Result<Vec<StructuredError>> {
    streaming_validate_with_schema_location_and_fetcher(
        reader,
        crate::schema::fetcher::DefaultFetcher::new(),
    )
}

/// Validates XML from a reader using streaming parser with a custom fetcher.
///
/// This performs true single-pass streaming validation.
#[doc(hidden)]
pub fn streaming_validate_with_schema_location_and_fetcher<
    R: std::io::BufRead,
    F: SchemaFetcher + 'static,
>(
    reader: R,
    fetcher: F,
) -> Result<Vec<StructuredError>> {
    use crate::event::StreamingParser;
    use std::sync::{Arc, Mutex};

    let mut parser = StreamingParser::new(reader);

    // Shared error collection
    let shared_errors = Arc::new(Mutex::new(Vec::new()));
    let validator = LazySchemaValidatorWithSharedErrors::new(fetcher, Arc::clone(&shared_errors));
    parser.add_handler(Box::new(validator));

    parser.parse()?;

    // Collect errors from shared state
    let errors = shared_errors.lock().unwrap().clone();
    Ok(errors)
}

/// Validates a document using schemas referenced in xsi:schemaLocation with an async fetcher.
///
/// This is the async version of [`validate_with_schema_location_and_fetcher`].
/// It fetches schemas asynchronously using the provided fetcher.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location_with_async_fetcher};
/// use fastxml::schema::AsyncDefaultFetcher;
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let fetcher = AsyncDefaultFetcher::new()?;
/// let errors = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await?;
/// ```
#[cfg(feature = "tokio")]
#[doc(hidden)]
pub async fn validate_with_schema_location_with_async_fetcher<
    F: crate::schema::fetcher::AsyncSchemaFetcher,
>(
    doc: &XmlDocument,
    fetcher: &F,
) -> Result<Vec<StructuredError>> {
    let Some(locations) = referenced_schema_locations(doc)? else {
        return validate_with_builtin_schema(doc);
    };

    // Use a single async resolver to avoid duplicate dependency fetches
    let mut resolver = crate::schema::xsd::AsyncSchemaResolver::new(fetcher);
    let mut all_errors = Vec::new();
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location).await {
            Ok(fetch_result) => {
                loaded_any |= record_schema_resolution(
                    location,
                    resolver
                        .resolve_entry(&fetch_result.content, &fetch_result.final_url)
                        .await,
                    &mut all_errors,
                );
            }
            Err(_) => return Err(schema_not_found(location)),
        }
    }

    if !loaded_any {
        return validate_with_builtin_schema(doc);
    }

    validate_resolved_schemas(doc, resolver.take_all_schemas(), all_errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_with_schema_location_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = validate_with_schema_location_and_fetcher(&doc, &fetcher);
        // No schemaLocation found, uses builtin schema -> Ok
        assert!(result.is_ok());
    }

    #[test]
    fn test_streaming_validate_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let reader = std::io::BufReader::new(xml.as_bytes());
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = streaming_validate_with_schema_location_and_fetcher(reader, fetcher);
        // No schemaLocation, uses builtin schema -> Ok
        assert!(result.is_ok());
    }
}

#[cfg(all(test, feature = "tokio"))]
mod async_tests {
    use super::*;
    use crate::schema::fetcher::test_support::AsyncTrackingFetcher;

    #[tokio::test]
    async fn test_validate_with_schema_location_async_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = AsyncTrackingFetcher::new();

        let result = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await;
        // No schemaLocation found, uses builtin schema -> Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_with_schema_location_async_with_schema() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/ns">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">content</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = AsyncTrackingFetcher::new();
        fetcher.add_response("http://example.com/schema.xsd", xsd.as_bytes());

        let result = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await;
        assert!(result.is_ok());
    }
}
