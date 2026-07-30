//! Runtime value types used by logical and physical evaluators.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use serde::Serialize;

use crate::ir::ids::{ListId, SymbolId};
use crate::ir::interner::Interner;
use crate::runtime::ast::ArithmeticOp;
use crate::runtime::eval::Value;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NumberValue {
    Int(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericBinaryError {
    DivisionByZero,
}

pub(crate) fn eval_numeric_binary(
    left: NumberValue,
    op: ArithmeticOp,
    right: NumberValue,
) -> Result<NumberValue, NumericBinaryError> {
    match (left, right) {
        (NumberValue::Int(left), NumberValue::Int(right)) => {
            eval_int_binary(left, op, right).map(NumberValue::Int)
        }
        (left, right) => eval_float_binary(number_as_float(left), op, number_as_float(right))
            .map(NumberValue::Float),
    }
}

fn eval_int_binary(left: i64, op: ArithmeticOp, right: i64) -> Result<i64, NumericBinaryError> {
    match op {
        ArithmeticOp::Add => Ok(left + right),
        ArithmeticOp::Sub => Ok(left - right),
        ArithmeticOp::Mul => Ok(left * right),
        ArithmeticOp::Div | ArithmeticOp::Rem if right == 0 => {
            Err(NumericBinaryError::DivisionByZero)
        }
        ArithmeticOp::Div => Ok(left / right),
        ArithmeticOp::Rem => Ok(left % right),
    }
}

fn eval_float_binary(left: f64, op: ArithmeticOp, right: f64) -> Result<f64, NumericBinaryError> {
    match op {
        ArithmeticOp::Add => Ok(left + right),
        ArithmeticOp::Sub => Ok(left - right),
        ArithmeticOp::Mul => Ok(left * right),
        ArithmeticOp::Div | ArithmeticOp::Rem if right == 0.0 => {
            Err(NumericBinaryError::DivisionByZero)
        }
        ArithmeticOp::Div => Ok(left / right),
        ArithmeticOp::Rem => Ok(left % right),
    }
}

#[allow(clippy::cast_precision_loss)]
fn number_as_float(value: NumberValue) -> f64 {
    match value {
        NumberValue::Int(value) => value as f64,
        NumberValue::Float(value) => value,
    }
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

    #[test]
    fn numeric_binary_preserves_integer_results_and_promotes_mixed_inputs() {
        assert_eq!(
            eval_numeric_binary(NumberValue::Int(7), ArithmeticOp::Div, NumberValue::Int(2)),
            Ok(NumberValue::Int(3))
        );
        assert_eq!(
            eval_numeric_binary(
                NumberValue::Int(7),
                ArithmeticOp::Add,
                NumberValue::Float(0.5)
            ),
            Ok(NumberValue::Float(7.5))
        );
        assert_eq!(
            eval_numeric_binary(
                NumberValue::Float(7.0),
                ArithmeticOp::Rem,
                NumberValue::Int(2)
            ),
            Ok(NumberValue::Float(1.0))
        );
    }

    #[test]
    fn numeric_binary_rejects_integer_and_float_zero_divisors() {
        for (left, right) in [
            (NumberValue::Int(1), NumberValue::Int(0)),
            (NumberValue::Float(1.0), NumberValue::Float(0.0)),
        ] {
            assert_eq!(
                eval_numeric_binary(left, ArithmeticOp::Div, right),
                Err(NumericBinaryError::DivisionByZero)
            );
            assert_eq!(
                eval_numeric_binary(left, ArithmeticOp::Rem, right),
                Err(NumericBinaryError::DivisionByZero)
            );
        }
    }
}
