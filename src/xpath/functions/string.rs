//! String Functions.
//!
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

use crate::error::Result;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

fn wrong_argument_count<T>(function: &str, expected: &str, found: usize) -> Result<T> {
    Err(XPathEvalError::WrongArgumentCount {
        function: function.to_string(),
        expected: expected.to_string(),
        found,
    }
    .into())
}

fn string_or_context(
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
    function: &str,
) -> Result<String> {
    match args.len() {
        0 => Ok(ctx.node.get_content().unwrap_or_default()),
        1 => Ok(args[0].to_string_value()),
        found => wrong_argument_count(function, "0 or 1", found),
    }
}

fn string_arguments<const N: usize>(args: Vec<XPathValue>, function: &str) -> Result<[String; N]> {
    let found = args.len();
    let values: [XPathValue; N] = match args.try_into() {
        Ok(values) => values,
        Err(_) => return wrong_argument_count(function, &N.to_string(), found),
    };
    Ok(values.map(|value| value.to_string_value()))
}

/// `string([object])` - converts the argument to a string.
pub fn fn_string(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    Ok(XPathValue::String(string_or_context(args, ctx, "string")?))
}

/// `concat(string, string, ...)` - concatenates all arguments.
pub fn fn_concat(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 {
        return wrong_argument_count("concat", "at least 2", args.len());
    }

    let result: String = args.into_iter().map(|v| v.to_string_value()).collect();

    Ok(XPathValue::String(result))
}

/// `starts-with(string, string)` - returns true if first string starts with second.
pub fn fn_starts_with(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let [string, prefix] = string_arguments(args, "starts-with")?;
    Ok(XPathValue::Boolean(string.starts_with(&prefix)))
}

/// `contains(haystack, needle)` - returns true if haystack contains needle.
pub fn fn_contains(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let [haystack, needle] = string_arguments(args, "contains")?;
    Ok(XPathValue::Boolean(haystack.contains(&needle)))
}

