//! Single-handler streaming processing functions.

use crate::TextContent;
use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::super::context::TransformContext;
use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    PathTracker, add_empty_to_builder, add_end_to_builder, add_start_to_builder,
    extract_element_info, serialize_editable, xml_parse_error_with_location,
};

trait MatchCallback {
    fn call(&mut self, node: &mut EditableNode, context: &TransformContext);
}

struct WithoutContext<F>(F);

impl<F> MatchCallback for WithoutContext<F>
where
    F: FnMut(&mut EditableNode),
{
    fn call(&mut self, node: &mut EditableNode, _context: &TransformContext) {
        (self.0)(node);
    }
}

struct WithContext<F>(F);

impl<F> MatchCallback for WithContext<F>
where
    F: FnMut(&mut EditableNode, &TransformContext),
{
    fn call(&mut self, node: &mut EditableNode, context: &TransformContext) {
        (self.0)(node, context);
    }
}

trait MatchOutput {
    fn before_match(&mut self, input: &str, offset: usize) -> TransformResult<()>;
    fn emit_match(&mut self, node: &EditableNode) -> TransformResult<()>;
    fn after_match(&mut self, offset: usize);
    fn finish(&mut self, input: &str) -> TransformResult<()>;
}

struct NoOutput;

impl MatchOutput for NoOutput {
    fn before_match(&mut self, _input: &str, _offset: usize) -> TransformResult<()> {
        Ok(())
    }

    fn emit_match(&mut self, _node: &EditableNode) -> TransformResult<()> {
        Ok(())
    }

    fn after_match(&mut self, _offset: usize) {}

    fn finish(&mut self, _input: &str) -> TransformResult<()> {
        Ok(())
    }
}

struct StreamingOutput<'a, W> {
    writer: &'a mut W,
    written: usize,
}

impl<W: Write> MatchOutput for StreamingOutput<'_, W> {
    fn before_match(&mut self, input: &str, offset: usize) -> TransformResult<()> {
        self.writer
            .write_all(&input.as_bytes()[self.written..offset])?;
        Ok(())
    }

    fn emit_match(&mut self, node: &EditableNode) -> TransformResult<()> {
        if !node.is_removed() {
            serialize_editable(node, self.writer)?;
        }
        Ok(())
    }

    fn after_match(&mut self, offset: usize) {
        self.written = offset;
    }

    fn finish(&mut self, input: &str) -> TransformResult<()> {
        self.writer.write_all(&input.as_bytes()[self.written..])?;
        Ok(())
    }
}

fn new_builder(namespaces: &HashMap<String, String>) -> EditableNodeBuilder {
    let mut builder = EditableNodeBuilder::new();
    builder.set_namespaces(namespaces.clone());
    builder
}

fn process_matches<C, O>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: C,
    mut output: O,
) -> TransformResult<usize>
where
    C: MatchCallback,
    O: MatchOutput,
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder = None;
    let mut match_context = None;
    let mut match_count = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                tracker.push_element(extract_element_info(&e, before_pos, namespaces)?);

                if let Some(builder) = &mut subtree_builder {
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    output.before_match(input, before_pos)?;
                    match_context = Some(tracker.to_context());
                    let mut builder = new_builder(namespaces);
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }
            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                tracker.push_element(extract_element_info(&e, before_pos, namespaces)?);

                if let Some(builder) = &mut subtree_builder {
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    output.before_match(input, before_pos)?;
                    let context = tracker.to_context();
                    let mut builder = new_builder(namespaces);
                    add_empty_to_builder(&mut builder, &e, namespaces)?;
                    complete_match(builder, &mut callback, &context, &mut output)?;
                    match_count += 1;
                    output.after_match(after_pos);
                }

                tracker.pop_element();
            }
            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;
                    if builder.is_complete() {
                        let context = match_context.take().unwrap_or_else(|| tracker.to_context());
                        complete_match(builder, &mut callback, &context, &mut output)?;
                        match_count += 1;
                        output.after_match(after_pos);
                    } else {
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }
            Ok(Event::Text(e)) => {
                if let Some(builder) = &mut subtree_builder {
                    let text = e
                        .text_content()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(builder) = &mut subtree_builder {
                    builder.cdata(std::str::from_utf8(&e).map_err(TransformError::Utf8)?);
                }
            }
            Ok(Event::Comment(e)) => {
                if let Some(builder) = &mut subtree_builder {
                    builder.comment(std::str::from_utf8(&e).map_err(TransformError::Utf8)?);
                }
            }
            Ok(Event::Eof) => {
                output.finish(input)?;
                break;
            }
            Ok(_) => {}
            Err(error) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", error),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

fn complete_match<C, O>(
    builder: EditableNodeBuilder,
    callback: &mut C,
    context: &TransformContext,
    output: &mut O,
) -> TransformResult<()>
where
    C: MatchCallback,
    O: MatchOutput,
{
    let mut editable = builder.build()?;
    callback.call(&mut editable, context);
    output.emit_match(&editable)
}

/// Processes XML with streaming transformation.
pub fn process_streaming<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    process_matches(
        input,
        xpath,
        namespaces,
        WithoutContext(transform_fn),
        StreamingOutput { writer, written: 0 },
    )
}

/// Processes XML with streaming transformation and context.
pub fn process_streaming_with_context<W, F>(
    xml: &str,
    query: &StreamableXPath,
    namespace_bindings: &HashMap<String, String>,
    context_callback: F,
    output: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode, &TransformContext),
{
    process_matches(
        xml,
        query,
        namespace_bindings,
        WithContext(context_callback),
        StreamingOutput {
            writer: output,
            written: 0,
        },
    )
}

/// Processes XML with streaming iteration (no transformation output).
pub fn process_for_each<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    process_matches(input, xpath, namespaces, WithoutContext(callback), NoOutput)
}

/// Processes XML with streaming iteration and context (no transformation output).
pub fn process_for_each_with_context<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode, &TransformContext),
{
    process_matches(input, xpath, namespaces, WithContext(callback), NoOutput)
}
