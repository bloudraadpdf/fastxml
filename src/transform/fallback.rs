//! Two-pass fallback processor for non-streamable XPath expressions.
//!
//! When an XPath expression cannot be processed in a single streaming pass
//! (e.g., uses `last()`, backward axes, or complex predicates), this module
//! provides a two-pass approach:
//!
//! 1. **Pass 1**: Parse the document and evaluate XPath to find matching nodes
//! 2. **Pass 2**: Stream through the input, transforming matched nodes

use crate::TextContent;
use std::collections::HashSet;
use std::io::Write;
use std::ops::Range;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::document::XmlDocument;
use crate::namespace::Namespace;
use crate::parser::parse;
use crate::xpath::XPathResult;
use crate::xpath::evaluator::evaluate;

use super::editable::{EditableNode, EditableNodeBuilder};
use super::error::{TransformError, TransformResult};
use super::streaming::serialize_editable;

/// Processes XML using two-pass fallback for non-streamable XPath.
pub fn process_fallback<W, F>(
    input: &str,
    xpath_expr: &str,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let (doc, matching_node_ids) = matching_nodes(input, xpath_expr, "transformation")?;

    if matching_node_ids.is_empty() {
        // No matches, write input unchanged
        writer.write_all(input.as_bytes())?;
        return Ok(0);
    }

    // Pass 2: Stream through and transform matches
    process_with_matches(input, &doc, &matching_node_ids, &mut transform_fn, writer)
}

/// Second pass: stream through input, transforming matched nodes.
fn process_with_matches<W, F>(
    input: &str,
    doc: &XmlDocument,
    matching_ids: &HashSet<usize>,
    transform_fn: &mut F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut prev_written: usize = 0;
    let transform_count = visit_matches(input, doc, matching_ids, |editable, source_range| {
        writer.write_all(&input.as_bytes()[prev_written..source_range.start])?;
        transform_fn(editable);
        if !editable.is_removed() {
            serialize_editable(editable, writer)?;
        }
        prev_written = source_range.end;
        Ok(())
    })?;
    writer.write_all(&input.as_bytes()[prev_written..])?;
    Ok(transform_count)
}

fn matching_nodes(
    input: &str,
    xpath_expr: &str,
    operation: &str,
) -> TransformResult<(XmlDocument, HashSet<usize>)> {
    let doc = parse(input).map_err(|error| TransformError::XmlParse(error.to_string()))?;
    let result = evaluate(&doc, xpath_expr)
        .map_err(|error| TransformError::InvalidXPath(error.to_string()))?;
    let XPathResult::Nodes(nodes) = result else {
        return Err(TransformError::InvalidXPath(format!(
            "XPath must return nodes for {operation}"
        )));
    };
    let matching_ids = nodes.iter().map(|node| node.id()).collect();
    Ok((doc, matching_ids))
}

/// Find the nth child element ID of a parent node.
fn find_child_element_id(doc: &XmlDocument, parent_id: usize, child_index: usize) -> Option<usize> {
    let parent = doc.get_node(parent_id)?;
    let children = parent.get_child_elements();
    children.get(child_index).map(|n| n.id())
}

fn current_child_element_id(
    doc: &XmlDocument,
    node_stack: &[usize],
    child_indices: &[usize],
) -> Option<usize> {
    find_child_element_id(
        doc,
        node_stack.last().copied().unwrap_or(0),
        child_indices.last().copied().unwrap_or(0),
    )
}

fn add_event_to_builder(builder: &mut EditableNodeBuilder, event: &Event) -> TransformResult<()> {
    match event {
        Event::Start(e) => {
            let name_bytes = e.name();
            let full_name =
                std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

            let (prefix, name) = match full_name.split_once(':') {
                Some((p, n)) => (Some(p), n),
                None => (None, full_name),
            };

            let mut attributes = Vec::new();
            let mut ns_decls = Vec::new();

            for attr in e.attributes().filter_map(|a| a.ok()) {
                let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
                let value = crate::decode_attribute_value(&attr, e.decoder())
                    .map_err(|err| TransformError::XmlParse(err.to_string()))?;

                if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
                    ns_decls.push(Namespace::new(ns_prefix, value.as_ref()));
                } else if key == "xmlns" {
                    ns_decls.push(Namespace::new("", value.as_ref()));
                } else {
                    attributes.push((key.to_string(), value.to_string()));
                }
            }

            let attr_refs: Vec<(&str, &str)> = attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            builder.start_element(name, prefix, None, attr_refs, vec![], ns_decls);
        }

        Event::Empty(e) => {
            add_event_to_builder(builder, &Event::Start(e.to_owned()))?;
            builder.end_element();
        }

        Event::End(_) => {
            builder.end_element();
        }

        _ => {}
    }

    Ok(())
}

