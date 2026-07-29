//! Static teaching policy, partitioned by runtime vocabulary family.

mod predicate;
mod primitive;
mod stored;
mod verb;

/// Predicate teaching policy consumed by the index builder.
pub(super) use predicate::{
    common_joins, diagnostic_code_extra_lines, predicate_example, predicate_extra_lines,
    predicate_relationship, predicate_requires, predicate_see_also,
};
/// Primitive teaching policy consumed by the index builder.
pub(super) use primitive::{
    primitive_determinism, primitive_doc, primitive_example, primitive_relationship,
    primitive_requires, primitive_see_also,
};
/// Stored-relation policy consumed by the index builder.
pub(super) use stored::{
    fallback_stored_relation_example, stored_relation_extra_lines, stored_relation_see_also,
    stored_signature,
};
/// Verb teaching policy consumed by the index builder.
pub(super) use verb::{verb_example, verb_relationship, verb_see_also};
