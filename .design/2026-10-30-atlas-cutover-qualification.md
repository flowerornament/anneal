# Shared Atlas consumer qualification — 2026-10-30

Evidence for `girard-xqe.4`, against [the native cutover plan](2026-10-29-atlas-cutover.md). This record distinguishes shared test coverage, Anneal's legacy presentation and exact-output comparisons. Candidate and artifact identities are in the freeze packet under ignored `target/atlas/anneal-cutover/` in the Atlas repository; the original native oracle is retained only as a test artifact.

## Source and interpretation boundaries

The dependency is public `atlas-core` 0.1.1 at `513aebb3d521b9a0785813bb3a55213c44bb4077`. CLI-only safety changes through Atlas 0.6.2 did not change this core. Product crates do not depend on it. The replacement deletes `xtask/src/scan.rs` and delegates grammar, source facts, name/edge interpretation and shape measures to core; local code keeps invocation, selection, report schemas and presentation.

Metadata is narrowed to each current member's `src` target roots before capture. Historical capture retains current membership and relocates that metadata, excluding targets absent in the historical tree. Physical shape diff explicitly selects native source-tree paths. Shared capture can additionally retain declared module-mount evidence; it does not promote such outside-source-tree paths into native counts. Concentration outlines only selected package roots. Navigation is constructed only for commands that consume it.

Coordinator acceptance of deterministic capture diagnostics on stderr is `1030T224051-e05d`. Stdout and success/refusal behavior retain their native contracts; diagnostic lines are sorted/deduplicated, have a fixed `atlas source:` prefix and source witness, and do not disappear during legacy serialization. A symlink fixture compares the complete census with and without the excluded link and checks the exact diagnostic. Current and historical diagnostic populations belong in the freeze packet.

The other accepted differences remain the plan's union grammar, physical-production shape preference and twelve comment-only ordinary edges. None licenses other output changes.

## Recovered regression claims

All nineteen deleted scanner tests were read against shared assertions. Sixteen retain the same shared scanner tests: function qualifiers, impl targets/capture, visibility, reexports, multiline aliases, struct blocks, doc attachment, multiple test regions, strings, characters, cfg predicates, test modules, explicit test paths, comma splitting and individually gated helper accounting. Tests are in core `src/scan.rs`; public boundary cases also exercise `tests/source_contract.rs`.

The remaining three claims are covered as follows:

- Live-code Unicode: restored through `unicode_in_live_code_preserves_projection_bytes`, including `π`, exact projected lines and capture after the Unicode bytes. Unicode in comments alone was insufficient.
- File-module production/test triples: shared source-context fixtures plus native `named_test_paths_and_production_mounts_select_the_native_population` exercise test-only, shared and production explicit mounts.
- Explicit production path: the same native fixture checks its ungated production-only case.

The sixteen deleted Atlas tests were also read against their assertions:

- Census workspace/crate populations and module ladder: shared navigation assertions and native `census_ladder_and_visibility_preserve_native_units`; native text and full real-tree fields are additionally compared in the transcript ledger.
- Dump authorities and root facade classification: shared navigation dump/facade assertions, native shared-facts test, and native legacy `vis`, `pub`, ID/private-item assertions. Full legacy shape is covered by the real-tree dump ledger, rather than assuming typed shared fields prove wrapper serialization.
- Name/module cards, documentation gaps and graph classes: shared name/module tests, native shared-facts fixture, missing-header/doc markers and exact audit totals; transcript ledger covers native table rendering. Orphan names are explicitly sorted alphabetically and tested with reverse source order.
- Same-crate preference and cross-crate ambiguity: native three-package fixture checks unique local coordinates and ambiguous cross-package edges without arbitrary selection.
- External shadows: native fixture checks both suppression of the external spelling and survival of an unrelated local edge.
- External grouped imports: shared captured-import grammar and external-shadow interpretation are separate claims; native public-boundary fixtures also check grouped/self/alias/glob bindings explicitly, rather than relying on a similar test name.
- Fake imports in comments/raw strings: native fixture checks that real return edges survive. The separate comment-induced-edge fixture checks false positive edges, not this false suppression case.
- Arrows inside generic bounds: native fixture checks `Fn(Tile) -> Cost` versus the function's actual `-> Grade`, plus a no-return Frame parameter.
- Identifier boundaries: native approximate-reference fixture compares `Tile` with `LaneTile`, `TileSet`, `Tile_x` and punctuation/qualified uses in live code.
- Citation markers: native fixture independently checks literal `CR-`, `.design`, `corpus-runtime` and `§` markers through the configured audit.
- Nested module spellings: shared module mapping and native qualified-name behavior are source-inspected; native fixtures directly pin the public path helper for nested files, mod.rs and crate roots; native name-card selectors retain their physical path-fragment meaning, not Rust module resolution.

This mapping records why a claim survives; it is not a claim that test names establish equivalence. The controlled fixtures, transcript ledger and independent review complement each other.

## Verification performed during resumption

The focused wrapper suite passed twenty-one tests. Strict xtask Clippy passed. The native Git-boundary suite passed twelve tests with its census corrected from four builders to three after deletion of the xtask Git launcher. Newly unblessed Rust, Python and shell launches still turn that gate red.

The complete native `just check` passed after the resumed code/test changes; `just audit` passed including advisories, bans, licenses and sources. These are local untimed qualification runs, not benchmark figures. Frozen publication checks and dependency/Nix custody are separately identified in the final receipt; this record does not substitute for coordinator GO or `just land`.