/// `substring(string, start, [length])` - extracts a substring.
///
/// Note: XPath uses 1-based indexing and rounds to nearest integer.
pub fn fn_substring(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 || args.len() > 3 {
        return wrong_argument_count("substring", "2 or 3", args.len());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let start = iter.next().unwrap().to_number();
    let length = iter.next().map(|v| v.to_number());

    // Handle NaN cases
    if start.is_nan() {
        return Ok(XPathValue::String(String::new()));
    }

    // XPath substring uses 1-based indexing with round()
    let start_idx = (start.round() as i64 - 1).max(0) as usize;

    let chars: Vec<char> = string.chars().collect();

    let result = if let Some(len) = length {
        if len.is_nan() || len <= 0.0 {
            String::new()
        } else {
            // Handle case where start is negative
            let actual_start = (start.round() as i64 - 1).max(0) as usize;
            let end_idx = ((start.round() + len.round()) as i64 - 1).max(0) as usize;
            let actual_len = end_idx.saturating_sub(actual_start);

            chars.iter().skip(actual_start).take(actual_len).collect()
        }
    } else {
        chars.iter().skip(start_idx).collect()
    };

    Ok(XPathValue::String(result))
}

/// `substring-before(string, string)` - returns substring before first occurrence.
pub fn fn_substring_before(
    args: Vec<XPathValue>,
    _ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    let [string, search] = string_arguments(args, "substring-before")?;

    let result = if search.is_empty() {
        String::new()
    } else if let Some(idx) = string.find(&search) {
        string[..idx].to_string()
    } else {
        String::new()
    };

    Ok(XPathValue::String(result))
}

/// `substring-after(string, string)` - returns substring after first occurrence.
pub fn fn_substring_after(
    args: Vec<XPathValue>,
    _ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    let [string, search] = string_arguments(args, "substring-after")?;

    let result = if search.is_empty() {
        string
    } else if let Some(idx) = string.find(&search) {
        string[idx + search.len()..].to_string()
    } else {
        String::new()
    };

    Ok(XPathValue::String(result))
}

/// `string-length([string])` - returns the length of the string.
pub fn fn_string_length(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let string = string_or_context(args, ctx, "string-length")?;
    Ok(XPathValue::Number(string.chars().count() as f64))
}

/// `normalize-space([string])` - strips leading/trailing whitespace and collapses internal.
pub fn fn_normalize_space(
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    let string = string_or_context(args, ctx, "normalize-space")?;
    let result: String = string.split_whitespace().collect::<Vec<_>>().join(" ");

    Ok(XPathValue::String(result))
}

/// `translate(string, from, to)` - replaces characters.
pub fn fn_translate(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let [string, from, to] = string_arguments(args, "translate")?;

    let from_chars: Vec<char> = from.chars().collect();
    let to_chars: Vec<char> = to.chars().collect();

    let result: String = string
        .chars()
        .filter_map(|c| {
            if let Some(idx) = from_chars.iter().position(|&fc| fc == c) {
                if idx < to_chars.len() {
                    Some(to_chars[idx])
                } else {
                    None
                }
            } else {
                Some(c)
            }
        })
        .collect();

    Ok(XPathValue::String(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::namespace::NamespaceResolver;
    use crate::xpath::functions::evaluate_function;

    const TEST_XML: &str = "<root><item id=\"1\">10</item><item id=\"2\">20</item></root>";

    fn text(value: &str) -> XPathValue {
        XPathValue::String(value.to_string())
    }

    fn texts<const N: usize>(values: [&str; N]) -> Vec<XPathValue> {
        values.into_iter().map(text).collect()
    }

    fn evaluate_in(xml: &str, function: &str, args: Vec<XPathValue>) -> Result<XPathValue> {
        let document = crate::parse(xml).unwrap();
        let root = document.get_root_element().unwrap();
        let context = EvaluationContext::new(root, &document, NamespaceResolver::new());
        evaluate_function(function, args, &context)
    }

    fn evaluate(function: &str, args: Vec<XPathValue>) -> Result<XPathValue> {
        evaluate_in(TEST_XML, function, args)
    }

    fn assert_string(function: &str, args: Vec<XPathValue>, expected: &str) {
        assert_eq!(
            evaluate(function, args).unwrap().to_string_value(),
            expected
        );
    }

    fn assert_boolean(function: &str, args: Vec<XPathValue>, expected: bool) {
        assert_eq!(evaluate(function, args).unwrap().to_boolean(), expected);
    }

    #[test]
    fn string_and_concat_values() {
        assert_string("string", vec![XPathValue::Number(42.0)], "42");
        let result = evaluate_in("<root>hello</root>", "string", vec![]).unwrap();
        assert_eq!(result.to_string_value(), "hello");
        assert_string("concat", texts(["Hello", " ", "World"]), "Hello World");
    }

    #[test]
    fn string_predicates() {
        assert_boolean("starts-with", texts(["Hello World", "Hello"]), true);
        assert_boolean("starts-with", texts(["Hello World", "World"]), false);
        assert_boolean("contains", texts(["Hello World", "o W"]), true);
        assert_boolean("contains", texts(["Hello World", "xyz"]), false);
    }

    #[test]
    fn substring_ranges() {
        assert_string(
            "substring",
            vec![text("12345"), XPathValue::Number(2.0)],
            "2345",
        );
        assert_string(
            "substring",
            vec![
                text("12345"),
                XPathValue::Number(2.0),
                XPathValue::Number(3.0),
            ],
            "234",
        );
        assert_string(
            "substring",
            vec![text("12345"), XPathValue::Number(f64::NAN)],
            "",
        );
        assert_string(
            "substring",
            vec![
                text("12345"),
                XPathValue::Number(1.0),
                XPathValue::Number(f64::NAN),
            ],
            "",
        );
    }

    #[test]
    fn substring_boundaries() {
        for (function, expected) in [("substring-before", "1999"), ("substring-after", "04/01")] {
            assert_string(function, texts(["1999/04/01", "/"]), expected);
        }
        for function in ["substring-before", "substring-after"] {
            assert_string(function, texts(["hello", "xyz"]), "");
        }
        assert_string("substring-before", texts(["hello", ""]), "");
        assert_string("substring-after", texts(["hello", ""]), "hello");
    }

    #[test]
    fn string_length_counts_characters() {
        assert_eq!(
            evaluate("string-length", texts(["hello"]))
                .unwrap()
                .to_number(),
            5.0
        );
        assert_eq!(
            evaluate("string-length", texts(["日本語"]))
                .unwrap()
                .to_number(),
            3.0
        );
        let result = evaluate_in("<root>hello</root>", "string-length", vec![]).unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn normalize_space_values() {
        assert_string(
            "normalize-space",
            texts(["  hello   world  "]),
            "hello world",
        );
        let result =
            evaluate_in("<root>  hello   world  </root>", "normalize-space", vec![]).unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn translate_values() {
        assert_string("translate", texts(["bar", "abc", "ABC"]), "BAr");
        assert_string("translate", texts(["--aaa--", "abc-", "ABC"]), "AAA");
    }

    #[test]
    fn wrong_argument_counts() {
        let cases = [
            (
                "string",
                vec![XPathValue::Number(1.0), XPathValue::Number(2.0)],
            ),
            ("concat", texts(["only one"])),
            ("starts-with", texts(["test"])),
            ("contains", vec![]),
            ("substring", texts(["test"])),
            ("substring-before", vec![]),
            ("substring-after", vec![]),
            ("string-length", texts(["a", "b"])),
            ("normalize-space", texts(["a", "b"])),
            ("translate", texts(["test", "abc"])),
        ];

        for (function, args) in cases {
            assert!(evaluate(function, args).is_err(), "{function}");
        }
    }
}
