//! SP-NT1 — cyclic negation must be rejected at compile time.
//!
//! Spec §10 / LR-D5: "Mutual recursion through negation is rejected at
//! load time with the cycle named." For the `ascent` engine, the
//! rejection happens earlier — at compile time — which is strictly
//! stronger than the spec requires.
//!
//! This file intentionally fails to compile. The `trybuild` harness in
//! `tests/sp_nt1_compile_fail.rs` asserts the failure and snapshots the
//! exact error message in `sp_nt1_cyclic_negation.stderr`.

use ascent::ascent;
use spike_runner::types::{HandleId, Status};

ascent! {
    pub struct CyclicNegProgram;

    relation handle(HandleId, Status);

    relation active(HandleId);
    active(h) <-- handle(h, s), if s.is_active();

    // Mutual negation: blocked depends on !advancing; advancing on !blocked.
    relation blocked(HandleId);
    blocked(h) <-- active(h), !advancing(h);

    relation advancing(HandleId);
    advancing(h) <-- active(h), !blocked(h);
}

fn main() {}
