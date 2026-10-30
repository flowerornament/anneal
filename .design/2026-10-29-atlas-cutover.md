# Shared Atlas cutover — 2026-10-29

Technical plan for the implementer and Anneal's reviewer. Authority: the user's
2026-10-29 go on Girard `girard-xqe.4`; coordinator boundary and toolchain rulings
`1030T054723-e4e4` and `1030T055250-bc6e`. The shared source/check specification
is Atlas `docs/design/consolidation.md`. Toolchain work is `anneal-tnzf`.

## Model / Replaces / Adds

Atlas core owns captured Rust source, lexical grammar and test evidence, native
navigation and source-review computations. Anneal's xtask owns its command syntax,
legacy JSON/text formatting and comment-policy values. Product crates do not
use Atlas; xtask remains independent of anneal-core.

Replace `xtask/src/scan.rs` and the generic discovery, counts, relationship lookup,
revision and shape calculations in `xtask/src/atlas.rs`. Keep their existing test
claims at the new public boundary. Add a pinned, publicly fetchable core dependency
and only the thin native presentation/input wrapper. No old-reader fallback,
policy relaxation or new command schema is introduced.

## Fixed inputs and complete fan-out

Baseline source and executable: `2b8e3ba4b9bb362520139ecdaf4184aba13cedd4`.
The shared pin is the audited public Atlas source named in the cutover receipt;
publication/PII custody is `girard-xqe.6`. Candidate source, compiler, binary,
policy and command identities are recorded before comparison.

- `xtask/src/main.rs`: seven Atlas verbs and `nontest-loc` dispatch.
- `xtask/src/atlas.rs`: validator, native report schemas, ordering, text budgets,
  table layout and the four comment citation markers.
- `xtask/src/scan.rs`: every lexical/LOC law and every direct caller; remove the
  implementation after the callers and regression claims switch.
- `xtask/Cargo.toml`, workspace `Cargo.lock`, `deny.toml`: exact Git revision,
  explicit approved source, no sibling path or product dependency.
- `nix/package.nix`: fixed-output Git hash required by the workspace lockfile,
  including xtask despite runtime-only package selection.
- `scripts/check-git-calls.py` and its expectation: remove the old xtask builder
  from the census when deleted. Shared revision custody owns child Git/JJ
  isolation; no ancestor repository or inherited environment may select source.
- Existing just/cargo dispatch, Git-only/jj-only fixtures, documentation and
  release/installer/Home Manager consumers: preserve their actual boundaries.

## Two atomic changes

1. **Toolchain prerequisite.** `rust-toolchain.toml` is the only compiler-version
   source. Local Rust, CI, advisories, release and Nix use 1.99.1 from that file.
   Add a pinned Rust-toolchain input to Nix, leaving the existing nixpkgs, Git and
   jj 0.39 pin unchanged. Correct new strict Clippy findings without enlarging
   the allow-list. Freeze/review/land this independently; it adds no Atlas code.
2. **Consumer replacement.** Pin the public core, switch all listed consumers,
   delete replaced mechanisms and update dependency/Git/Nix censuses together.
   The native command interface and source policy remain the same. Freeze with
   the full behavior ledger; owner GO precedes ordinary `just land`.

This is the whole registered decomposition, not a chain of temporary bridges.

## Behavior ledger and oracles

Each ledger row stores a stable case ID, question, exact input/candidate pins,
argv/direct caller, expected unit/record count, stdout/stderr hashes, process
exit and disposition. JSON comparison preserves every field and array order.
Only declared incidental path/identity/duration fields may be normalized; counts,
findings, ambiguity and unavailable evidence are never normalized into agreement.
Text formatting has separate byte/transcript comparisons.

The case inventory covers:

- **Invocation:** all seven verbs; public/default populations; census workspace,
  crate, directory and module rungs; module item/graph flags; audit requirement;
  missing/excess positional arguments; unknown verbs/flags; forbidden flag mixes;
  default xtask help; valid/missing/excess `nontest-loc` paths.
- **Membership:** every native source site, contextual file mount, directly gated
  helper, test-only sibling, shared production/test mount, external target,
  parent path, symlink exclusion and unreadable required source. Resolve the
  earlier 54 gated-helper and 18 external-target discrepancy classes explicitly.
- **Names and edges:** grouped aliases, root facades, impl locations, same-name
  local/global ambiguity, external shadows, macro/type-name collisions and
  references in strings/comments. Preserve source witnesses and unknowns.
- **Policy:** documentation/header/citation counts with Anneal's existing marker
  values; concentration block/item/LOC units and sorting; shape changes including
  deleted modules; native LOC boundary laws. No policy budget changes.
- **Custody:** immutable baseline reads; exact toplevel; child Git environment
  scrub; jj-only requested root or explicit refusal, never the home repository;
  unchanged source after reads; source-sensitive and missing-tool failure paths.

Each row ends **Match**, **Owner-accepted deliberate change**, or **Unresolved**.
An unresolved row blocks landing. Every difference has a source witness and an
owner disposition; totals alone cannot close a row. Frozen old tools are test
oracles only. A single deliberate changed observation must make the comparison
fail; restore it before freezing the candidate.

## Verification and acceptance

Preserve row-counted raw artifacts under ignored `target/atlas/`; link exact
receipts from the work beads. Keep the Git archive/digest of the old tool and its
executable identity independent of candidate builds.

For each frozen change run strict format/Clippy, workspace tests, `just check`,
`just audit`, `nix build .#default` under its exact lock, Home Manager module
smoke, and the suite under `nix shell --inputs-from path:DESK nixpkgs#jujutsu`.
Record every compiler and flake.lock input change. Test scripts requiring a real
Git toplevel run from an owned materialization of the frozen candidate, not from
an ancestor or another agent's desk. Ordinary jj gates run in the jj-only desk.

The freeze packet names commit, literal `jj diff -r COMMIT --git | shasum -a 256`
and its output, source comparison, behavior ledger, knockout and all gate
outcomes. The coordinator recomputes the digest and gives GO before `just land`.
No raw push or release tag. The coordinator owns retirement of `anneal-fowh`.

## Risks and decision authority

Shared source is deliberately conservative about unresolved bindings and has
measured time/memory costs. These are explicit consumer decisions, never reasons
to silently suppress evidence. A public repository is required for anonymous and
Nix source builds; private historical receipts/PII must not be published.
Repository layout and privacy publication are decided on `girard-xqe.6` before
adding the dependency. No changed runtime, storage format or corpus semantics is
part of this cutover.
