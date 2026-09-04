//! Tests for the transform module.

use crate::transform::EditableNode;

mod builder_tests;
mod context_tests;
mod multi_tests;
mod reader_tests;

fn content(node: &mut EditableNode) -> String {
    node.get_content().unwrap_or_default()
}

fn content_collector(values: &mut Vec<String>) -> impl FnMut(&mut EditableNode) + '_ {
    move |node| values.push(content(node))
}

fn assert_single_for_each(run: impl FnOnce(&str, &mut Vec<String>)) {
    let mut ids = Vec::new();
    run(r#"<root><item id="1"/><item id="2"/></root>"#, &mut ids);
    assert_eq!(ids, vec!["1", "2"]);
}

fn assert_multiple_for_each(run: impl FnOnce(&str, &mut Vec<String>, &mut Vec<String>)) {
    let mut items = Vec::new();
    let mut others = Vec::new();
    run(
        r#"<root><item>A</item><other>B</other></root>"#,
        &mut items,
        &mut others,
    );
    assert_eq!(items, vec!["A"]);
    assert_eq!(others, vec!["B"]);
}
