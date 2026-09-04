//! Multi-handler streaming processing functions.

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
use super::{
    MultiHandler, MultiHandlerWithContext, MultiTransformHandler, MultiTransformHandlerWithContext,
};

trait HandlerSet {
    fn len(&self) -> usize;
    fn xpath(&self, index: usize) -> &StreamableXPath;
    fn call(&mut self, index: usize, node: &mut EditableNode, context: &TransformContext);
}

struct WithoutContext<'s, 'h>(&'s mut [MultiHandler<'h>]);

struct WithContext<'s, 'h>(&'s mut [MultiHandlerWithContext<'h>]);

macro_rules! handler_set {
    ($adapter:ident, $parameter:ident $(, $argument:ident)?) => {
        impl HandlerSet for $adapter<'_, '_> {
            fn len(&self) -> usize {
                self.0.len()
            }

            fn xpath(&self, index: usize) -> &StreamableXPath {
                self.0[index].0
            }

            fn call(&mut self, index: usize, node: &mut EditableNode, $parameter: &TransformContext) {
                (self.0[index].1)(node $(, $argument)?);
            }
        }
    };
}

handler_set!(WithoutContext, _context);
handler_set!(WithContext, context, context);

struct HandlerProgress {
    builder: Option<EditableNodeBuilder>,
    context: Option<TransformContext>,
}

#[derive(Default)]
struct StreamingProgress {
    active_handler: Option<usize>,
    written: usize,
    transform_count: usize,
}

fn handler_progress(count: usize) -> Vec<HandlerProgress> {
    (0..count)
        .map(|_| HandlerProgress {
            builder: None,
            context: None,
        })
        .collect()
}

fn new_builder(namespaces: &HashMap<String, String>) -> EditableNodeBuilder {
    let mut builder = EditableNodeBuilder::new();
    builder.set_namespaces(namespaces.clone());
    builder
}

fn drive_events<F>(input: &str, mut handle: F) -> TransformResult<()>
where
    F: for<'event> FnMut(
        usize,
        usize,
        Result<Event<'event>, quick_xml::Error>,
    ) -> TransformResult<bool>,
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();

    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buffer);
        let after = reader.buffer_position() as usize;
        if !handle(before, after, event)? {
            return Ok(());
        }
        buffer.clear();
    }
}

fn track_element(
    tracker: &mut PathTracker,
    element: &quick_xml::events::BytesStart<'_>,
    offset: usize,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    tracker.push_element(extract_element_info(element, offset, namespaces)?);
    Ok(())
}

fn add_raw_content(
    states: &mut [HandlerProgress],
    bytes: &[u8],
    add: fn(&mut EditableNodeBuilder, &str),
) -> TransformResult<()> {
    if states.iter().any(|state| state.builder.is_some()) {
        let text = std::str::from_utf8(bytes).map_err(TransformError::Utf8)?;
        for state in states {
            if let Some(builder) = &mut state.builder {
                add(builder, text);
            }
        }
    }
    Ok(())
}

