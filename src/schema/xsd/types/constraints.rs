//! XSD identity constraint definitions.

use super::qname::QName;

pub use super::super::constraint_model::IdentityConstraintType as XsdConstraintType;

/// Identity constraint definition (unique, key, keyref).
pub type XsdIdentityConstraint = super::super::constraint_model::IdentityConstraintModel<QName>;
