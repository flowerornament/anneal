---
status: review
updated: 2026-04-28
verifies: formal-model/v17.md
depends-on: formal-model/v17.md
references: implementation/2026-04-28-v17-import-plan.md
---

# REV-17 Formal Model v17 Conformance Audit

This v17 conformance audit checks whether the field evidence model can support
offline station reports, crate transfers, and reviewer signoff without relying
on network order.

## Summary

The audit finds that formal-model/v17.md is ready for the Harbor Ledger pilot.
The model handles ordinary station observations and the two-witness crate
transfer fallback. OQ-17 remains open because delayed station reports need a
clear tie-break rule.

## Method

The audit follows three synthetic routes through the model. Each route includes
a station observation, a crate transfer, and a review note. The audit then
checks whether IMP-17 can reproduce the expected evidence graph from the same
packet.

## Findings

REV-17 verifies FM-17 for the pilot scope. It does not close OQ-17.
