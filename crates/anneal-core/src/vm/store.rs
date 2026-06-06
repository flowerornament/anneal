//! Tuple-backed relation storage for the physical runtime.

use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactIdentity, HandleFact, MetaFact,
    SnapshotFact, SpanFact,
};
use crate::ir::ids::{FieldId, RelationId, RowId};
use crate::ir::interner::Interner;
use crate::ir::schema::{RelationSchema, SchemaRegistry};
use crate::runtime::eval::{NumberValue, Value};
use crate::store::{FactStore, GenerationFact};
use crate::visibility::hidden_handles;

use super::value::{ListArena, PhysicalValue};

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

    pub(crate) fn candidate_rows(
        &self,
        constraints: &[(FieldId, PhysicalValue)],
    ) -> RowCandidates<'_> {
        let mut best = None;
        for (field, value) in constraints {
            let Some(index) = self.index(*field) else {
                return RowCandidates::Empty;
            };
            let rows = index.rows_for(*value);
            if rows.is_empty() {
                return RowCandidates::Empty;
            }
            if best.is_none_or(|current: &[RowId]| rows.len() < current.len()) {
                best = Some(rows);
            }
        }
        best.map_or_else(
            || RowCandidates::All(0..self.rows.len()),
            |rows| RowCandidates::Indexed(rows.iter()),
        )
    }
}

pub(crate) enum RowCandidates<'a> {
    All(std::ops::Range<usize>),
    Indexed(std::slice::Iter<'a, RowId>),
    Empty,
}

impl Iterator for RowCandidates<'_> {
    type Item = RowId;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(rows) => rows.next().map(RowId::from_index),
            Self::Indexed(rows) => rows.next().copied(),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TupleDb {
    interner: Interner,
    schemas: SchemaRegistry,
    lists: ListArena,
    relations: BTreeMap<RelationId, RelationStore>,
}

impl Default for TupleDb {
    fn default() -> Self {
        Self::with_stored_builtins()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TupleRow<'a> {
    schema: &'a RelationSchema,
    tuple: &'a Tuple,
    interner: &'a Interner,
    lists: &'a ListArena,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogicalRowInsert {
    Inserted(RowId),
    UnknownRelation,
    UnknownField,
}

impl<'a> TupleRow<'a> {
    pub(crate) fn string(&self, field: &str) -> Option<&'a str> {
        match self.physical(field)? {
            PhysicalValue::Sym(symbol) => self.interner.resolve(symbol),
            PhysicalValue::Number(_)
            | PhysicalValue::Bool(_)
            | PhysicalValue::Null
            | PhysicalValue::List(_) => None,
        }
    }

