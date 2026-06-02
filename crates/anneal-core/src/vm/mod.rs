//! Physical runtime storage for the compiler-arc implementation.
//!
//! The tuple store lands before eval routing; keep scaffold contracts quiet
//! until the `physical-substrate` execution path consumes them directly.
#![allow(dead_code)]

#[cfg(feature = "physical-substrate")]
pub(crate) mod store;
pub(crate) mod value;
