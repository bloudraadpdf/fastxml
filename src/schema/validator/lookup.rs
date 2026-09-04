use std::collections::HashSet;
use std::sync::Arc;

use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    SimpleType, TypeDef,
};
use crate::schema::xsd::facets::FacetConstraints;

pub(super) fn facet_constraints(simple: &SimpleType) -> FacetConstraints {
    let mut constraints = FacetConstraints::new();
    if let Some(minimum) = simple.min_length {
        constraints = constraints.with_min_length(minimum as usize);
    }
    if let Some(maximum) = simple.max_length {
        constraints = constraints.with_max_length(maximum as usize);
    }
    if let Some(minimum) = &simple.min_inclusive {
        constraints = constraints.with_min_inclusive(minimum.clone());
    }
    if let Some(maximum) = &simple.max_inclusive {
        constraints = constraints.with_max_inclusive(maximum.clone());
    }
    if !simple.enumeration.is_empty() {
        constraints = constraints.with_enumeration(simple.enumeration.clone());
    }
    if let Some(pattern) = &simple.pattern {
        constraints = constraints.with_pattern(pattern.clone());
    }
    constraints
}

pub(super) fn flatten_complex_type(
    schema: &CompiledSchema,
    complex: &ComplexType,
) -> FlattenedChildren {
    let content_model_type = match &complex.content {
        ContentModel::Sequence(_)
        | ContentModel::ComplexExtension { .. }
        | ContentModel::Any { .. } => ContentModelType::Sequence,
        ContentModel::Choice(_) => ContentModelType::Choice,
        ContentModel::All(_) => ContentModelType::All,
        ContentModel::Empty | ContentModel::SimpleContent { .. } => ContentModelType::Empty,
    };
    let mut flattened = FlattenedChildren::with_content_model(content_model_type);
    let elements = inherited_elements(schema, complex, &mut HashSet::new());
    for element in &elements {
        flattened.constraints.insert(
            element.name.clone(),
            (element.min_occurs, element.max_occurs),
        );
    }
    flattened.ordered_elements = Arc::from(
        elements
            .into_iter()
            .map(|element| element.name)
            .collect::<Vec<_>>(),
    );
    flattened
}

pub(super) fn inherited_elements(
    schema: &CompiledSchema,
    complex: &ComplexType,
    visited: &mut HashSet<String>,
) -> Vec<ElementDef> {
    let mut elements = Vec::new();
    match &complex.content {
        ContentModel::Sequence(children)
        | ContentModel::Choice(children)
        | ContentModel::All(children) => {
            elements.extend(children.iter().cloned());
        }
        ContentModel::ComplexExtension {
            base_type,
            elements: extension_elements,
        } => {
            if visited.insert(base_type.clone()) {
                if let Some(TypeDef::Complex(base)) = schema.get_type(base_type) {
                    elements.extend(inherited_elements(schema, base, visited));
                }
            }
            elements.extend(extension_elements.iter().cloned());
        }
        _ => {}
    }
    elements
}