    pub(crate) fn i64(&self, field: &str) -> Option<i64> {
        match self.physical(field)? {
            PhysicalValue::Number(NumberValue::Int(value)) => Some(value),
            PhysicalValue::Number(NumberValue::Float(_))
            | PhysicalValue::Sym(_)
            | PhysicalValue::Bool(_)
            | PhysicalValue::Null
            | PhysicalValue::List(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn logical(&self, field: &str) -> Option<Value> {
        self.physical(field)?.to_logical(self.interner, self.lists)
    }

    pub(crate) fn physical(&self, field: &str) -> Option<PhysicalValue> {
        let field_name = self.interner.lookup(field)?;
        let field = self.schema.field(field_name)?;
        self.tuple.get(field)
    }

    pub(crate) fn for_each_logical_field_filtered(
        &self,
        mut include: impl FnMut(&str) -> bool,
        mut visit: impl FnMut(&str, Value),
    ) {
        for field in self.schema.fields() {
            let Some(name) = self.interner.resolve(field.name()) else {
                continue;
            };
            if !include(name) {
                continue;
            }
            let Some(value) = self
                .tuple
                .get(field.id())
                .and_then(|value| value.to_logical(self.interner, self.lists))
            else {
                continue;
            };
            visit(name, value);
        }
    }
}

impl TupleDb {
    pub(crate) fn from_store_with_visibility(
        store: &FactStore,
        fact_visible: impl Fn(&FactIdentity) -> bool,
    ) -> Self {
        let mut db = Self::with_stored_builtins();
        let hidden_handles = hidden_handles(store, &fact_visible);
        db.insert_relation_rows_from(
            "handle",
            store
                .handles()
                .iter()
                .filter(|fact| fact_visible(&fact.identity)),
            Self::handle_values,
        );
        db.insert_relation_rows_from(
            "edge",
            store.edges().iter().filter(|fact| {
                fact_visible(&fact.identity)
                    && !hidden_handles.contains(&fact.from)
                    && !hidden_handles.contains(&fact.to)
            }),
            Self::edge_values,
        );
        db.insert_relation_rows_from(
            "meta",
            store.meta().iter().filter(|fact| {
                fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
            }),
            Self::meta_values,
        );
        db.insert_relation_rows_from(
            "content",
            store.content().iter().filter(|fact| {
                fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
            }),
            Self::content_values,
        );
        db.insert_relation_rows_from(
            "span",
            store.spans().iter().filter(|fact| {
                fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
            }),
            Self::span_values,
        );
        db.insert_relation_rows_from(
            "concern",
            store.concerns().iter().filter(|fact| {
                fact_visible(&fact.identity) && !hidden_handles.contains(&fact.member)
            }),
            Self::concern_values,
        );
        db.insert_relation_rows_from("config", store.configs().iter(), Self::config_values);
        db.insert_relation_rows_from(
            "snapshot",
            store
                .snapshots()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.id)),
            Self::snapshot_values,
        );
        db.insert_relation_rows_from(
            "generation",
            store.generations().iter(),
            Self::generation_values,
        );
        db
    }

    fn with_stored_builtins() -> Self {
        let mut interner = Interner::default();
        let mut schemas = SchemaRegistry::default();
        schemas.register_stored_builtins(&mut interner);
        Self {
            interner,
            schemas,
            lists: ListArena::default(),
            relations: BTreeMap::new(),
        }
    }

    pub(crate) fn insert_relation(&mut self, relation: RelationStore) {
        self.relations.insert(relation.relation(), relation);
    }

    pub(crate) fn insert_logical_row<'a>(
        &mut self,
        relation: &str,
        fields: impl IntoIterator<Item = (&'a str, &'a Value)>,
    ) -> LogicalRowInsert {
        let Some(relation_name) = self.interner.lookup(relation) else {
            return LogicalRowInsert::UnknownRelation;
        };
        let Some(schema) = self.schemas.relation_by_name(relation_name).cloned() else {
            return LogicalRowInsert::UnknownRelation;
        };
        let mut values = vec![PhysicalValue::Null; schema.arity()];
        for (field, value) in fields {
            let Some(field_name) = self.interner.lookup(field) else {
                return LogicalRowInsert::UnknownField;
            };
            let Some(field) = schema.field(field_name) else {
                return LogicalRowInsert::UnknownField;
            };
            values[field.index()] =
                PhysicalValue::from_logical(value, &mut self.interner, &mut self.lists);
        }
        let store = self
            .relations
            .entry(schema.id())
            .or_insert_with(|| RelationStore::new(&schema));
        LogicalRowInsert::Inserted(store.push(Tuple::new(values)))
    }

