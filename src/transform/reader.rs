//! Reader-based StreamTransformer for memory-efficient processing.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::handler::{
    Handler, HandlerCallback, SimpleCallback, add_namespace, add_namespaces, simple_handler,
};
use super::streamable::IntoStreamable;
use super::streaming;
use super::xpath_analyze::{self, StreamableXPath, XPathAnalysis};

fn unsupported_context_callback() -> TransformError {
    TransformError::InvalidXPath(
        "WithContext callbacks are not supported in reader mode".to_string(),
    )
}

fn simple_callback(handler: Handler<'_>) -> TransformResult<SimpleCallback<'_>> {
    match handler.callback {
        HandlerCallback::Simple(callback) => Ok(callback),
        HandlerCallback::WithContext(_) => Err(unsupported_context_callback()),
    }
}

fn pair_handlers<'a, 'b>(
    xpaths: &'b [StreamableXPath],
    handlers: &'b mut [Handler<'a>],
) -> TransformResult<Vec<streaming::MultiHandler<'b>>>
where
    'a: 'b,
{
    xpaths
        .iter()
        .zip(handlers)
        .map(|(xpath, handler)| match &mut handler.callback {
            HandlerCallback::Simple(callback) => Ok((
                xpath,
                callback.as_mut() as &mut dyn FnMut(&mut EditableNode),
            )),
            HandlerCallback::WithContext(_) => Err(unsupported_context_callback()),
        })
        .collect()
}

/// Builder for streaming XML transformations from a reader source.
///
/// Unlike [`StreamTransformer`](super::StreamTransformer) which requires the entire XML input as a string,
/// this reads from a `BufRead` source, enabling memory-efficient processing of
/// large XML files. Non-matched content is echoed through quick_xml's Writer
/// rather than using zero-copy byte slicing.
///
/// # Example
///
/// ```ignore
/// use std::io::{BufReader, Cursor};
/// use fastxml::transform::StreamTransformerReader;
///
/// let xml = r#"<root><item>A</item><other>B</other></root>"#;
/// let reader = BufReader::new(Cursor::new(xml));
///
/// let mut output = Vec::new();
/// let count = StreamTransformerReader::new(reader)
///     .on("//item", |node| node.set_attribute("processed", "true"))
///     .run_to_writer(&mut output)?;
///
/// let result = String::from_utf8(output).unwrap();
/// assert!(result.contains(r#"processed="true""#));
/// # Ok::<(), fastxml::transform::TransformError>(())
/// ```
pub(crate) struct StreamTransformerReader<'a, R: BufRead> {
    reader: R,
    handlers: Vec<Handler<'a>>,
    namespaces: HashMap<String, String>,
}

impl<'a, R: BufRead> StreamTransformerReader<'a, R> {
    /// Creates a new reader-based transformer.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            handlers: Vec::new(),
            namespaces: HashMap::new(),
        }
    }

    /// Registers an XPath expression with its callback function.
    ///
    /// Only streamable XPath expressions are supported. Non-streamable expressions
    /// (e.g., those using `last()`, backward axes) will return an error at execution time.
    pub fn on<X, F>(mut self, xpath: X, callback: F) -> Self
    where
        X: IntoStreamable,
        F: FnMut(&mut EditableNode) + 'a,
    {
        self.handlers.push(simple_handler(xpath, callback));
        self
    }

    /// Registers a namespace prefix for use in XPath expressions.
    pub fn namespace(mut self, prefix: &str, uri: &str) -> Self {
        add_namespace(&mut self.namespaces, prefix, uri);
        self
    }

    /// Registers multiple namespace prefixes at once.
    pub fn namespaces<I, S1, S2>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = (S1, S2)>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        add_namespaces(&mut self.namespaces, iter);
        self
    }

    /// Executes all registered handlers and writes the result to a writer.
    ///
    /// This is the memory-efficient way to transform XML: the input is read
    /// incrementally and the output is written incrementally, without needing
    /// the entire file in memory.
    ///
    /// Returns the number of elements that were matched and transformed.
    pub fn run_to_writer<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        self.execute_transform_reader(writer)
    }

    /// Executes all registered handlers for their side effects only.
    ///
    /// Unlike `run_to_writer()`, this method does not produce output XML.
    /// Use this when you only need to extract data from a large file.
    pub fn for_each(self) -> TransformResult<()> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        self.execute_for_each_reader()?;
        Ok(())
    }

    /// Internal: Execute transformation with reader source
    fn execute_transform_reader<W: Write>(mut self, writer: &mut W) -> TransformResult<usize> {
        let (streamable_xpaths, callback) = self.prepare()?;
        if let Some(mut callback) = callback {
            return streaming::process_streaming_from_reader(
                self.reader,
                &streamable_xpaths[0],
                &self.namespaces,
                callback.as_mut(),
                writer,
            );
        }

        let mut handler_pairs = pair_handlers(&streamable_xpaths, &mut self.handlers)?;

        streaming::process_streaming_multi_from_reader(
            self.reader,
            &mut handler_pairs,
            &self.namespaces,
            writer,
        )
    }

    /// Internal: Execute for_each with reader source
    fn execute_for_each_reader(mut self) -> TransformResult<usize> {
        let (streamable_xpaths, callback) = self.prepare()?;
        if let Some(mut callback) = callback {
            return streaming::process_for_each_from_reader(
                self.reader,
                &streamable_xpaths[0],
                &self.namespaces,
                callback.as_mut(),
            );
        }

        let mut handler_pairs = pair_handlers(&streamable_xpaths, &mut self.handlers)?;

        streaming::process_for_each_multi_from_reader(
            self.reader,
            &mut handler_pairs,
            &self.namespaces,
        )
    }

    fn prepare(&mut self) -> TransformResult<(Vec<StreamableXPath>, Option<SimpleCallback<'a>>)> {
        let xpaths = self.analyze_xpaths()?;
        let callback = (self.handlers.len() == 1)
            .then(|| simple_callback(self.handlers.remove(0)))
            .transpose()?;
        Ok((xpaths, callback))
    }

    fn analyze_xpaths(&self) -> TransformResult<Vec<StreamableXPath>> {
        self.handlers
            .iter()
            .map(|handler| -> TransformResult<_> {
                let expression = handler.xpath.parse()?;
                match xpath_analyze::analyze_xpath(&expression) {
                    XPathAnalysis::Streamable(xpath) => Ok(xpath),
                    XPathAnalysis::NotStreamable(reason) => Err(TransformError::NotStreamable {
                        xpath: handler
                            .xpath
                            .as_string()
                            .map(str::to_owned)
                            .unwrap_or_else(|| "<ast>".to_string()),
                        reason,
                    }),
                }
            })
            .collect()
    }
}
