//! XPath function library.
//!
//! This module implements the core XPath 1.0 function library:
//!
//! ## Node Set Functions
//! - `last()` - returns the context size
//! - `position()` - returns the context position
//! - `count(node-set)` - returns the number of nodes
//! - `name([node-set])` - returns the expanded-name
//! - `local-name([node-set])` - returns the local part of the name
//! - `namespace-uri([node-set])` - returns the namespace URI
//!
//! ## String Functions
//! - `string([object])` - converts to string
//! - `concat(string, string, ...)` - concatenates strings
//! - `starts-with(string, string)` - tests string prefix
//! - `contains(string, string)` - tests if string contains substring
//! - `substring(string, number, [number])` - extracts substring
//! - `substring-before(string, string)` - returns substring before match
//! - `substring-after(string, string)` - returns substring after match
//! - `string-length([string])` - returns string length
//! - `normalize-space([string])` - normalizes whitespace
//! - `translate(string, string, string)` - character translation
//!
//! ## Boolean Functions
//! - `boolean(object)` - converts to boolean
//! - `not(boolean)` - negates boolean
//! - `true()` - returns true
//! - `false()` - returns false
//!
//! ## Number Functions
//! - `number([object])` - converts to number
//! - `sum(node-set)` - sums node values
//! - `floor(number)` - rounds down
//! - `ceiling(number)` - rounds up
//! - `round(number)` - rounds to nearest integer

mod boolean;
mod helpers;
mod nodeset;
mod number;
mod string;

use crate::error::Result;
use crate::xpath::error::XPathEvalError;

use super::types::{EvaluationContext, XPathValue};

#[cfg(test)]
mod test_support {
    use crate::document::XmlDocument;
    use crate::error::Result;
    use crate::namespace::NamespaceResolver;
    use crate::node::XmlNode;

    use super::{EvaluationContext, XPathValue, evaluate_function};

    pub(super) fn create_test_document() -> XmlDocument {
        crate::parse(
            "<root><item id=\"1\">10</item><item id=\"2\">20</item><item id=\"3\">30</item></root>",
        )
        .unwrap()
    }

    pub(super) fn create_context<'a>(
        doc: &'a XmlDocument,
        node: &XmlNode,
    ) -> EvaluationContext<'a> {
        EvaluationContext::new(node.clone(), doc, NamespaceResolver::new())
    }

    pub(super) fn evaluate(name: &str, args: Vec<XPathValue>) -> Result<XPathValue> {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        evaluate_function(name, args, &create_context(&doc, &root))
    }
}

// Re-export for tests
pub use boolean::{fn_boolean, fn_false, fn_lang, fn_not, fn_true};
pub use helpers::{fn_text, get_first_node_or_context};
pub use nodeset::{
    fn_count, fn_id, fn_last, fn_local_name, fn_name, fn_namespace_uri, fn_position,
};
pub use number::{fn_ceiling, fn_floor, fn_number, fn_round, fn_sum};
pub use string::{
    fn_concat, fn_contains, fn_normalize_space, fn_starts_with, fn_string, fn_string_length,
    fn_substring, fn_substring_after, fn_substring_before, fn_translate,
};

/// Evaluates an XPath function call.
pub fn evaluate_function(
    name: &str,
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    match name {
        // Node Set Functions
        "last" => nodeset::fn_last(args, ctx),
        "position" => nodeset::fn_position(args, ctx),
        "count" => nodeset::fn_count(args, ctx),
        "name" => nodeset::fn_name(args, ctx),
        "local-name" => nodeset::fn_local_name(args, ctx),
        "namespace-uri" => nodeset::fn_namespace_uri(args, ctx),
        "id" => nodeset::fn_id(args, ctx),

        // String Functions
        "string" => string::fn_string(args, ctx),
        "concat" => string::fn_concat(args, ctx),
        "starts-with" => string::fn_starts_with(args, ctx),
        "contains" => string::fn_contains(args, ctx),
        "substring" => string::fn_substring(args, ctx),
        "substring-before" => string::fn_substring_before(args, ctx),
        "substring-after" => string::fn_substring_after(args, ctx),
        "string-length" => string::fn_string_length(args, ctx),
        "normalize-space" => string::fn_normalize_space(args, ctx),
        "translate" => string::fn_translate(args, ctx),

        // Boolean Functions
        "boolean" => boolean::fn_boolean(args, ctx),
        "not" => boolean::fn_not(args, ctx),
        "true" => boolean::fn_true(args, ctx),
        "false" => boolean::fn_false(args, ctx),
        "lang" => boolean::fn_lang(args, ctx),

        // Number Functions
        "number" => number::fn_number(args, ctx),
        "sum" => number::fn_sum(args, ctx),
        "floor" => number::fn_floor(args, ctx),
        "ceiling" => number::fn_ceiling(args, ctx),
        "round" => number::fn_round(args, ctx),

        // text() is handled as a node test, but if called as function
        "text" => helpers::fn_text(args, ctx),

        _ => Err(XPathEvalError::UnknownFunction {
            name: name.to_string(),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_function() {
        assert!(test_support::evaluate("unknown-function", vec![]).is_err());
    }
}
