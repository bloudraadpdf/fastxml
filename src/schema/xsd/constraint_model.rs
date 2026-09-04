//! Shared XSD identity constraint model.

/// Type of identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// Identity constraint definition from XSD.
#[derive(Debug, Clone)]
pub struct IdentityConstraintModel<R> {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: IdentityConstraintType,
    /// XPath selector expression
    pub selector: String,
    /// XPath field expressions (one or more for composite keys)
    pub fields: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<R>,
}

impl<R> IdentityConstraintModel<R> {
    /// Creates a new unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: IdentityConstraintType::Unique,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: IdentityConstraintType::Key,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new keyref constraint.
    pub fn keyref(
        name: impl Into<String>,
        selector: impl Into<String>,
        refer: impl Into<R>,
    ) -> Self {
        Self {
            name: name.into(),
            constraint_type: IdentityConstraintType::KeyRef,
            selector: selector.into(),
            fields: Vec::new(),
            refer: Some(refer.into()),
        }
    }

    /// Adds a field expression.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.fields.push(field.into());
        self
    }

    /// Adds multiple field expressions.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.fields.extend(fields.into_iter().map(Into::into));
        self
    }
}
