//! Node Set Functions.
//!
//! - `last()` - returns the context size
//! - `position()` - returns the context position
//! - `count(node-set)` - returns the number of nodes
//! - `name([node-set])` - returns the expanded-name
//! - `local-name([node-set])` - returns the local part of the name
//! - `namespace-uri([node-set])` - returns the namespace URI
//! - `id(object)` - selects elements by ID

use crate::error::Result;
use crate::node::XmlNode;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

use super::helpers::{get_first_node_or_context, require_arguments};

/// `last()` - returns the context size.
pub fn fn_last(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "last", 0)?;
    Ok(XPathValue::Number(ctx.size() as f64))
}

/// `position()` - returns the context position.
pub fn fn_position(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "position", 0)?;
    Ok(XPathValue::Number(ctx.position() as f64))
}

/// `count(node-set)` - returns the number of nodes in the node-set.
pub fn fn_count(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "count", 1)?;
    let nodes = args.into_iter().next().unwrap();
    match nodes {
        XPathValue::NodeSet(ns) => Ok(XPathValue::Number(ns.len() as f64)),
        _ => Err(XPathEvalError::InvalidArgumentType {
            function: "count".to_string(),
            expected: "node-set".to_string(),
        }
        .into()),
    }
}

/// `name([node-set])` - returns the qualified name of the first node.
pub fn fn_name(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    Ok(XPathValue::String(node.qname()))
}

/// `local-name([node-set])` - returns the local part of the name.
pub fn fn_local_name(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    Ok(XPathValue::String(node.get_name()))
}

/// `namespace-uri([node-set])` - returns the namespace URI.
pub fn fn_namespace_uri(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    let uri = node.get_namespace_uri().unwrap_or_default();
    Ok(XPathValue::String(uri))
}

/// `id(object)` - selects elements by their ID.
pub fn fn_id(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "id", 1)?;

    let id_value = args.into_iter().next().unwrap();
    let ids: Vec<String> = match id_value {
        XPathValue::NodeSet(nodes) => {
            // Get string values of all nodes, split by whitespace
            nodes
                .iter()
                .filter_map(|n| n.get_content())
                .flat_map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect()
        }
        _ => {
            // Split string value by whitespace
            id_value
                .to_string_value()
                .split_whitespace()
                .map(String::from)
                .collect()
        }
    };

    // Find elements with matching id attribute
    let mut result = Vec::new();
    let root = ctx.doc.document_node();
    find_elements_by_id(&root, &ids, &mut result);

    Ok(XPathValue::NodeSet(result))
}

fn find_elements_by_id(node: &XmlNode, ids: &[String], result: &mut Vec<XmlNode>) {
    if node.is_element() {
        if let Some(id_attr) = node.get_attribute("id") {
            if ids.iter().any(|id| id == &id_attr) {
                result.push(node.clone());
            }
        }
    }
    for child in node.get_child_nodes() {
        find_elements_by_id(&child, ids, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::functions::evaluate_function;
    use crate::xpath::functions::test_support::{create_context, create_test_document, evaluate};

    fn evaluate_id(value: XPathValue) -> Vec<XmlNode> {
        match evaluate("id", vec![value]).unwrap() {
            XPathValue::NodeSet(nodes) => nodes,
            _ => panic!("Expected NodeSet"),
        }
    }

    #[test]
    fn test_fn_last() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root).with_position(1, 5);

        let result = evaluate_function("last", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn test_fn_last_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("last", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_position() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root).with_position(3, 5);

        let result = evaluate_function("position", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_position_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("position", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_count() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let result = evaluate_function("count", vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_count_empty() {
        let result = evaluate("count", vec![XPathValue::NodeSet(vec![])]).unwrap();
        assert_eq!(result.to_number(), 0.0);
    }

    #[test]
    fn test_fn_count_wrong_args() {
        assert!(evaluate("count", vec![]).is_err());
        assert!(evaluate("count", vec![XPathValue::String("test".to_string())]).is_err());
    }

    #[test]
    fn test_fn_name() {
        let result = evaluate("name", vec![]).unwrap();
        assert_eq!(result.to_string_value(), "root");
    }

    #[test]
    fn test_fn_name_with_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let result = evaluate_function("name", vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "item");
    }

    #[test]
    fn test_fn_local_name() {
        let result = evaluate("local-name", vec![]).unwrap();
        assert_eq!(result.to_string_value(), "root");
    }

    #[test]
    fn test_fn_namespace_uri() {
        let result = evaluate("namespace-uri", vec![]).unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_id() {
        let nodes = evaluate_id(XPathValue::String("2".to_string()));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_attribute("id").unwrap(), "2");
    }

    #[test]
    fn test_fn_id_multiple() {
        assert_eq!(evaluate_id(XPathValue::String("1 3".to_string())).len(), 2);
    }

    #[test]
    fn test_fn_id_not_found() {
        assert!(evaluate_id(XPathValue::String("999".to_string())).is_empty());
    }

    #[test]
    fn test_fn_id_wrong_args() {
        assert!(evaluate("id", vec![]).is_err());
    }

    #[test]
    fn test_fn_id_with_nodeset() {
        let doc = crate::parse(
            "<root><ids>1 2</ids><item id=\"1\">A</item><item id=\"2\">B</item></root>",
        )
        .unwrap();
        let root = doc.get_root_element().unwrap();
        let ids_node = root.get_child_nodes().into_iter().next().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("id", vec![XPathValue::NodeSet(vec![ids_node])], &ctx).unwrap();
        match result {
            XPathValue::NodeSet(nodes) => {
                assert_eq!(nodes.len(), 2);
            }
            _ => panic!("Expected NodeSet"),
        }
    }
}