fn process_for_each_handlers<H>(
    input: &str,
    mut handlers: H,
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize>
where
    H: HandlerSet,
{
    let mut tracker = PathTracker::new();
    let mut states = handler_progress(handlers.len());
    let mut match_count = 0;

    drive_events(input, |before_pos, after_pos, event| {
        match event {
            Ok(Event::Start(e)) => {
                track_element(&mut tracker, &e, before_pos, namespaces)?;
                for (index, state) in states.iter_mut().enumerate() {
                    if let Some(builder) = &mut state.builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(handlers.xpath(index)) {
                        state.context = Some(tracker.to_context());
                        let mut builder = new_builder(namespaces);
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        state.builder = Some(builder);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                track_element(&mut tracker, &e, before_pos, namespaces)?;
                for (index, state) in states.iter_mut().enumerate() {
                    if let Some(builder) = &mut state.builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(handlers.xpath(index)) {
                        let context = tracker.to_context();
                        let mut builder = new_builder(namespaces);
                        add_empty_to_builder(&mut builder, &e, namespaces)?;
                        call_handler(builder, &mut handlers, index, &context)?;
                        match_count += 1;
                    }
                }
                tracker.pop_element();
            }
            Ok(Event::End(e)) => {
                for (index, state) in states.iter_mut().enumerate() {
                    if let Some(mut builder) = state.builder.take() {
                        add_end_to_builder(&mut builder, &e)?;
                        if builder.is_complete() {
                            let context =
                                state.context.take().unwrap_or_else(|| tracker.to_context());
                            call_handler(builder, &mut handlers, index, &context)?;
                            match_count += 1;
                        } else {
                            state.builder = Some(builder);
                        }
                    }
                }
                tracker.pop_element();
            }
            Ok(Event::Text(e)) => {
                if states.iter().any(|state| state.builder.is_some()) {
                    let text = e
                        .text_content()
                        .map_err(|error| TransformError::XmlParse(error.to_string()))?;
                    for state in &mut states {
                        if let Some(builder) = &mut state.builder {
                            builder.text(&text);
                        }
                    }
                }
            }
            Ok(Event::CData(e)) => {
                add_raw_content(&mut states, &e, EditableNodeBuilder::cdata)?;
            }
            Ok(Event::Comment(e)) => {
                add_raw_content(&mut states, &e, EditableNodeBuilder::comment)?;
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(error) => return Err(parse_error(error, after_pos, input, &tracker)),
        }
        Ok(true)
    })?;

    Ok(match_count)
}

fn process_streaming_handlers<H, W>(
    input: &str,
    mut handlers: H,
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize>
where
    H: HandlerSet,
    W: Write,
{
    let mut tracker = PathTracker::new();
    let mut states = handler_progress(handlers.len());
    let mut progress = StreamingProgress::default();
    let input_bytes = input.as_bytes();

    drive_events(input, |before_pos, after_pos, event| {
        match event {
            Ok(Event::Start(e)) => {
                track_element(&mut tracker, &e, before_pos, namespaces)?;
                if let Some(index) = progress.active_handler {
                    if let Some(builder) = &mut states[index].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    }
                } else if let Some(index) = first_match(&handlers, &tracker) {
                    writer.write_all(&input_bytes[progress.written..before_pos])?;
                    states[index].context = Some(tracker.to_context());
                    let mut builder = new_builder(namespaces);
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    states[index].builder = Some(builder);
                    progress.active_handler = Some(index);
                }
            }
            Ok(Event::Empty(e)) => {
                track_element(&mut tracker, &e, before_pos, namespaces)?;
                if let Some(index) = progress.active_handler {
                    if let Some(builder) = &mut states[index].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    }
                } else if let Some(index) = first_match(&handlers, &tracker) {
                    writer.write_all(&input_bytes[progress.written..before_pos])?;
                    let context = tracker.to_context();
                    let mut builder = new_builder(namespaces);
                    add_empty_to_builder(&mut builder, &e, namespaces)?;
                    transform_handler(builder, &mut handlers, index, &context, writer)?;
                    progress.transform_count += 1;
                    progress.written = after_pos;
                }
                tracker.pop_element();
            }
            Ok(Event::End(e)) => {
                if let Some(index) = progress.active_handler {
                    if let Some(mut builder) = states[index].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;
                        if builder.is_complete() {
                            let context = states[index]
                                .context
                                .take()
                                .unwrap_or_else(|| tracker.to_context());
                            transform_handler(builder, &mut handlers, index, &context, writer)?;
                            progress.transform_count += 1;
                            progress.written = after_pos;
                            progress.active_handler = None;
                        } else {
                            states[index].builder = Some(builder);
                        }
                    }
                }
                tracker.pop_element();
            }
            Ok(Event::Text(e)) => {
                if let Some(builder) = active_builder(&mut states, progress.active_handler) {
                    let text = e
                        .text_content()
                        .map_err(|error| TransformError::XmlParse(error.to_string()))?;
                    builder.text(&text);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(builder) = active_builder(&mut states, progress.active_handler) {
                    builder.cdata(std::str::from_utf8(&e).map_err(TransformError::Utf8)?);
                }
            }
            Ok(Event::Comment(e)) => {
                if let Some(builder) = active_builder(&mut states, progress.active_handler) {
                    builder.comment(std::str::from_utf8(&e).map_err(TransformError::Utf8)?);
                }
            }
            Ok(Event::Eof) => {
                writer.write_all(&input_bytes[progress.written..])?;
                return Ok(false);
            }
            Ok(_) => {}
            Err(error) => return Err(parse_error(error, after_pos, input, &tracker)),
        }
        Ok(true)
    })?;

    Ok(progress.transform_count)
}

fn first_match<H: HandlerSet>(handlers: &H, tracker: &PathTracker) -> Option<usize> {
    (0..handlers.len()).find(|&index| tracker.matches(handlers.xpath(index)))
}

fn active_builder(
    states: &mut [HandlerProgress],
    active_handler: Option<usize>,
) -> Option<&mut EditableNodeBuilder> {
    active_handler.and_then(|index| states[index].builder.as_mut())
}

fn call_handler<H: HandlerSet>(
    builder: EditableNodeBuilder,
    handlers: &mut H,
    index: usize,
    context: &TransformContext,
) -> TransformResult<EditableNode> {
    let mut editable = builder.build()?;
    handlers.call(index, &mut editable, context);
    Ok(editable)
}

fn transform_handler<H, W>(
    builder: EditableNodeBuilder,
    handlers: &mut H,
    index: usize,
    context: &TransformContext,
    writer: &mut W,
) -> TransformResult<()>
where
    H: HandlerSet,
    W: Write,
{
    let editable = call_handler(builder, handlers, index, context)?;
    if !editable.is_removed() {
        serialize_editable(&editable, writer)?;
    }
    Ok(())
}

fn parse_error(
    error: quick_xml::Error,
    byte_offset: usize,
    input: &str,
    tracker: &PathTracker,
) -> TransformError {
    xml_parse_error_with_location(
        format!("{:?}", error),
        byte_offset,
        input,
        Some(tracker.current_xpath()),
    )
}

/// Processes XML with streaming iteration for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_for_each` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
pub fn process_for_each_multi<'a>(
    input: &str,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    process_for_each_handlers(input, WithoutContext(handlers), namespaces)
}

/// Processes XML with streaming iteration and context for multiple XPath handlers in a single pass.
pub fn process_for_each_multi_with_context<'a>(
    xml: &str,
    context_handlers: &mut [MultiHandlerWithContext<'a>],
    namespace_bindings: &HashMap<String, String>,
) -> TransformResult<usize> {
    process_for_each_handlers(xml, WithContext(context_handlers), namespace_bindings)
}

/// Processes XML with streaming transformation for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
///
/// # Strategy: First-Match-Wins
///
/// When multiple handlers could match nested elements, only the **first matching handler**
/// (in registration order) is active at a time. While a handler is processing an element's
/// subtree, other handlers cannot match elements within that subtree.
pub fn process_streaming_multi<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    process_streaming_handlers(input, WithoutContext(handlers), namespaces, writer)
}

/// Processes XML with streaming transformation and context for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming_with_context` multiple times.
///
/// # Strategy: First-Match-Wins
///
/// Uses the same first-match-wins strategy as [`process_streaming_multi`].
pub fn process_streaming_multi_with_context<'a, W: Write>(
    xml: &str,
    context_handlers: &mut [MultiTransformHandlerWithContext<'a>],
    namespace_bindings: &HashMap<String, String>,
    output: &mut W,
) -> TransformResult<usize> {
    process_streaming_handlers(
        xml,
        WithContext(context_handlers),
        namespace_bindings,
        output,
    )
}
