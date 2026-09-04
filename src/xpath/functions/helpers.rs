//! Helper Functions.
//!
//! - `text()` - returns the text content of the context node
//! - Helper utilities for other functions

use crate::error::Result;
use crate::node::XmlNode;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

pub(super) fn require_arguments(
    args: &[XPathValue],
    function: &str,
    expected: usize,
) -> Result<()> {
    if args.len() == expected {
        return Ok(());
    }
    Err(XPathEvalError::WrongArgumentCount {
        function: function.to_string(),
        expected: expected.to_string(),
        found: args.len(),
    }
    .into())
}

/// `text()` - returns the text content of the context node.
/// (Usually handled as a node test, but can be called as function)
pub fn fn_text(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "text".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
    }
    let content = ctx.node.get_content().unwrap_or_default();
    Ok(XPathValue::String(content))
}

/// Gets the first node from a node-set argument, or the context node if no argument.
pub fn get_first_node_or_context(
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XmlNode> {
    if args.is_empty() {
        return Ok(ctx.node.clone());
    }

    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "(node function)".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let arg = args.into_iter().next().unwrap();
    match arg {
        XPathValue::NodeSet(nodes) => {
            Ok(nodes.into_iter().next().unwrap_or_else(|| ctx.node.clone()))
        }
        _ => Ok(ctx.node.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::functions::evaluate_function;
    use crate::xpath::functions::test_support::{create_context, create_test_document};

    #[test]
    fn test_fn_text() {
        let doc = crate::parse("<root>hello world</root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("text", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_fn_text_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("text", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_first_node_or_context_empty() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node = get_first_node_or_context(vec![], &ctx).unwrap();
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let node = get_first_node_or_context(vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(node.get_name(), "item");
    }

    #[test]
    fn test_get_first_node_or_context_empty_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node = get_first_node_or_context(vec![XPathValue::NodeSet(vec![])], &ctx).unwrap();
        // Returns context node when empty
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_non_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node =
            get_first_node_or_context(vec![XPathValue::String("test".to_string())], &ctx).unwrap();
        // Returns context node for non-nodeset
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = get_first_node_or_context(
            vec![
                XPathValue::String("a".to_string()),
                XPathValue::String("b".to_string()),
            ],
            &ctx,
        );
        assert!(result.is_err());
    }
}
