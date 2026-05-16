//! SP-NT1 trybuild harness — asserts the cyclic-negation file fails to
//! compile, and captures the exact ascent error message as a snapshot.
//!
//! Snapshot lives at `tests/compile_fail/sp_nt1_cyclic_negation.stderr`.
//! Run `TRYBUILD=overwrite cargo test --test sp_nt1_compile_fail` to
//! regenerate after intentional engine or message changes.

#[test]
fn sp_nt1_cyclic_negation_rejected_at_compile_time() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/sp_nt1_cyclic_negation.rs");
}
