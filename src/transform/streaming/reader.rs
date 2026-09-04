//! Reader-based streaming processing functions.

use crate::TextContent;
use std::collections::HashMap;
use std::io::{BufRead, Write};

use quick_xml::Reader;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::writer::Writer as XmlWriter;

use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    PathTracker, add_empty_to_builder, add_end_to_builder, add_start_to_builder,
    extract_element_info, serialize_editable, xml_parse_error_at_offset,
};
use super::{HandlerState, MultiHandler, MultiTransformHandler, TransformHandlerState};

fn new_reader<R: BufRead>(reader: R) -> (Reader<R>, PathTracker, Vec<u8>) {
    let mut reader = Reader::from_reader(reader);
    reader.config_mut().trim_text(false);
    (reader, PathTracker::new(), Vec::new())
}

fn reader_error<R: BufRead>(
    reader: &Reader<R>,
    tracker: &PathTracker,
    error: impl std::fmt::Debug,
) -> TransformError {
    xml_parse_error_at_offset(
        format!("{error:?}"),
        reader.buffer_position() as usize,
        Some(tracker.current_xpath()),
    )
}

fn new_builder(namespaces: &HashMap<String, String>) -> EditableNodeBuilder {
    let mut builder = EditableNodeBuilder::new();
    builder.set_namespaces(namespaces.clone());
    builder
}

fn write_event<W: Write>(writer: &mut XmlWriter<W>, event: Event<'_>) -> TransformResult<()> {
    writer
        .write_event(event)
        .map_err(|err| TransformError::Io(std::io::Error::other(err)))
}

fn add_content_to_builder(
    builder: &mut EditableNodeBuilder,
    event: &Event<'_>,
) -> TransformResult<()> {
    match event {
        Event::Text(text) => {
            let text = text
                .text_content()
                .map_err(|err| TransformError::XmlParse(err.to_string()))?;
            builder.text(&text);
        }
        Event::CData(text) => {
            builder.cdata(std::str::from_utf8(text).map_err(TransformError::Utf8)?)
        }
        Event::Comment(text) => {
            builder.comment(std::str::from_utf8(text).map_err(TransformError::Utf8)?)
        }
        _ => unreachable!("content handler requires text, CDATA, or comment"),
    }
    Ok(())
}

fn push_element(
    tracker: &mut PathTracker,
    element: &BytesStart<'_>,
    position: usize,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    tracker.push_element(extract_element_info(element, position, namespaces)?);
    Ok(())
}

fn apply_transform<W, F>(
    builder: EditableNodeBuilder,
    transform: &mut F,
    writer: &mut XmlWriter<W>,
    count: &mut usize,
) -> TransformResult<()>
where
    W: Write,
    F: FnMut(&mut EditableNode) + ?Sized,
{
    let mut editable = builder.build()?;
    transform(&mut editable);
    *count += 1;
    if !editable.is_removed() {
        serialize_editable(&editable, writer.get_mut())?;
    }
    Ok(())
}

fn matching_handler(states: &[TransformHandlerState<'_>], tracker: &PathTracker) -> Option<usize> {
    states.iter().position(|state| tracker.matches(state.xpath))
}

fn active_builder<'a>(
    states: &'a mut [TransformHandlerState<'_>],
    active_handler: Option<usize>,
) -> Option<&'a mut EditableNodeBuilder> {
    active_handler.and_then(|index| states[index].builder.as_mut())
}

fn finish_end<W: Write>(
    writer: &mut XmlWriter<W>,
    tracker: &mut PathTracker,
    event: &BytesEnd<'_>,
    echo: bool,
) -> TransformResult<()> {
    if echo {
        write_event(writer, Event::End(event.clone()))?;
    }
    tracker.pop_element();
    Ok(())
}

fn handle_empty<F>(
    active: Option<&mut EditableNodeBuilder>,
    matched: bool,
    event: &BytesStart<'_>,
    namespaces: &HashMap<String, String>,
    mut on_match: F,
) -> TransformResult<bool>
where
    F: FnMut(EditableNodeBuilder) -> TransformResult<()>,
{
    if let Some(builder) = active {
        add_empty_to_builder(builder, event, namespaces)?;
    } else if matched {
        let mut builder = new_builder(namespaces);
        add_empty_to_builder(&mut builder, event, namespaces)?;
        on_match(builder)?;
    } else {
        return Ok(true);
    }
    Ok(false)
}