    fn insert_relation_rows_from<'a, T: 'a>(
        &mut self,
        relation: &str,
        rows: impl IntoIterator<Item = &'a T>,
        mut values: impl FnMut(&mut Self, &'a T) -> Vec<PhysicalValue>,
    ) {
        let relation_name = self.interner.intern(relation);
        let schema = self
            .schemas
            .relation_by_name(relation_name)
            .expect("stored builtin schema exists")
            .clone();
        let mut store = RelationStore::new(&schema);
        for item in rows {
            let row = values(self, item);
            debug_assert_eq!(row.len(), schema.arity());
            store.push(Tuple::new(row));
        }
        self.insert_relation(store);
    }

    fn source_values(
        &mut self,
        identity: &FactIdentity,
        values: impl IntoIterator<Item = PhysicalValue>,
    ) -> Vec<PhysicalValue> {
        let mut row = self.identity_values(identity);
        row.extend(values);
        row
    }

    fn identity_values(&mut self, identity: &FactIdentity) -> Vec<PhysicalValue> {
        vec![
            self.string_value(identity.corpus.as_str()),
            self.string_value(identity.source.as_str()),
            self.string_value(identity.native_id.as_str()),
            self.string_value(identity.origin_uri.as_str()),
            self.string_value(identity.revision.as_str()),
            Self::generation_value(identity.generation),
        ]
    }

    fn handle_values(&mut self, fact: &HandleFact) -> Vec<PhysicalValue> {
        let id = self.string_value(&fact.id);
        let kind = self.string_value(&fact.kind);
        let status = self.opt_string(fact.status.as_ref());
        let namespace = self.string_value(&fact.namespace);
        let file = self.string_value(&fact.file);
        let line = physical_int_value(i64::from(fact.line));
        let date = self.opt_string(fact.date.as_ref());
        let area = self.string_value(&fact.area);
        let summary = self.string_value(&fact.summary);
        self.source_values(
            &fact.identity,
            [id, kind, status, namespace, file, line, date, area, summary],
        )
    }

    fn edge_values(&mut self, fact: &EdgeFact) -> Vec<PhysicalValue> {
        let from = self.string_value(&fact.from);
        let to = self.string_value(&fact.to);
        let kind = self.string_value(&fact.kind);
        let file = self.string_value(&fact.file);
        let line = physical_int_value(i64::from(fact.line));
        self.source_values(&fact.identity, [from, to, kind, file, line])
    }

    fn meta_values(&mut self, fact: &MetaFact) -> Vec<PhysicalValue> {
        let handle = self.string_value(&fact.handle);
        let key = self.string_value(&fact.key);
        let value = self.string_value(&fact.value);
        self.source_values(&fact.identity, [handle, key, value])
    }

    fn content_values(&mut self, fact: &ContentFact) -> Vec<PhysicalValue> {
        let handle = self.string_value(&fact.handle);
        let span_id = self.string_value(&fact.span_id);
        let lines = physical_int_value(i64::from(fact.lines));
        let text = self.string_value(&fact.text);
        let tokens = physical_int_value(i64::from(fact.tokens));
        self.source_values(&fact.identity, [handle, span_id, lines, text, tokens])
    }

    fn span_values(&mut self, fact: &SpanFact) -> Vec<PhysicalValue> {
        let id = self.string_value(&fact.id);
        let handle = self.string_value(&fact.handle);
        let start_line = physical_int_value(i64::from(fact.start_line));
        let end_line = physical_int_value(i64::from(fact.end_line));
        let summary = self.string_value(&fact.summary);
        self.source_values(&fact.identity, [id, handle, start_line, end_line, summary])
    }

    fn concern_values(&mut self, fact: &ConcernFact) -> Vec<PhysicalValue> {
        let name = self.string_value(&fact.name);
        let member = self.string_value(&fact.member);
        self.source_values(&fact.identity, [name, member])
    }

    fn config_values(&mut self, fact: &ConfigFact) -> Vec<PhysicalValue> {
        vec![
            self.string_value(fact.corpus.as_str()),
            self.string_value(&fact.key),
            self.string_value(&fact.value),
            fact.ordinal.map_or(PhysicalValue::Null, |ordinal| {
                physical_int_value(i64::from(ordinal))
            }),
        ]
    }

    fn snapshot_values(&mut self, fact: &SnapshotFact) -> Vec<PhysicalValue> {
        vec![
            self.string_value(fact.corpus.as_str()),
            self.string_value(&fact.snapshot),
            self.string_value(&fact.at),
            self.string_value(&fact.id),
            self.string_value(&fact.key),
            self.string_value(&fact.value),
        ]
    }

    fn generation_values(&mut self, fact: &GenerationFact) -> Vec<PhysicalValue> {
        vec![
            self.string_value(fact.corpus.as_str()),
            self.string_value(fact.source.as_str()),
            Self::generation_value(fact.current),
        ]
    }

    fn opt_string(&mut self, value: Option<&String>) -> PhysicalValue {
        value.map_or(PhysicalValue::Null, |value| self.string_value(value))
    }

    fn string_value(&mut self, value: &str) -> PhysicalValue {
        PhysicalValue::Sym(self.interner.intern(value))
    }

    fn generation_value(generation: crate::ids::Generation) -> PhysicalValue {
        physical_int_value(i64::try_from(generation.get()).unwrap_or(i64::MAX))
    }

    pub(crate) fn relation(&self, relation: RelationId) -> Option<&RelationStore> {
        self.relations.get(&relation)
    }

    pub(crate) fn cloned_interner(&self) -> Interner {
        self.interner.clone()
    }

    pub(crate) fn cloned_lists(&self) -> ListArena {
        self.lists.clone()
    }

    pub(crate) fn empty_relation_store(&self, relation: &str) -> Option<RelationStore> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        Some(RelationStore::new(schema))
    }

    #[cfg(test)]
    pub(crate) fn projected_rows(&self, relation: &str) -> Vec<BTreeMap<String, Value>> {
        let mut rows = Vec::new();
        self.for_each_projected_row(relation, |row| rows.push(row));
        rows
    }

    #[cfg(test)]
    pub(crate) fn for_each_projected_row(
        &self,
        relation: &str,
        mut visit: impl FnMut(BTreeMap<String, Value>),
    ) {
        self.for_each_tuple_row(relation, |row| {
            visit(self.project_tuple(row.schema, row.tuple));
        });
    }