/// Processes XML for iteration without transformation (two-pass fallback).
pub fn process_for_each<F>(input: &str, xpath_expr: &str, mut callback: F) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    let (doc, matching_node_ids) = matching_nodes(input, xpath_expr, "iteration")?;

    if matching_node_ids.is_empty() {
        return Ok(0);
    }

    visit_matches(input, &doc, &matching_node_ids, |editable, _| {
        callback(editable);
        Ok(())
    })
}

struct MatchedSubtree {
    builder: EditableNodeBuilder,
    start_position: usize,
}

fn visit_matches<F>(
    input: &str,
    doc: &XmlDocument,
    matching_ids: &HashSet<usize>,
    mut visitor: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode, Range<usize>) -> TransformResult<()>,
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut node_stack: Vec<usize> = vec![0];
    let mut current_child_index: Vec<usize> = vec![0];
    let mut match_count = 0;
    let mut buf = Vec::new();
    let mut matched_subtree: Option<MatchedSubtree> = None;

    loop {
        let before_position = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let expected_id = current_child_element_id(doc, &node_stack, &current_child_index);

                if let Some(MatchedSubtree { builder, .. }) = &mut matched_subtree {
                    add_event_to_builder(builder, &Event::Start(e.clone()))?;
                } else if expected_id.is_some_and(|id| matching_ids.contains(&id)) {
                    let mut builder = EditableNodeBuilder::new();
                    add_event_to_builder(&mut builder, &Event::Start(e.clone()))?;
                    matched_subtree = Some(MatchedSubtree {
                        builder,
                        start_position: before_position,
                    });
                }

                if let Some(id) = expected_id {
                    node_stack.push(id);
                }
                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
                current_child_index.push(0);
            }

            Ok(Event::Empty(e)) => {
                let after_position = reader.buffer_position() as usize;
                let expected_id = current_child_element_id(doc, &node_stack, &current_child_index);

                if let Some(MatchedSubtree { builder, .. }) = &mut matched_subtree {
                    add_event_to_builder(builder, &Event::Empty(e.clone()))?;
                } else if expected_id.is_some_and(|id| matching_ids.contains(&id)) {
                    let mut builder = EditableNodeBuilder::new();
                    add_event_to_builder(&mut builder, &Event::Empty(e.clone()))?;
                    let mut editable = builder.build()?;
                    visitor(&mut editable, before_position..after_position)?;
                    match_count += 1;
                }

                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
            }

            Ok(Event::End(e)) => {
                let after_position = reader.buffer_position() as usize;
                if let Some(mut subtree) = matched_subtree.take() {
                    add_event_to_builder(&mut subtree.builder, &Event::End(e.clone()))?;

                    if subtree.builder.is_complete() {
                        let mut editable = subtree.builder.build()?;
                        visitor(&mut editable, subtree.start_position..after_position)?;
                        match_count += 1;
                    } else {
                        matched_subtree = Some(subtree);
                    }
                }

                node_stack.pop();
                current_child_index.pop();
            }

            Ok(Event::Text(e)) => {
                if let Some(MatchedSubtree { builder, .. }) = &mut matched_subtree {
                    let text = e
                        .text_content()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(MatchedSubtree { builder, .. }) = &mut matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(MatchedSubtree { builder, .. }) = &mut matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                return Err(TransformError::XmlParse(format!(
                    "Error at position {}: {:?}",
                    reader.buffer_position(),
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_with_last() {
        let input = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let mut output = Vec::new();
        let count = process_fallback(
            input,
            "//item[last()]",
            |node| {
                node.set_attribute("last", "true");
            },
            &mut output,
        )
        .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 1);
        // The last item should have the attribute
        assert!(result.contains(r#"last="true""#));
    }

    #[test]
    fn test_fallback_no_match() {
        let input = r#"<root><item>A</item></root>"#;

        let mut output = Vec::new();
        let count = process_fallback(input, "//nonexistent", |_node| {}, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 0);
        assert_eq!(result, input);
    }
}