fn close_builder<F>(
    builder: Option<EditableNodeBuilder>,
    event: &BytesEnd<'_>,
    mut on_complete: F,
) -> TransformResult<Option<EditableNodeBuilder>>
where
    F: FnMut(EditableNodeBuilder) -> TransformResult<()>,
{
    let Some(mut builder) = builder else {
        return Ok(None);
    };
    add_end_to_builder(&mut builder, event)?;
    if builder.is_complete() {
        on_complete(builder)?;
        Ok(None)
    } else {
        Ok(Some(builder))
    }
}

fn read_events<R, F>(reader: R, mut handle: F) -> TransformResult<()>
where
    R: BufRead,
    F: FnMut(Event<'_>, usize, &mut PathTracker) -> TransformResult<()>,
{
    let (mut reader, mut tracker, mut buffer) = new_reader(reader);
    loop {
        let position = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(event) => handle(event, position, &mut tracker)?,
            Err(error) => return Err(reader_error(&reader, &tracker, error)),
        }
        buffer.clear();
    }
    Ok(())
}

/// Processes XML from a reader with streaming iteration (no transformation output).
///
/// Unlike `process_for_each`, this function reads from a `BufRead` source instead of
/// requiring the entire XML string in memory. This is important for processing large
/// XML files without excessive memory usage.
pub fn process_for_each_from_reader<R, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    R: BufRead,
    F: FnMut(&mut EditableNode),
{
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;

    read_events(reader, |event, position, tracker| {
        match event {
            Event::Start(e) => {
                push_element(tracker, &e, position, namespaces)?;

                if let Some(ref mut builder) = subtree_builder {
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    let mut builder = new_builder(namespaces);
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Event::Empty(e) => {
                push_element(tracker, &e, position, namespaces)?;

                if let Some(ref mut builder) = subtree_builder {
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    let mut builder = new_builder(namespaces);
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    let mut editable = builder.build()?;
                    callback(&mut editable);
                    match_count += 1;
                }

                tracker.pop_element();
            }

            Event::End(e) => {
                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        let mut editable = builder.build()?;
                        callback(&mut editable);
                        match_count += 1;
                    } else {
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            event @ (Event::Text(_) | Event::CData(_) | Event::Comment(_)) => {
                if let Some(ref mut builder) = subtree_builder {
                    add_content_to_builder(builder, &event)?;
                }
            }

            _ => {}
        }

        Ok(())
    })?;

    Ok(match_count)
}

/// Processes XML from a reader with streaming transformation.
///
/// Unlike `process_streaming`, this function reads from a `BufRead` source and uses
/// quick_xml's Writer to echo non-matched events, avoiding the need to keep the entire
/// XML input in memory. Matched elements are still built into a DOM subtree for editing.
///
/// Note: The output may have minor formatting differences from the original XML
/// (e.g., attribute quoting style) for non-matched content, since events are
/// reconstructed through the XML writer rather than copied verbatim.
pub fn process_streaming_from_reader<R, W, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut xml_writer = XmlWriter::new(writer);
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut transform_count: usize = 0;

    read_events(reader, |event, position, tracker| {
        match event {
            Event::Start(ref e) => {
                push_element(tracker, e, position, namespaces)?;

                if let Some(ref mut builder) = subtree_builder {
                    add_start_to_builder(builder, e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts - start building DOM subtree
                    let mut builder = new_builder(namespaces);
                    add_start_to_builder(&mut builder, e, namespaces)?;
                    subtree_builder = Some(builder);
                } else {
                    // No match - echo event to writer
                    write_event(&mut xml_writer, Event::Start(e.clone()))?;
                }
            }

            Event::Empty(ref e) => {
                push_element(tracker, e, position, namespaces)?;
                let echo = handle_empty(
                    subtree_builder.as_mut(),
                    tracker.matches(xpath),
                    e,
                    namespaces,
                    |builder| {
                        apply_transform(
                            builder,
                            &mut transform_fn,
                            &mut xml_writer,
                            &mut transform_count,
                        )
                    },
                )?;
                if echo {
                    write_event(&mut xml_writer, Event::Empty(e.clone()))?;
                }
                tracker.pop_element();
            }

            Event::End(ref e) => {
                let echo = subtree_builder.is_none();
                subtree_builder = close_builder(subtree_builder.take(), e, |builder| {
                    apply_transform(
                        builder,
                        &mut transform_fn,
                        &mut xml_writer,
                        &mut transform_count,
                    )
                })?;
                finish_end(&mut xml_writer, tracker, e, echo)?;
            }

            ref event @ (Event::Text(_) | Event::CData(_) | Event::Comment(_)) => {
                if let Some(ref mut builder) = subtree_builder {
                    add_content_to_builder(builder, event)?;
                } else {
                    write_event(&mut xml_writer, event.clone())?;
                }
            }

            event => {
                // PI, Decl, DocType - pass through
                write_event(&mut xml_writer, event)?;
            }
        }

        Ok(())
    })?;

    Ok(transform_count)
}

/// Processes XML from a reader with streaming iteration for multiple XPath handlers.
///
/// Reader-based version of `process_for_each_multi`. Reads from a `BufRead` source
/// instead of requiring the entire XML string in memory.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi_from_reader<'a, R>(
    reader: R,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize>
where
    R: BufRead,
{
    let mut match_count: usize = 0;

    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
        })
        .collect();

    read_events(reader, |event, position, tracker| {
        match event {
            Event::Start(e) => {
                push_element(tracker, &e, position, namespaces)?;

                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        let mut builder = new_builder(namespaces);
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        states[i].builder = Some(builder);
                    }
                }
            }

            Event::Empty(e) => {
                push_element(tracker, &e, position, namespaces)?;

                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        let mut builder = new_builder(namespaces);
                        add_empty_to_builder(&mut builder, &e, namespaces)?;

                        let mut editable = builder.build()?;
                        handlers[i].1(&mut editable);
                        match_count += 1;
                    }
                }

                tracker.pop_element();
            }

            Event::End(e) => {
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            match_count += 1;
                        } else {
                            states[i].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            event @ (Event::Text(_) | Event::CData(_) | Event::Comment(_)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        add_content_to_builder(builder, &event)?;
                    }
                }
            }

            _ => {}
        }

        Ok(())
    })?;

    Ok(match_count)
}