    pub(crate) fn for_each_relation_row(&self, mut visit: impl FnMut(&str, TupleRow<'_>)) {
        for store in self.relations.values() {
            let Some(schema) = self.schemas.relation(store.relation()) else {
                continue;
            };
            let Some(relation_name) = self.interner.resolve(schema.name()) else {
                continue;
            };
            for tuple in store.rows() {
                visit(
                    relation_name,
                    TupleRow {
                        schema,
                        tuple,
                        interner: &self.interner,
                        lists: &self.lists,
                    },
                );
            }
        }
    }

    pub(crate) fn for_each_tuple_row<'a>(
        &'a self,
        relation: &str,
        mut visit: impl FnMut(TupleRow<'a>),
    ) {
        self.for_each_tuple_row_id(relation, |_row_id, row| visit(row));
    }

    pub(crate) fn for_each_tuple_row_id<'a>(
        &'a self,
        relation: &str,
        mut visit: impl FnMut(RowId, TupleRow<'a>),
    ) {
        let Some(relation_name) = self.interner.lookup(relation) else {
            return;
        };
        let Some(schema) = self.schemas.relation_by_name(relation_name) else {
            return;
        };
        let Some(store) = self.relation(schema.id()) else {
            return;
        };
        for (row_id, tuple) in store.rows().iter().enumerate() {
            visit(
                RowId::from_index(row_id),
                TupleRow {
                    schema,
                    tuple,
                    interner: &self.interner,
                    lists: &self.lists,
                },
            );
        }
    }

    pub(crate) fn tuple_row(&self, relation: &str, row: RowId) -> Option<TupleRow<'_>> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        let store = self.relation(schema.id())?;
        self.tuple_row_in_store(schema, store, row)
    }

    pub(crate) fn tuple_row_in_named_store<'a>(
        &'a self,
        relation: &str,
        store: &'a RelationStore,
        row: RowId,
    ) -> Option<TupleRow<'a>> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        self.tuple_row_in_store(schema, store, row)
    }

    fn tuple_row_in_store<'a>(
        &'a self,
        schema: &'a RelationSchema,
        store: &'a RelationStore,
        row: RowId,
    ) -> Option<TupleRow<'a>> {
        Some(TupleRow {
            schema,
            tuple: store.row(row)?,
            interner: &self.interner,
            lists: &self.lists,
        })
    }

    #[cfg(test)]
    pub(crate) fn candidate_rows(
        &self,
        relation: &str,
        constraints: &[(String, Value)],
    ) -> RowCandidates<'_> {
        let Some(relation_name) = self.interner.lookup(relation) else {
            return RowCandidates::Empty;
        };
        let Some(schema) = self.schemas.relation_by_name(relation_name) else {
            return RowCandidates::Empty;
        };
        let Some(store) = self.relation(schema.id()) else {
            return RowCandidates::Empty;
        };
        let Some(constraints) = self.physical_constraints(schema, constraints) else {
            return RowCandidates::Empty;
        };
        store.candidate_rows(&constraints)
    }

    #[cfg(test)]
    pub(crate) fn candidate_rows_in_store<'a>(
        &'a self,
        relation: &str,
        store: &'a RelationStore,
        constraints: &[(String, Value)],
    ) -> RowCandidates<'a> {
        let Some(relation_name) = self.interner.lookup(relation) else {
            return RowCandidates::Empty;
        };
        let Some(schema) = self.schemas.relation_by_name(relation_name) else {
            return RowCandidates::Empty;
        };
        let Some(constraints) = self.physical_constraints(schema, constraints) else {
            return RowCandidates::Empty;
        };
        store.candidate_rows(&constraints)
    }

    #[cfg(test)]
    pub(crate) fn logical_field_value(
        &self,
        relation: &str,
        row: RowId,
        field: &str,
    ) -> Option<Value> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        let field_name = self.interner.lookup(field)?;
        let field = schema.field(field_name)?;
        let store = self.relation(schema.id())?;
        store
            .row(row)?
            .get(field)?
            .to_logical(&self.interner, &self.lists)
    }

    pub(crate) fn clone_tuple(&self, relation: &str, row: RowId) -> Option<Tuple> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        let store = self.relation(schema.id())?;
        store.row(row).cloned()
    }

    pub(crate) fn clone_tuple_with_patches(
        &self,
        relation: &str,
        row: RowId,
        patches: &BTreeMap<&str, PhysicalValue>,
    ) -> Option<Tuple> {
        let relation_name = self.interner.lookup(relation)?;
        let schema = self.schemas.relation_by_name(relation_name)?;
        let mut values = self.relation(schema.id())?.row(row)?.values().to_vec();
        for (field, value) in patches {
            let field_name = self.interner.lookup(field)?;
            let field = schema.field(field_name)?;
            let slot = values.get_mut(field.index())?;
            *slot = *value;
        }
        Some(Tuple::new(values))
    }

    #[cfg(test)]
    pub(crate) fn relation_names(&self) -> BTreeSet<String> {
        self.relations
            .keys()
            .filter_map(|relation| self.schemas.relation(*relation))
            .filter_map(|schema| self.interner.resolve(schema.name()).map(str::to_owned))
            .collect()
    }

    #[cfg(test)]
    fn physical_constraints(
        &self,
        schema: &RelationSchema,
        constraints: &[(String, Value)],
    ) -> Option<Vec<(FieldId, PhysicalValue)>> {
        constraints
            .iter()
            .map(|(field, value)| {
                let field_name = self.interner.lookup(field)?;
                let field = schema.field(field_name)?;
                let value = self.physical_value(value)?;
                Some((field, value))
            })
            .collect()
    }

    #[cfg(test)]
    fn physical_value(&self, value: &Value) -> Option<PhysicalValue> {
        match value {
            Value::String(value) => self.interner.lookup(value).map(PhysicalValue::Sym),
            Value::Number(value) => Some(PhysicalValue::Number(*value)),
            Value::Bool(value) => Some(PhysicalValue::Bool(*value)),
            Value::Null => Some(PhysicalValue::Null),
            Value::List(_) => None,
        }
    }

    #[cfg(test)]
    fn project_tuple(&self, schema: &RelationSchema, tuple: &Tuple) -> BTreeMap<String, Value> {
        schema
            .fields()
            .iter()
            .zip(tuple.values())
            .filter_map(|(field, value)| {
                let name = self.interner.resolve(field.name())?.to_owned();
                let value = value.to_logical(&self.interner, &self.lists)?;
                Some((name, value))
            })
            .collect()
    }
}

