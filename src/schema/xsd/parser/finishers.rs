//! Finisher functions for XSD parsing.

use crate::error::Result;
use crate::schema::xsd::types::*;

use super::XsdParser;
use super::stack_frame::StackFrame;

impl XsdParser {
    fn nearest_complex_type(&mut self) -> Option<&mut XsdComplexType> {
        self.stack.iter_mut().rev().find_map(|frame| match frame {
            StackFrame::ComplexType(complex_type) => Some(complex_type),
            _ => None,
        })
    }

    fn finish_type_definition(&mut self, type_def: XsdTypeDef) {
        match self.stack.last_mut() {
            Some(StackFrame::Schema) | None => self.schema.types.push(type_def),
            Some(StackFrame::Element(element)) => element.inline_type = Some(Box::new(type_def)),
            _ => {}
        }
    }

    fn finish_particle(
        &mut self,
        particle: XsdParticle,
        nested_item: Option<fn(XsdParticle) -> XsdParticleItem>,
    ) {
        let Some(parent) = self.stack.last_mut() else {
            return;
        };
        match parent {
            StackFrame::ComplexType(complex_type) => {
                complex_type.content = XsdComplexContent::Particle(particle);
            }
            StackFrame::Sequence(sequence) => {
                if let Some(make_item) = nested_item {
                    sequence.particles.push(make_item(particle));
                }
            }
            StackFrame::Choice(choice) => {
                if let Some(make_item) = nested_item {
                    choice.particles.push(make_item(particle));
                }
            }
            StackFrame::ComplexContentExtension(extension) => extension.particle = Some(particle),
            StackFrame::ComplexContentRestriction(restriction) => {
                restriction.particle = Some(particle)
            }
            StackFrame::Group(group) => group.particle = Some(particle),
            _ => {}
        }
    }

    fn finish_simple_content(&mut self, derivation: XsdSimpleContentDerivation) {
        if let Some(complex_type) = self.nearest_complex_type() {
            complex_type.content =
                XsdComplexContent::SimpleContent(XsdSimpleContentDef { derivation });
        }
    }

    fn finish_complex_derivation(&mut self, derivation: XsdComplexContentDerivation) {
        if let Some(complex_type) = self.nearest_complex_type() {
            complex_type.content = XsdComplexContent::ComplexContent(XsdComplexContentDef {
                mixed: complex_type.mixed,
                derivation,
            });
        }
    }

    fn finish_simple_type_content(&mut self, content: XsdSimpleTypeContent) {
        if let Some(StackFrame::SimpleType(simple_type)) = self.stack.last_mut() {
            simple_type.content = content;
        }
    }

    pub(super) fn finish_element(&mut self, elem: XsdElement) -> Result<()> {
        // Find parent context
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.elements.push(elem);
                }
                StackFrame::Sequence(seq) => {
                    seq.particles.push(XsdParticleItem::Element(elem));
                }
                StackFrame::Choice(choice) => {
                    choice.particles.push(XsdParticleItem::Element(elem));
                }
                StackFrame::All(all) => {
                    all.elements.push(elem);
                }
                StackFrame::ComplexContentExtension(ext) => {
                    // Element in extension without explicit particle
                    if ext.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        ext.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                StackFrame::ComplexContentRestriction(r) => {
                    // Element in restriction without explicit particle
                    if r.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        r.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                StackFrame::Group(grp) => {
                    // Element directly in group (unusual but possible)
                    if grp.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        grp.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                _ => {}
            }
        } else {
            // Top-level element
            self.schema.elements.push(elem);
        }
        Ok(())
    }

    pub(super) fn finish_complex_type(&mut self, ct: XsdComplexType) -> Result<()> {
        self.finish_type_definition(XsdTypeDef::Complex(ct));
        Ok(())
    }

