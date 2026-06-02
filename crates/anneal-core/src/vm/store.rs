//! Tuple-backed relation storage for the physical runtime.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::ir::ids::{FieldId, RelationId, RowId};
use crate::ir::schema::RelationSchema;

use super::value::PhysicalValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tuple(Box<[PhysicalValue]>);

impl Tuple {
    pub(crate) fn new(values: impl Into<Box<[PhysicalValue]>>) -> Self {
        Self(values.into())
    }

    pub(crate) fn values(&self) -> &[PhysicalValue] {
        &self.0
    }

    pub(crate) fn get(&self, field: FieldId) -> Option<PhysicalValue> {
        self.0.get(field.index()).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RelationIndex {
    values: BTreeMap<PhysicalValue, Vec<RowId>>,
}

impl RelationIndex {
    pub(crate) fn rows_for(&self, value: PhysicalValue) -> &[RowId] {
        self.values.get(&value).map_or(&[], Vec::as_slice)
    }

    fn insert(&mut self, value: PhysicalValue, row: RowId) {
        self.values.entry(value).or_default().push(row);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationStore {
    relation: RelationId,
    rows: Vec<Tuple>,
    indexes: BTreeMap<FieldId, RelationIndex>,
}

impl RelationStore {
    pub(crate) fn new(schema: &RelationSchema) -> Self {
        let indexes = schema
            .fields()
            .iter()
            .map(|field| (field.id(), RelationIndex::default()))
            .collect();
        Self {
            relation: schema.id(),
            rows: Vec::new(),
            indexes,
        }
    }

    pub(crate) fn relation(&self) -> RelationId {
        self.relation
    }

    pub(crate) fn push(&mut self, tuple: Tuple) -> RowId {
        let row = RowId::from_index(self.rows.len());
        for (field, index) in &mut self.indexes {
            if let Some(value) = tuple.get(*field) {
                index.insert(value, row);
            }
        }
        self.rows.push(tuple);
        row
    }

    pub(crate) fn row(&self, row: RowId) -> Option<&Tuple> {
        self.rows.get(row.index())
    }

    pub(crate) fn rows(&self) -> &[Tuple] {
        &self.rows
    }

    pub(crate) fn index(&self, field: FieldId) -> Option<&RelationIndex> {
        self.indexes.get(&field)
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TupleDb {
    relations: BTreeMap<RelationId, RelationStore>,
}

impl TupleDb {
    pub(crate) fn insert_relation(&mut self, relation: RelationStore) {
        self.relations.insert(relation.relation(), relation);
    }

    pub(crate) fn relation(&self, relation: RelationId) -> Option<&RelationStore> {
        self.relations.get(&relation)
    }

    pub(crate) fn relation_mut(&mut self, relation: RelationId) -> Option<&mut RelationStore> {
        self.relations.get_mut(&relation)
    }

    pub(crate) fn len(&self) -> usize {
        self.relations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ids::{FieldId, RelationId, SymbolId};
    use crate::ir::schema::{RelationSchema, ValueType};

    #[test]
    fn relation_store_preserves_insertion_order_and_indexes_values() {
        let schema = RelationSchema::new(
            RelationId::from_index(0),
            SymbolId::from_index(0),
            [
                (SymbolId::from_index(1), ValueType::Symbol),
                (SymbolId::from_index(2), ValueType::Number),
            ],
        );
        let mut store = RelationStore::new(&schema);
        let stable = PhysicalValue::Sym(SymbolId::from_index(10));
        let draft = PhysicalValue::Sym(SymbolId::from_index(11));

        let first = store.push(Tuple::new([stable, PhysicalValue::Null]));
        let second = store.push(Tuple::new([draft, PhysicalValue::Null]));
        let third = store.push(Tuple::new([stable, PhysicalValue::Null]));

        assert_eq!(first, RowId::from_index(0));
        assert_eq!(second, RowId::from_index(1));
        assert_eq!(third, RowId::from_index(2));
        assert_eq!(store.rows()[0].get(FieldId::from_index(0)), Some(stable));
        assert_eq!(
            store
                .index(FieldId::from_index(0))
                .expect("field is indexed")
                .rows_for(stable),
            &[RowId::from_index(0), RowId::from_index(2)]
        );
    }
}