fn physical_int_value(value: i64) -> PhysicalValue {
    PhysicalValue::Number(NumberValue::Int(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{EdgeFact, FactBatch, FactBatchMode, FactIdentity, HandleFact};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::ir::ids::{FieldId, RelationId, SymbolId};
    use crate::ir::schema::{RelationSchema, ValueType};
    use crate::visibility::FactVisibility;

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

    #[test]
    fn tuple_db_lowers_store_rows_in_canonical_order() {
        let mut store = FactStore::default();
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles.push(handle_fact("b.md", "draft"));
        batch.handles.push(handle_fact("a.md", "stable"));
        store.merge(batch).expect("batch merges");

        let db = TupleDb::from_store_with_visibility(&store, |_| true);
        let rows = db.projected_rows("handle");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("id"), Some(&Value::String("a.md".to_string())));
        assert_eq!(rows[1].get("id"), Some(&Value::String("b.md".to_string())));
        assert_eq!(
            rows[0].get("line"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn tuple_db_applies_visibility_before_lowering() {
        let mut store = FactStore::default();
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles.push(handle_fact("public.md", "draft"));
        let mut private = handle_fact("private.md", "draft");
        private.identity.native_id = NativeId::from("private.md");
        batch.handles.push(private);
        batch.edges.push(EdgeFact {
            identity: identity("edge"),
            from: "public.md".to_string(),
            to: "private.md".to_string(),
            kind: "DependsOn".to_string(),
            file: "public.md".to_string(),
            line: 1,
        });
        batch.set_visibility(NativeId::from("private.md"), FactVisibility::Private);
        store.merge(batch).expect("batch merges");

        let db = TupleDb::from_store_with_visibility(&store, |identity| {
            store.visibility_for(identity) == FactVisibility::Public
        });

        assert_eq!(db.projected_rows("handle").len(), 1);
        assert_eq!(db.projected_rows("edge").len(), 0);
    }

    fn handle_fact(id: &str, status: &str) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: "file".to_string(),
            status: Some(status.to_string()),
            namespace: String::new(),
            file: id.to_string(),
            line: 1,
            date: None,
            area: String::new(),
            summary: id.to_string(),
        }
    }

    fn identity(native_id: &str) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            NativeId::from(native_id),
            OriginUri::from(format!("fixture://{native_id}")),
            Revision::from("rev"),
            Generation::initial(),
        )
    }
}
