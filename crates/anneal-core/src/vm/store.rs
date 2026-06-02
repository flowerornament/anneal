//! Tuple-backed relation storage for the physical runtime.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactIdentity, HandleFact, MetaFact,
    SnapshotFact, SpanFact,
};
use crate::ir::ids::{FieldId, RelationId, RowId};
use crate::ir::interner::Interner;
use crate::ir::schema::{RelationSchema, SchemaRegistry};
use crate::runtime::eval::{NumberValue, Value};
use crate::store::{FactStore, GenerationFact};

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
    interner: Interner,
    schemas: SchemaRegistry,
    lists: ListArena,
    relations: BTreeMap<RelationId, RelationStore>,
}

impl TupleDb {
    pub(crate) fn from_store_with_visibility(
        store: &FactStore,
        fact_visible: impl Fn(&FactIdentity) -> bool,
    ) -> Self {
        let mut db = Self::with_stored_builtins();
        let hidden_handles = hidden_handles(store, &fact_visible);
        db.insert_relation_rows(
            "handle",
            store
                .handles()
                .iter()
                .filter(|fact| fact_visible(&fact.identity))
                .map(handle_values),
        );
        db.insert_relation_rows(
            "edge",
            store
                .edges()
                .iter()
                .filter(|fact| {
                    fact_visible(&fact.identity)
                        && !hidden_handles.contains(&fact.from)
                        && !hidden_handles.contains(&fact.to)
                })
                .map(edge_values),
        );
        db.insert_relation_rows(
            "meta",
            store
                .meta()
                .iter()
                .filter(|fact| {
                    fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                })
                .map(meta_values),
        );
        db.insert_relation_rows(
            "content",
            store
                .content()
                .iter()
                .filter(|fact| {
                    fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                })
                .map(content_values),
        );
        db.insert_relation_rows(
            "span",
            store
                .spans()
                .iter()
                .filter(|fact| {
                    fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                })
                .map(span_values),
        );
        db.insert_relation_rows(
            "concern",
            store
                .concerns()
                .iter()
                .filter(|fact| {
                    fact_visible(&fact.identity) && !hidden_handles.contains(&fact.member)
                })
                .map(concern_values),
        );
        db.insert_relation_rows("config", store.configs().iter().map(config_values));
        db.insert_relation_rows(
            "snapshot",
            store
                .snapshots()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.id))
                .map(snapshot_values),
        );
        db.insert_relation_rows(
            "generation",
            store.generations().iter().map(generation_values),
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

    fn insert_relation_rows(&mut self, relation: &str, rows: impl IntoIterator<Item = Vec<Value>>) {
        let relation_name = self.interner.intern(relation);
        let schema = self
            .schemas
            .relation_by_name(relation_name)
            .expect("stored builtin schema exists")
            .clone();
        let mut store = RelationStore::new(&schema);
        for row in rows {
            debug_assert_eq!(row.len(), schema.arity());
            let tuple = row
                .iter()
                .map(|value| {
                    PhysicalValue::from_logical(value, &mut self.interner, &mut self.lists)
                })
                .collect::<Vec<_>>();
            store.push(Tuple::new(tuple));
        }
        self.insert_relation(store);
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

    pub(crate) fn projected_rows(&self, relation: &str) -> Vec<BTreeMap<String, Value>> {
        let Some(relation_name) = self.interner.lookup(relation) else {
            return Vec::new();
        };
        let Some(schema) = self.schemas.relation_by_name(relation_name) else {
            return Vec::new();
        };
        let Some(store) = self.relation(schema.id()) else {
            return Vec::new();
        };
        store
            .rows()
            .iter()
            .map(|tuple| self.project_tuple(schema, tuple))
            .collect()
    }

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

fn hidden_handles<F>(store: &FactStore, fact_visible: &F) -> BTreeSet<String>
where
    F: Fn(&FactIdentity) -> bool,
{
    store
        .handles()
        .iter()
        .filter(|fact| !fact_visible(&fact.identity))
        .map(|fact| fact.id.clone())
        .collect()
}

fn source_values(identity: &FactIdentity, values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut row = identity_values(identity);
    row.extend(values);
    row
}

fn identity_values(identity: &FactIdentity) -> Vec<Value> {
    vec![
        string_value(identity.corpus.to_string()),
        string_value(identity.source.to_string()),
        string_value(identity.native_id.to_string()),
        string_value(identity.origin_uri.to_string()),
        string_value(identity.revision.to_string()),
        generation_value(identity.generation),
    ]
}

fn handle_values(fact: &HandleFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [
            string_value(&fact.id),
            string_value(&fact.kind),
            opt_string(fact.status.as_ref()),
            string_value(&fact.namespace),
            string_value(&fact.file),
            int_value(i64::from(fact.line)),
            opt_string(fact.date.as_ref()),
            string_value(&fact.area),
            string_value(&fact.summary),
        ],
    )
}

fn edge_values(fact: &EdgeFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [
            string_value(&fact.from),
            string_value(&fact.to),
            string_value(&fact.kind),
            string_value(&fact.file),
            int_value(i64::from(fact.line)),
        ],
    )
}

fn meta_values(fact: &MetaFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [
            string_value(&fact.handle),
            string_value(&fact.key),
            string_value(&fact.value),
        ],
    )
}

fn content_values(fact: &ContentFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [
            string_value(&fact.handle),
            string_value(&fact.span_id),
            int_value(i64::from(fact.lines)),
            string_value(&fact.text),
            int_value(i64::from(fact.tokens)),
        ],
    )
}

fn span_values(fact: &SpanFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [
            string_value(&fact.id),
            string_value(&fact.handle),
            int_value(i64::from(fact.start_line)),
            int_value(i64::from(fact.end_line)),
            string_value(&fact.summary),
        ],
    )
}

fn concern_values(fact: &ConcernFact) -> Vec<Value> {
    source_values(
        &fact.identity,
        [string_value(&fact.name), string_value(&fact.member)],
    )
}

fn config_values(fact: &ConfigFact) -> Vec<Value> {
    vec![
        string_value(fact.corpus.to_string()),
        string_value(&fact.key),
        string_value(&fact.value),
        fact.ordinal
            .map_or(Value::Null, |ordinal| int_value(i64::from(ordinal))),
    ]
}

fn snapshot_values(fact: &SnapshotFact) -> Vec<Value> {
    vec![
        string_value(fact.corpus.to_string()),
        string_value(&fact.snapshot),
        string_value(&fact.at),
        string_value(&fact.id),
        string_value(&fact.key),
        string_value(&fact.value),
    ]
}

fn generation_values(fact: &GenerationFact) -> Vec<Value> {
    vec![
        string_value(fact.corpus.to_string()),
        string_value(fact.source.to_string()),
        generation_value(fact.current),
    ]
}

fn opt_string(value: Option<&String>) -> Value {
    value.cloned().map_or(Value::Null, Value::String)
}

fn string_value(value: impl ToString) -> Value {
    Value::String(value.to_string())
}

fn int_value(value: i64) -> Value {
    Value::Number(NumberValue::Int(value))
}

fn generation_value(generation: crate::ids::Generation) -> Value {
    int_value(i64::try_from(generation.get()).unwrap_or(i64::MAX))
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
