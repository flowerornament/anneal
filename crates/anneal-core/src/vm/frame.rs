//! Slot-frame bindings for the planned executor.

use std::sync::Arc;

use crate::ir::ids::SlotId;
use crate::runtime::eval::DerivationNode;
use crate::vm::value::PhysicalValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedFrame {
    pub(crate) slots: Vec<Option<PhysicalValue>>,
    pub(crate) steps: Vec<Arc<DerivationNode>>,
}

impl PlannedFrame {
    pub(crate) fn empty(slot_count: usize) -> Self {
        Self {
            slots: vec![None; slot_count],
            steps: Vec::new(),
        }
    }

    pub(crate) fn with_values_only(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            steps: Vec::new(),
        }
    }

    pub(crate) fn push_step(
        mut self,
        trace: bool,
        step: impl FnOnce() -> Arc<DerivationNode>,
    ) -> Self {
        if trace {
            self.steps.push(step());
        }
        self
    }

    pub(crate) fn get(&self, slot: SlotId) -> Option<PhysicalValue> {
        self.slots.get(slot.index()).and_then(|value| *value)
    }

    pub(crate) fn set(&mut self, slot: SlotId, value: PhysicalValue) -> bool {
        let Some(current) = self.slots.get_mut(slot.index()) else {
            return false;
        };
        match current {
            Some(existing) => *existing == value,
            slot @ None => {
                *slot = Some(value);
                true
            }
        }
    }

    pub(crate) fn overwrite(&mut self, slot: SlotId, value: PhysicalValue) -> bool {
        let Some(current) = self.slots.get_mut(slot.index()) else {
            return false;
        };
        *current = Some(value);
        true
    }
}
