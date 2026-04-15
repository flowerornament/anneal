---
status: draft
date: 2026-04-15
depends-on: anneal-spec.md
---

# CLI UX Audit

Findings from a manual audit of anneal's full command and flag surface. Focus is on discoverability for newcomers (both human and agent), flag consistency, and naming clarity.

## Discoverability

### D-01: `status` doesn't tell you what to do next

`status` reports "2 errors, 0 warnings" but not which ones. You have to know to run `check`. When errors > 0, the human output should include a hint:

```
 health  2 errors, 0 warnings
         run `anneal check` for details
```

Same for suggestions — when the count is nonzero, hint at `anneal check --suggest`.

### D-02: `check` buries errors under suggestions

The default text output lists diagnostics in code order. In Murail, 13 S002 suggestions appear before the 2 E001 errors. Human output should sort by severity (errors first, then warnings, then info, then suggestions) regardless of code number.

JSON output already sorts by severity. The text renderer should match.

### D-03: `find` ranks sections above labels for prefix searches

Searching `anneal find OQ` returns `LABELS.md#open-questions-(oq-)` (a section heading that happens to contain "oq") before `OQ-1` (the actual label). For searches that match a known label prefix, labels should rank above sections.

Possible heuristic: if the query is all-uppercase and matches a confirmed or observed namespace prefix, sort label matches first.

### D-04: `query handles` default output lacks context

`anneal query handles` with no flags dumps `A-1` through `A-19` — alphabetical but useless. The human table shows kind, status, incoming, outgoing, and handle ID, but no file path. Adding a truncated file path column would make the output scannable.

### D-05: `map` default isn't actionable

The default `map` output is a summary: "579 labels, 11433 sections." This isn't useful for orientation or decision-making. The "Expand with:" hint is good but the default view itself is inert.

Consider changing the default to `map --by-area` (from the areas spec) once area support lands. The area-level topology is a better 30-second corpus overview than raw counts.

### D-06: `obligations` gives no hint when linear namespaces are unconfigured

`anneal obligations` with no linear namespaces prints "0 outstanding, 0 discharged, 0 mooted" — which looks like "everything is fine" when it really means "nothing is configured." Should print a hint:

```
No linear namespaces configured.
Set [handles] linear = ["OQ", "REQ"] in anneal.toml to enable obligation tracking.
```

## Flag Inconsistencies

### F-01: `--active-only` and `--include-terminal` are redundant inverses

`check` has both `--active-only` (the default behavior) and `--include-terminal` (the expansion). `--active-only` is a no-op — it just confirms what already happens. Having both suggests they're independent switches rather than a toggle.

Recommendation: keep `--include-terminal` as the expansion flag, remove `--active-only`. If backward compatibility is needed, keep `--active-only` as a hidden alias.

### F-02: `--scope=active|all` vs `--active-only`/`--include-terminal`

`query` subcommands use `--scope=active|all`. `check` uses `--active-only`/`--include-terminal`. Same concept, different surface.

Recommendation: adopt `--scope=active|all` on `check` too, deprecating the boolean flags. Or adopt `--include-terminal` on `query` too. Either way, converge.

### F-03: `query` subcommands have no help descriptions

`anneal query --help` lists subcommands with zero descriptions:

```
Commands:
  handles
  edges
  diagnostics
  obligations
  suggestions
```

Compare to the top-level help which has good one-line descriptions for every command. Each query subcommand should have a description line, e.g.:

```
Commands:
  handles       Filter and list handles by kind, status, namespace, or edge count
  edges         Filter edges by kind, endpoint properties, or structural patterns
  diagnostics   Query freshly-derived diagnostics with severity/code/file filters
  obligations   Query obligation state by namespace and disposition
  suggestions   Query structural suggestions by code
```

### F-04: `explain` subcommands have no help descriptions

Same issue as F-03. `anneal explain --help` lists subcommands with no descriptions, and the individual subcommand flags (`--id`, `--code`, `--handle`) have no help text.

### F-05: `--json` position sensitivity

`--json` and `--pretty` are defined on every subcommand individually. This means `anneal --json status` uses the top-level `--json` flag while `anneal status --json` uses the subcommand's. They should behave identically. Verify this is the case or consolidate to one definition point.

## Naming / Conceptual Clarity

### N-01: "frozen" vs "terminal"

`status` output says "12241 active, 253 frozen" but the help text, spec, and concepts section all say "terminal." The word "frozen" appears nowhere in the concept definitions. Should use "terminal" consistently in output, or define "frozen" as a synonym in the concepts section.

### N-02: `find` vs `query handles` distinction is invisible

`find` searches by text substring. `query handles` filters by structured fields. The `query` help says this explicitly, but from the command names alone, a newcomer can't tell which to use.

The `find` help could include a "See also" line:

```
Use `anneal query handles` for structured filtering by kind, status, namespace,
or edge count. Use `anneal find` for text search across handle identities.
```

## Missing Affordances

### M-01: No `--count` flag

Sometimes you just want "how many?" without the list. `anneal find OQ --count` → `181`. Useful for scripts and quick orientation. Could apply to `find` and `query` subcommands.

### M-02: No way to list namespaces

There's no direct way to see available namespaces. You have to `query handles --kind=label --full` and extract prefixes. A `--namespaces` flag on `find` or a `query namespaces` subcommand would help orientation. Alternatively, `anneal areas` (from the areas spec) may subsume this need if it shows namespaces per area.

### M-03: `impact` has no `--depth` flag

`impact` traverses the full transitive closure. For large corpora or hub handles with high fan-out, bounding to 1-2 hops would be useful:

```
anneal impact OQ-64 --depth=1    # direct dependents only
anneal impact OQ-64 --depth=2    # 2-hop neighborhood
anneal impact OQ-64              # full transitive closure (current behavior)
```

## Summary

| ID | Category | Severity | Description |
|----|----------|----------|-------------|
| D-01 | Discoverability | Low | `status` should hint at `check` when errors > 0 |
| D-02 | Discoverability | Medium | `check` text output should sort errors first |
| D-03 | Discoverability | Low | `find` should rank labels above sections for prefix matches |
| D-04 | Discoverability | Low | `query handles` human output should include file paths |
| D-05 | Discoverability | Low | `map` default could be more useful (revisit with areas) |
| D-06 | Discoverability | Low | `obligations` should hint when linear is unconfigured |
| F-01 | Flag consistency | Low | Remove redundant `--active-only` on `check` |
| F-02 | Flag consistency | Medium | Converge `--scope` and `--active-only`/`--include-terminal` |
| F-03 | Flag consistency | Medium | Add descriptions to `query` subcommands |
| F-04 | Flag consistency | Medium | Add descriptions to `explain` subcommands |
| F-05 | Flag consistency | Low | Verify `--json` position doesn't matter |
| N-01 | Naming | Medium | Use "terminal" consistently, not "frozen" |
| N-02 | Naming | Low | Cross-reference `find` and `query handles` in help |
| M-01 | Missing | Low | Add `--count` flag to `find` and `query` |
| M-02 | Missing | Low | Add namespace listing (or defer to `areas`) |
| M-03 | Missing | Low | Add `--depth` flag to `impact` |
