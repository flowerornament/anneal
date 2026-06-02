//! Internal logical identifiers and schema metadata for the physical runtime.
//!
//! This module is compiled behind `physical-substrate` before the evaluator is
//! routed through it, so some contracts are intentionally exercised only by
//! unit tests in the scaffold commit.
#![allow(dead_code)]

pub(crate) mod ids;
pub(crate) mod interner;
pub(crate) mod schema;
