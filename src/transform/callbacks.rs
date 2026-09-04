//! Callback dispatch functions for handler execution.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::context::TransformContext;
use super::error::{TransformError, TransformResult};
use super::fallback;
use super::handler::HandlerCallback;
use super::streaming;
use super::xpath_analyze::{self, StreamableXPath, XPathAnalysis};

enum ExecutionPlan<'a> {
    Streamable(StreamableXPath),
    Fallback(&'a str),
}

fn execution_plan<'a>(
    xpath_source: &'a XPathSource,
    fallback_mode: FallbackMode,
) -> TransformResult<ExecutionPlan<'a>> {
    match xpath_analyze::analyze_xpath(&xpath_source.parse()?) {
        XPathAnalysis::Streamable(streamable) => Ok(ExecutionPlan::Streamable(streamable)),
        XPathAnalysis::NotStreamable(reason) if fallback_mode == FallbackMode::Disabled => {
            Err(TransformError::NotStreamable {
                xpath: xpath_source
                    .as_string()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "<ast>".to_string()),
                reason,
            })
        }
        XPathAnalysis::NotStreamable(_) => xpath_source
            .as_string()
            .map(ExecutionPlan::Fallback)
            .ok_or_else(|| {
                TransformError::InvalidXPath(
                    "XPath AST without string representation cannot use fallback processor. \
                     Use a streamable XPath pattern or provide the expression as a string."
                        .to_string(),
                )
            }),
    }
}

pub(crate) fn stream_transform_with_callback<'a, W: Write>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
    callback: HandlerCallback<'a>,
    writer: &mut W,
) -> TransformResult<usize> {
    match execution_plan(xpath_source, fallback_mode)? {
        ExecutionPlan::Streamable(streamable) => match callback {
            HandlerCallback::Simple(mut f) => {
                streaming::process_streaming(input, &streamable, namespaces, |node| f(node), writer)
            }
            HandlerCallback::WithContext(mut f) => streaming::process_streaming_with_context(
                input,
                &streamable,
                namespaces,
                |node, ctx| f(node, ctx),
                writer,
            ),
        },
        ExecutionPlan::Fallback(xpath) => match callback {
            HandlerCallback::Simple(mut f) => {
                fallback::process_fallback(input, xpath, |node| f(node), writer)
            }
            HandlerCallback::WithContext(mut f) => {
                let empty_ctx = TransformContext::new(vec![], 0, 0);
                fallback::process_fallback(input, xpath, |node| f(node, &empty_ctx), writer)
            }
        },
    }
}

pub(crate) fn stream_for_each_with_callback<'a>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
    callback: HandlerCallback<'a>,
) -> TransformResult<usize> {
    match execution_plan(xpath_source, fallback_mode)? {
        ExecutionPlan::Streamable(streamable) => match callback {
            HandlerCallback::Simple(mut f) => {
                streaming::process_for_each(input, &streamable, namespaces, |node| f(node))
            }
            HandlerCallback::WithContext(mut f) => streaming::process_for_each_with_context(
                input,
                &streamable,
                namespaces,
                |node, ctx| f(node, ctx),
            ),
        },
        ExecutionPlan::Fallback(xpath) => match callback {
            HandlerCallback::Simple(mut f) => {
                fallback::process_for_each(input, xpath, |node| f(node))
            }
            HandlerCallback::WithContext(mut f) => {
                let empty_ctx = TransformContext::new(vec![], 0, 0);
                fallback::process_for_each(input, xpath, |node| f(node, &empty_ctx))
            }
        },
    }
}
