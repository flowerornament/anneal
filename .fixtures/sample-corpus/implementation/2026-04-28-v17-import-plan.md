---
status: plan
updated: 2026-04-28
implements: formal-model/v17.md
depends-on: formal-model/v17.md
references: operations/field-rollout-plan.md
---

# IMP-17 Import And Reconciliation Plan

IMP-17 converts handwritten station packets into ledger events. The importer
creates draft rows first, then a review pass attaches evidence and marks rows
verified.

## Work Items

- Parse station packet headers into route identifiers.
- Attach crate transfer receipts to their route event.
- Flag OQ-17 conflicts when observation time and upload time disagree.
