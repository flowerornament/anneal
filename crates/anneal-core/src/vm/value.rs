//! Runtime value types used by logical and physical evaluators.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use serde::Serialize;

use crate::ir::ids::{ListId, SymbolId};
use crate::ir::interner::Interner;
use crate::runtime::eval::Value;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NumberValue {
    Int(i64),
    Float(f64),
}

impl Eq for NumberValue {}

impl Ord for NumberValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.total_cmp(b),
            (Self::Int(_), Self::Float(_)) => Ordering::Less,
            (Self::Float(_), Self::Int(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NumberValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for NumberValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Int(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Self::Float(value) => {
                1_u8.hash(state);
                value.to_bits().hash(state);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PhysicalValue {
    Sym(SymbolId),
    Number(NumberValue),
    Bool(bool),
    Null,
    // Reserved for aggregate list slots once the Plan/IR middle-end owns list
    // lifetimes. Current logical lists still project at the Value boundary.
    #[allow(dead_code)]
    List(ListId),
}

impl PhysicalValue {
    pub(crate) fn from_logical(
        value: &Value,
        interner: &mut Interner,
        lists: &mut ListArena,
    ) -> Self {
        match value {
            Value::String(value) => Self::Sym(interner.intern(value)),
            Value::Number(value) => Self::Number(*value),
            Value::Bool(value) => Self::Bool(*value),
            Value::Null => Self::Null,
            Value::List(values) => {
                let values = values
                    .iter()
                    .map(|value| Self::from_logical(value, interner, lists))
                    .collect::<Vec<_>>();
                Self::List(lists.push(values))
            }
        }
    }

    pub(crate) fn to_logical(self, interner: &Interner, lists: &ListArena) -> Option<Value> {
        match self {
            Self::Sym(symbol) => interner
                .resolve(symbol)
                .map(|text| Value::String(text.to_owned())),
            Self::Number(value) => Some(Value::Number(value)),
            Self::Bool(value) => Some(Value::Bool(value)),
            Self::Null => Some(Value::Null),
            Self::List(list) => {
                let values = lists
                    .get(list)?
                    .iter()
                    .copied()
                    .map(|value| value.to_logical(interner, lists))
                    .collect::<Option<Vec<_>>>()?;
                Some(Value::List(values))
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ListArena {
    lists: Vec<Box<[PhysicalValue]>>,
}

impl ListArena {
    pub(crate) fn push(&mut self, values: Vec<PhysicalValue>) -> ListId {
        let id = ListId::from_index(self.lists.len());
        self.lists.push(values.into_boxed_slice());
        id
    }

    pub(crate) fn get(&self, id: ListId) -> Option<&[PhysicalValue]> {
        self.lists.get(id.index()).map(AsRef::as_ref)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lists.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_values_are_small_copy_types() {
        assert!(std::mem::size_of::<NumberValue>() <= 16);
        assert!(std::mem::size_of::<PhysicalValue>() <= 16);
        fn assert_copy<T: Copy>() {}
        assert_copy::<NumberValue>();
        assert_copy::<PhysicalValue>();
    }

    #[test]
    fn physical_value_round_trips_nested_lists() {
        let logical = Value::List(vec![
            Value::String("stable".to_string()),
            Value::Number(NumberValue::Int(42)),
            Value::List(vec![Value::Bool(true), Value::Null]),
        ]);
        let mut interner = Interner::default();
        let mut lists = ListArena::default();

        let physical = PhysicalValue::from_logical(&logical, &mut interner, &mut lists);

        assert_eq!(lists.len(), 2);
        assert_eq!(physical.to_logical(&interner, &lists), Some(logical));
    }
}
