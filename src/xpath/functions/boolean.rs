//! Boolean Functions.
//!
//! - `boolean(object)` - converts to boolean
//! - `not(boolean)` - negates boolean
//! - `true()` - returns true
//! - `false()` - returns false
//! - `lang(string)` - checks language

use crate::error::Result;
use crate::xpath::types::{EvaluationContext, XPathValue};

use super::helpers::require_arguments;

/// `boolean(object)` - converts the argument to boolean.
pub fn fn_boolean(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "boolean", 1)?;
    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(value.to_boolean()))
}

/// `not(boolean)` - negates the boolean value.
pub fn fn_not(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "not", 1)?;
    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(!value.to_boolean()))
}

/// `true()` - returns true.
pub fn fn_true(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "true", 0)?;
    Ok(XPathValue::Boolean(true))
}

/// `false()` - returns false.
pub fn fn_false(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "false", 0)?;
    Ok(XPathValue::Boolean(false))
}

/// `lang(string)` - checks if the context node's language matches.
pub fn fn_lang(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    require_arguments(&args, "lang", 1)?;
    let lang_arg = args
        .into_iter()
        .next()
        .unwrap()
        .to_string_value()
        .to_lowercase();

    // Search for lang attribute in context node and ancestors
    // (xml:lang is stored as "lang" since attributes use local names only)
    let mut node = Some(ctx.node.clone());
    while let Some(n) = node {
        if let Some(lang_attr) = n.get_attribute("lang") {
            let lang_lower = lang_attr.to_lowercase();
            // Check if lang matches or is a sublanguage
            let matches =
                lang_lower == lang_arg || lang_lower.starts_with(&format!("{}-", lang_arg));
            return Ok(XPathValue::Boolean(matches));
        }
        node = n.get_parent();
    }

    Ok(XPathValue::Boolean(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::functions::evaluate_function;
    use crate::xpath::functions::test_support::{create_context, evaluate};

    fn assert_boolean(name: &str, args: Vec<XPathValue>, expected: bool) {
        assert_eq!(evaluate(name, args).unwrap().to_boolean(), expected);
    }

    fn assert_wrong_args(name: &str, args: Vec<XPathValue>) {
        assert!(evaluate(name, args).is_err());
    }

    fn assert_lang(xml: &str, expected: bool) {
        let doc = crate::parse(xml).unwrap();
        let root = doc.get_root_element().unwrap();
        let child = root.get_child_nodes().into_iter().next().unwrap();
        let result = evaluate_function(
            "lang",
            vec![XPathValue::String("en".to_string())],
            &create_context(&doc, &child),
        )
        .unwrap();
        assert_eq!(result.to_boolean(), expected);
    }

    #[test]
    fn test_fn_boolean_true_string() {
        assert_boolean(
            "boolean",
            vec![XPathValue::String("hello".to_string())],
            true,
        );
    }

    #[test]
    fn test_fn_boolean_false_empty_string() {
        assert_boolean("boolean", vec![XPathValue::String("".to_string())], false);
    }

    #[test]
    fn test_fn_boolean_number() {
        assert_boolean("boolean", vec![XPathValue::Number(1.0)], true);
        assert_boolean("boolean", vec![XPathValue::Number(0.0)], false);
    }

    #[test]
    fn test_fn_boolean_wrong_args() {
        assert_wrong_args("boolean", vec![]);
    }

    #[test]
    fn test_fn_not_true() {
        assert_boolean("not", vec![XPathValue::Boolean(false)], true);
    }

    #[test]
    fn test_fn_not_false() {
        assert_boolean("not", vec![XPathValue::Boolean(true)], false);
    }

    #[test]
    fn test_fn_not_wrong_args() {
        assert_wrong_args("not", vec![]);
    }

    #[test]
    fn test_fn_true() {
        assert_boolean("true", vec![], true);
    }

    #[test]
    fn test_fn_true_wrong_args() {
        assert_wrong_args("true", vec![XPathValue::Boolean(false)]);
    }

    #[test]
    fn test_fn_false() {
        assert_boolean("false", vec![], false);
    }

    #[test]
    fn test_fn_false_wrong_args() {
        assert_wrong_args("false", vec![XPathValue::Boolean(true)]);
    }

    #[test]
    fn test_fn_lang_match() {
        assert_lang("<root xml:lang=\"en\"><child/></root>", true);
    }

    #[test]
    fn test_fn_lang_sublanguage() {
        assert_lang("<root xml:lang=\"en-US\"><child/></root>", true);
    }

    #[test]
    fn test_fn_lang_no_match() {
        assert_lang("<root xml:lang=\"fr\"><child/></root>", false);
    }

    #[test]
    fn test_fn_lang_no_attribute() {
        assert_lang("<root><child/></root>", false);
    }

    #[test]
    fn test_fn_lang_wrong_args() {
        assert_wrong_args("lang", vec![]);
    }
}
