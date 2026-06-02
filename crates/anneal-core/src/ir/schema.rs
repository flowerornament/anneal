//! Relation schema registry for tuple-backed stored relations.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::facts::STORED_RELATION_DESCRIPTORS;

use super::ids::{FieldId, RelationId, SymbolId};
use super::interner::Interner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueType {
    Symbol,
    Number,
    Bool,
    Null,
    List,
    Any,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldSchema {
    id: FieldId,
    name: SymbolId,
    value_type: ValueType,
}

impl FieldSchema {
    pub(crate) fn id(&self) -> FieldId {
        self.id
    }

    pub(crate) fn name(&self) -> SymbolId {
        self.name
    }

    pub(crate) fn value_type(&self) -> ValueType {
        self.value_type
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationSchema {
    id: RelationId,
    name: SymbolId,
    fields: Box<[FieldSchema]>,
    by_name: BTreeMap<SymbolId, FieldId>,
}

impl RelationSchema {
    pub(crate) fn new(
        id: RelationId,
        name: SymbolId,
        fields: impl IntoIterator<Item = (SymbolId, ValueType)>,
    ) -> Self {
        let fields = fields
            .into_iter()
            .enumerate()
            .map(|(index, (name, value_type))| FieldSchema {
                id: FieldId::from_index(index),
                name,
                value_type,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let by_name = fields.iter().map(|field| (field.name, field.id)).collect();
        Self {
            id,
            name,
            fields,
            by_name,
        }
    }

    pub(crate) fn id(&self) -> RelationId {
        self.id
    }

    pub(crate) fn name(&self) -> SymbolId {
        self.name
    }

    pub(crate) fn arity(&self) -> usize {
        self.fields.len()
    }

    pub(crate) fn fields(&self) -> &[FieldSchema] {
        &self.fields
    }

    pub(crate) fn field(&self, name: SymbolId) -> Option<FieldId> {
        self.by_name.get(&name).copied()
    }

    pub(crate) fn field_by_id(&self, id: FieldId) -> Option<&FieldSchema> {
        self.fields.get(id.index())
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SchemaRegistry {
    relations: Vec<RelationSchema>,
    by_name: BTreeMap<SymbolId, RelationId>,
}

impl SchemaRegistry {
    pub(crate) fn register(
        &mut self,
        interner: &mut Interner,
        name: &str,
        fields: impl IntoIterator<Item = (&'static str, ValueType)>,
    ) -> RelationId {
        let relation_name = interner.intern(name);
        if let Some(relation) = self.by_name.get(&relation_name) {
            return *relation;
        }
        let relation = RelationId::from_index(self.relations.len());
        let fields = fields
            .into_iter()
            .map(|(field, value_type)| (interner.intern(field), value_type));
        let schema = RelationSchema::new(relation, relation_name, fields);
        self.relations.push(schema);
        self.by_name.insert(relation_name, relation);
        relation
    }

    pub(crate) fn register_stored_builtins(&mut self, interner: &mut Interner) {
        for descriptor in STORED_RELATION_DESCRIPTORS {
            self.register(
                interner,
                descriptor.name,
                descriptor
                    .fields
                    .iter()
                    .copied()
                    .map(|field| (field, stored_field_type(field))),
            );
        }
    }

    pub(crate) fn relation(&self, id: RelationId) -> Option<&RelationSchema> {
        self.relations.get(id.index())
    }

    pub(crate) fn relation_by_name(&self, name: SymbolId) -> Option<&RelationSchema> {
        self.by_name
            .get(&name)
            .and_then(|relation| self.relation(*relation))
    }

    pub(crate) fn len(&self) -> usize {
        self.relations.len()
    }
}

fn stored_field_type(field: &str) -> ValueType {
    match field {
        "line" | "lines" | "tokens" | "start_line" | "end_line" | "ordinal" | "generation"
        | "current" => ValueType::Number,
        _ => ValueType::Symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_maps_builtin_fields_to_stable_columns() {
        let mut interner = Interner::default();
        let mut registry = SchemaRegistry::default();

        registry.register_stored_builtins(&mut interner);

        let handle_name = interner.intern("handle");
        let id_name = interner.intern("id");
        let handle = registry
            .relation_by_name(handle_name)
            .expect("handle schema registered");
        assert_eq!(handle.id(), RelationId::from_index(0));
        assert_eq!(handle.arity(), 15);
        assert_eq!(handle.field(id_name), Some(FieldId::from_index(6)));
        assert_eq!(
            handle
                .field_by_id(FieldId::from_index(11))
                .map(FieldSchema::value_type),
            Some(ValueType::Number)
        );
    }
}
