---
status: current
updated: 2026-05-30
depends-on: formal-model/v17.md
references: operations/field-rollout-plan.md
---

# Open Questions

## OQ-17 Delayed Station Reports

When a boat team submits a delayed water-station report, the ledger should
merge the report by observation time, not by upload time. This depends on
formal-model/v17.md and is tested in reviews/2026-04-28-harbor-ledger-conformance-audit.md.

## OQ-18 Crate Transfer Evidence

The team needs a compact rule for accepting a crate transfer when the sender
has a receipt but the receiver has not yet synchronized. IMP-17 proposes a
two-witness fallback.

## OQ-19 Temporary Packet Identifiers

Temporary paper packet identifiers should remain searchable during the pilot,
then retire once every route event has a stable ledger identifier. The field
rollout plan keeps OQ-19 visible for coordinator training.
