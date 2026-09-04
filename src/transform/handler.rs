use std::collections::HashMap;

use crate::xpath::XPathSource;

use super::context::TransformContext;
use super::editable::EditableNode;
use super::streamable::IntoStreamable;

pub(crate) struct Handler<'a> {
    pub(crate) xpath: XPathSource,
    pub(crate) callback: HandlerCallback<'a>,
}

pub(crate) type SimpleCallback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;
pub(crate) type ContextCallback<'a> = Box<dyn FnMut(&mut EditableNode, &TransformContext) + 'a>;

pub(crate) enum HandlerCallback<'a> {
    Simple(SimpleCallback<'a>),
    WithContext(ContextCallback<'a>),
}

pub(crate) fn simple_handler<'a, X, F>(xpath: X, callback: F) -> Handler<'a>
where
    X: IntoStreamable,
    F: FnMut(&mut EditableNode) + 'a,
{
    Handler {
        xpath: xpath.into_xpath_source(),
        callback: HandlerCallback::Simple(Box::new(callback)),
    }
}

pub(crate) fn context_handler<'a, X, F>(xpath: X, callback: F) -> Handler<'a>
where
    X: IntoStreamable,
    F: FnMut(&mut EditableNode, &TransformContext) + 'a,
{
    Handler {
        xpath: xpath.into_xpath_source(),
        callback: HandlerCallback::WithContext(Box::new(callback)),
    }
}

pub(crate) fn add_namespace(namespaces: &mut HashMap<String, String>, prefix: &str, uri: &str) {
    namespaces.insert(prefix.to_string(), uri.to_string());
}

pub(crate) fn add_namespaces<I, S1, S2>(namespaces: &mut HashMap<String, String>, iter: I)
where
    I: IntoIterator<Item = (S1, S2)>,
    S1: AsRef<str>,
    S2: AsRef<str>,
{
    for (prefix, uri) in iter {
        add_namespace(namespaces, prefix.as_ref(), uri.as_ref());
    }
}