    pub(super) fn finish_simple_type(&mut self, st: XsdSimpleType) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Attribute(attr) => {
                    attr.inline_type = Some(st);
                    return Ok(());
                }
                StackFrame::SimpleRestriction(r) => {
                    r.inline_base = Some(Box::new(st));
                    return Ok(());
                }
                StackFrame::SimpleList(list) => {
                    list.inline_type = Some(Box::new(st));
                    return Ok(());
                }
                StackFrame::SimpleUnion(union) => {
                    union.inline_types.push(st);
                    return Ok(());
                }
                _ => {}
            }
        }
        self.finish_type_definition(XsdTypeDef::Simple(st));
        Ok(())
    }

    pub(super) fn finish_sequence(&mut self, seq: XsdSequence) -> Result<()> {
        self.finish_particle(
            XsdParticle::Sequence(seq),
            Some(|particle| match particle {
                XsdParticle::Sequence(sequence) => XsdParticleItem::Sequence(sequence),
                _ => unreachable!(),
            }),
        );
        Ok(())
    }

    pub(super) fn finish_choice(&mut self, choice: XsdChoice) -> Result<()> {
        self.finish_particle(
            XsdParticle::Choice(choice),
            Some(|particle| match particle {
                XsdParticle::Choice(choice) => XsdParticleItem::Choice(choice),
                _ => unreachable!(),
            }),
        );
        Ok(())
    }

    pub(super) fn finish_all(&mut self, all: XsdAll) -> Result<()> {
        self.finish_particle(XsdParticle::All(all), None);
        Ok(())
    }

    pub(super) fn finish_attribute(&mut self, attr: XsdAttribute) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.attributes.push(attr);
                }
                StackFrame::ComplexType(ct) => {
                    ct.attributes.push(attr);
                }
                StackFrame::AttributeGroup(ag) => {
                    ag.attributes.push(attr);
                }
                StackFrame::SimpleContentExtension(ext) => {
                    ext.attributes.push(attr);
                }
                StackFrame::SimpleContentRestriction(r) => {
                    r.attributes.push(attr);
                }
                StackFrame::ComplexContentExtension(ext) => {
                    ext.attributes.push(attr);
                }
                StackFrame::ComplexContentRestriction(r) => {
                    r.attributes.push(attr);
                }
                _ => {}
            }
        } else {
            self.schema.attributes.push(attr);
        }
        Ok(())
    }

    pub(super) fn finish_attribute_group(&mut self, ag: XsdAttributeGroup) -> Result<()> {
        // If it's a reference, add it to the parent's attribute groups
        if ag.ref_.is_some() {
            if let Some(ref_qname) = ag.ref_.clone() {
                if let Some(parent) = self.stack.last_mut() {
                    match parent {
                        StackFrame::ComplexType(ct) => {
                            ct.attribute_groups.push(ref_qname);
                        }
                        StackFrame::AttributeGroup(parent_ag) => {
                            parent_ag.attribute_groups.push(ref_qname);
                        }
                        StackFrame::SimpleContentExtension(ext) => {
                            ext.attribute_groups.push(ref_qname);
                        }
                        StackFrame::SimpleContentRestriction(r) => {
                            r.attribute_groups.push(ref_qname);
                        }
                        StackFrame::ComplexContentExtension(ext) => {
                            ext.attribute_groups.push(ref_qname);
                        }
                        StackFrame::ComplexContentRestriction(r) => {
                            r.attribute_groups.push(ref_qname);
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // It's a definition
            if let Some(parent) = self.stack.last_mut() {
                if matches!(parent, StackFrame::Schema) {
                    self.schema.attribute_groups.push(ag);
                }
            } else {
                self.schema.attribute_groups.push(ag);
            }
        }
        Ok(())
    }

    pub(super) fn finish_group(&mut self, grp: XsdGroup) -> Result<()> {
        // If it's a reference, add it to the parent particle
        if grp.ref_.is_some() {
            if let Some(ref_qname) = grp.ref_.clone() {
                // Preserve the occurrence bounds declared at the ref site so they
                // can be propagated to the referenced group's members.
                let group_ref = XsdGroupRef {
                    name: ref_qname,
                    min_occurs: grp.min_occurs,
                    max_occurs: grp.max_occurs,
                };
                if let Some(parent) = self.stack.last_mut() {
                    match parent {
                        StackFrame::Sequence(seq) => {
                            seq.particles.push(XsdParticleItem::GroupRef(group_ref));
                        }
                        StackFrame::Choice(choice) => {
                            choice.particles.push(XsdParticleItem::GroupRef(group_ref));
                        }
                        StackFrame::ComplexType(ct) => {
                            ct.content =
                                XsdComplexContent::Particle(XsdParticle::GroupRef(group_ref));
                        }
                        StackFrame::ComplexContentExtension(ext) => {
                            ext.particle = Some(XsdParticle::GroupRef(group_ref));
                        }
                        StackFrame::ComplexContentRestriction(r) => {
                            r.particle = Some(XsdParticle::GroupRef(group_ref));
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // It's a definition
            if let Some(parent) = self.stack.last_mut() {
                if matches!(parent, StackFrame::Schema) {
                    self.schema.groups.push(grp);
                }
            } else {
                self.schema.groups.push(grp);
            }
        }
        Ok(())
    }

    pub(super) fn finish_simple_restriction(&mut self, r: XsdSimpleRestriction) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            if let StackFrame::SimpleType(st) = parent {
                st.content = XsdSimpleTypeContent::Restriction(r);
            }
        }
        Ok(())
    }

    pub(super) fn finish_simple_content_extension(
        &mut self,
        ext: XsdSimpleContentExtension,
    ) -> Result<()> {
        self.finish_simple_content(XsdSimpleContentDerivation::Extension(ext));
        Ok(())
    }

    pub(super) fn finish_simple_content_restriction(
        &mut self,
        r: XsdSimpleContentRestriction,
    ) -> Result<()> {
        self.finish_simple_content(XsdSimpleContentDerivation::Restriction(r));
        Ok(())
    }

    pub(super) fn finish_complex_content(&mut self, mixed: bool) -> Result<()> {
        // Set mixed on parent complexType if specified
        if mixed {
            for frame in self.stack.iter_mut().rev() {
                if let StackFrame::ComplexType(ct) = frame {
                    ct.mixed = true;
                    break;
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish_complex_content_extension(
        &mut self,
        ext: XsdComplexContentExtension,
    ) -> Result<()> {
        self.finish_complex_derivation(XsdComplexContentDerivation::Extension(ext));
        Ok(())
    }

    pub(super) fn finish_complex_content_restriction(
        &mut self,
        r: XsdComplexContentRestriction,
    ) -> Result<()> {
        self.finish_complex_derivation(XsdComplexContentDerivation::Restriction(r));
        Ok(())
    }

    pub(super) fn finish_simple_list(&mut self, list: XsdSimpleList) -> Result<()> {
        self.finish_simple_type_content(XsdSimpleTypeContent::List(list));
        Ok(())
    }

    pub(super) fn finish_simple_union(&mut self, union: XsdSimpleUnion) -> Result<()> {
        self.finish_simple_type_content(XsdSimpleTypeContent::Union(union));
        Ok(())
    }

    pub(super) fn finish_any(&mut self, any: XsdAny) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Sequence(seq) => {
                    seq.particles.push(XsdParticleItem::Any(any));
                }
                StackFrame::Choice(choice) => {
                    choice.particles.push(XsdParticleItem::Any(any));
                }
                StackFrame::ComplexType(ct) => {
                    ct.content = XsdComplexContent::Particle(XsdParticle::Any(any));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn finish_identity_constraint(
        &mut self,
        constraint: XsdIdentityConstraint,
    ) -> Result<()> {
        // Identity constraints are always children of element declarations
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::Element(elem) = frame {
                elem.identity_constraints.push(constraint);
                return Ok(());
            }
        }
        Ok(())
    }

    pub(super) fn finish_redefine(&mut self, redefine: XsdRedefine) -> Result<()> {
        // Redefine is a top-level schema component
        self.schema.redefines.push(redefine);
        Ok(())
    }
}