/// Processes XML from a reader with streaming transformation for multiple XPath handlers.
///
/// Reader-based version of `process_streaming_multi`. Uses quick_xml's Writer to echo
/// non-matched events instead of zero-copy byte slicing, avoiding the need to keep
/// the entire XML input in memory.
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi_from_reader<'a, R, W>(
    reader: R,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
{
    let mut xml_writer = XmlWriter::new(writer);
    let mut transform_count: usize = 0;

    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
        })
        .collect();

    let mut active_handler: Option<usize> = None;

    read_events(reader, |event, position, tracker| {
        match event {
            Event::Start(ref e) => {
                push_element(tracker, e, position, namespaces)?;

                if let Some(builder) = active_builder(&mut states, active_handler) {
                    add_start_to_builder(builder, e, namespaces)?;
                } else if let Some(index) = matching_handler(&states, tracker) {
                    let mut builder = new_builder(namespaces);
                    add_start_to_builder(&mut builder, e, namespaces)?;
                    states[index].builder = Some(builder);
                    active_handler = Some(index);
                } else {
                    write_event(&mut xml_writer, Event::Start(e.clone()))?;
                }
            }

            Event::Empty(ref e) => {
                push_element(tracker, e, position, namespaces)?;
                let match_index = active_handler
                    .is_none()
                    .then(|| matching_handler(&states, tracker))
                    .flatten();
                let echo = handle_empty(
                    active_builder(&mut states, active_handler),
                    match_index.is_some(),
                    e,
                    namespaces,
                    |builder| {
                        apply_transform(
                            builder,
                            &mut handlers[match_index.expect("matched handler")].1,
                            &mut xml_writer,
                            &mut transform_count,
                        )
                    },
                )?;
                if echo {
                    write_event(&mut xml_writer, Event::Empty(e.clone()))?;
                }
                tracker.pop_element();
            }

            Event::End(ref e) => {
                let echo = active_handler.is_none();
                if let Some(idx) = active_handler {
                    states[idx].builder =
                        close_builder(states[idx].builder.take(), e, |builder| {
                            apply_transform(
                                builder,
                                &mut handlers[idx].1,
                                &mut xml_writer,
                                &mut transform_count,
                            )?;
                            active_handler = None;
                            Ok(())
                        })?;
                }
                finish_end(&mut xml_writer, tracker, e, echo)?;
            }

            ref event @ (Event::Text(_) | Event::CData(_) | Event::Comment(_)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        add_content_to_builder(builder, event)?;
                    }
                } else {
                    write_event(&mut xml_writer, event.clone())?;
                }
            }

            event => {
                write_event(&mut xml_writer, event)?;
            }
        }

        Ok(())
    })?;

    Ok(transform_count)
}
