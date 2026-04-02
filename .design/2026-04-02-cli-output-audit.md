---
status: draft
updated: 2026-04-02
description: >
  Audit of anneal CLI output behavior across representative corpora, focused on
  identifying bounded defaults and risky output shapes for agent-oriented use.
---

# anneal CLI Output Audit

Date: 2026-04-02

## Purpose

This audit evaluates how easily `anneal` can emit large amounts of text or JSON that are harmless for humans in a terminal, but costly for AI agents with limited context windows.

The goal is not to criticize the current CLI. The goal is to identify output shapes that are safe for interactive use, risky for agent use, and good candidates for agent-aware improvements.

## Scope

Commands covered:

- `anneal --help`
- `anneal status`
- `anneal check`
- `anneal get`
- `anneal find`
- `anneal init`
- `anneal impact`
- `anneal map`
- `anneal diff`
- `anneal obligations`

Representative corpora:

- Self corpus: `/Users/morgan/code/anneal/.design`
- Herald corpus: `/Users/morgan/code/herald/.design`
- Murail corpus: `/Users/morgan/code/murail/.design`

Metrics captured:

- exit code
- line count
- byte count

## Method

The audit used direct command execution with output redirected to a temporary file, then measured with `wc -l` and `wc -c`.

This is not a perfect proxy for token usage, but it is directionally strong:

- large byte counts reliably correlate with large context cost
- large line counts make pretty-printed JSON especially dangerous
- low line counts can still hide large payloads when JSON embeds dense strings

## Executive Summary

`anneal` currently has three output risk classes:

| Class | Commands | Agent risk |
| --- | --- | --- |
| Compact by default | `status`, `diff`, `obligations`, plain `impact`, narrow `get`, `init --dry-run` | Low |
| Safe only when narrowed | `find`, `get --json`, `map --around`, `status --verbose`, `check --include-terminal` | Medium |
| Structurally unsafe by default | `check --json`, broad `find`, full `map`, `map --json` | High |

The most important findings:

1. `check --json` is the biggest structural hazard.
2. `map --json` is deceptively dangerous because it stays short in lines while still emitting hundreds of KB to MB.
3. `find` becomes unbounded very quickly for broad queries, especially `find ""`.
4. `get --json` can balloon on hub handles because human output caps edges but JSON does not.
5. `status`, `diff`, and `obligations` are good foundations for agent-safe defaults.

## Why Some Outputs Blow Up

The largest outputs are not accidental. They follow from a few implementation choices:

- `print_json()` pretty-prints all JSON output via `serde_json::to_string_pretty(...)` in `src/cli.rs:94`.
- `CheckOutput` always includes `extractions`, and JSON mode clones `result.extractions` before formatting in `src/cli.rs:106` and `src/main.rs:684`.
- `MapOutput` serializes a full rendered graph as a `content: String` field in `src/cli.rs:1233`.
- Human `get` output caps edge display with `EDGE_DISPLAY_LIMIT`, but JSON `get` returns the full edge lists in `src/cli.rs:262`.
- `find` performs unrestricted substring matching across all handles and returns all matches in `src/cli.rs:486`.

## Command-by-Command Audit

### `anneal --help`

Representative measurement:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal --help` | self | `0` | `109` | `4,818` |

Assessment:

- Safe for humans.
- Slightly verbose for agents, but not a major problem.
- The bigger issue is that the help text currently presents all commands as equally suitable entry points, even though their context risk differs sharply.

Suggestions:

- Consider annotating high-risk commands in help text with cues like "full graph dump" or "machine-readable full diagnostics".
- Consider highlighting a compact orientation path directly in `--help`, such as `status --json`, plain `check`, and narrowed `find`.

### `anneal status`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal status` | self | `0` | `6` | `163` |
| `anneal status --json` | self | `0` | `52` | `899` |
| `anneal status --verbose` | self | `0` | `6` | `163` |
| `anneal --root /Users/morgan/code/herald/.design status` | Herald | `0` | `11` | `368` |
| `anneal --root /Users/morgan/code/herald/.design status --json` | Herald | `0` | `93` | `1,525` |
| `anneal --root /Users/morgan/code/herald/.design status --verbose` | Herald | `0` | `60` | `3,170` |
| `anneal --root /Users/morgan/code/murail/.design status` | Murail | `0` | `11` | `374` |
| `anneal --root /Users/morgan/code/murail/.design status --json` | Murail | `0` | `104` | `1,762` |
| `anneal --root /Users/morgan/code/murail/.design status --verbose` | Murail | `0` | `21` | `842` |

Assessment:

- Safest orientation command in the CLI.
- `status --json` is compact enough to be a default machine-readable entry point.
- `status --verbose` scales with corpus shape, but stayed moderate in all tested corpora.

Suggestions:

- Keep `status --json` as the preferred machine-readable orientation command.
- Consider a `--summary` alias or explicit "agent-safe" wording in help/docs, since it is already the best compact dashboard.

### `anneal check`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal check` | self | `1` | `6` | `236` |
| `anneal check --active-only` | self | `1` | `6` | `236` |
| `anneal check --json` | self | `1` | `1,147` | `26,627` |
| `anneal check --active-only --json` | self | `1` | `1,147` | `26,627` |
| `anneal --root /Users/morgan/code/herald/.design check` | Herald | `1` | `74` | `7,507` |
| `anneal --root /Users/morgan/code/herald/.design check --json` | Herald | `1` | `16,056` | `393,204` |
| `anneal --root /Users/morgan/code/herald/.design check --file=README.md --json` | Herald | `0` | `15,619` | `376,500` |
| `anneal --root /Users/morgan/code/herald/.design check --include-terminal` | Herald | `1` | `202` | `16,875` |
| `anneal --root /Users/morgan/code/murail/.design check` | Murail | `0` | `6` | `333` |
| `anneal --root /Users/morgan/code/murail/.design check --json` | Murail | `0` | `129,645` | `3,072,828` |
| `anneal --root /Users/morgan/code/murail/.design check --include-terminal` | Murail | `1` | `408` | `35,470` |

Assessment:

- Plain `check` is acceptable for interactive use and small enough to recommend to agents.
- Any JSON form of `check` is structurally unsafe by default.
- The `--file` filter narrows diagnostics but not the serialized `extractions`, so it creates a false sense of boundedness.
- `--active-only` does not make JSON safe.
- `--include-terminal` is materially larger in human mode, but still much safer than JSON mode.

Rationale:

- `CheckOutput` always includes `extractions` in JSON-facing data structures in `src/cli.rs:106`.
- `main.rs` unconditionally clones `result.extractions` whenever `cli_args.json` is true in `src/main.rs:684`.

Suggestions:

- Consider making `check --json` summary-first by default.
- Consider moving `extractions` behind an explicit flag such as `--include-extractions`.
- Consider filtering `extractions` when `--file` is present.
- Consider offering a compact JSON summary shape with counts and the first `N` diagnostics.

### `anneal get`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal get anneal-spec.md` | self | `0` | `4` | `268` |
| `anneal get anneal-spec.md --json` | self | `0` | `9` | `356` |
| `anneal --root /Users/morgan/code/herald/.design get README.md` | Herald | `0` | `4` | `148` |
| `anneal --root /Users/morgan/code/herald/.design get README.md --json` | Herald | `0` | `9` | `236` |
| `anneal --root /Users/morgan/code/murail/.design get LABELS.md` | Murail | `0` | `28` | `676` |
| `anneal --root /Users/morgan/code/murail/.design get LABELS.md --json` | Murail | `0` | `3,216` | `59,140` |
| `anneal --root /Users/morgan/code/murail/.design get OPEN-QUESTIONS.md` | Murail | `0` | `29` | `816` |
| `anneal --root /Users/morgan/code/murail/.design get OPEN-QUESTIONS.md --json` | Murail | `0` | `796` | `14,643` |

Assessment:

- Human `get` is generally safe.
- JSON `get` is safe for ordinary handles, but unsafe on hub handles with many edges.
- This makes `get --json` a medium-risk command whose safety depends heavily on the chosen handle.

Rationale:

- Human mode caps displayed incoming and outgoing edges with `EDGE_DISPLAY_LIMIT` in `src/cli.rs:262`.
- JSON mode returns the full edge lists because the underlying `GetOutput` is serialized directly.

Suggestions:

- Consider adding edge counts plus capped edge samples in JSON output.
- Consider adding an explicit `--full-edges` flag for callers that truly need the complete adjacency list.
- Consider warning in docs/examples against `get --json` on hub files such as label indexes.

### `anneal find`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal find anneal` | self | `0` | `68` | `4,666` |
| `anneal find anneal --json` | self | `0` | `408` | `9,908` |
| `anneal find "" --status=draft` | self | `0` | `2` | `78` |
| `anneal find "" --status=draft --json` | self | `0` | `12` | `172` |
| `anneal --root /Users/morgan/code/herald/.design find OQ` | Herald | `0` | `13` | `749` |
| `anneal --root /Users/morgan/code/herald/.design find OQ --namespace=OQ --json` | Herald | `0` | `66` | `1,335` |
| `anneal --root /Users/morgan/code/herald/.design find ""` | Herald | `0` | `3,270` | `415,787` |
| `anneal --root /Users/morgan/code/herald/.design find "" --json` | Herald | `0` | `19,620` | `670,286` |
| `anneal --root /Users/morgan/code/murail/.design find ""` | Murail | `0` | `10,084` | `1,290,850` |
| `anneal --root /Users/morgan/code/murail/.design find "" --json` | Murail | `0` | `60,504` | `2,077,129` |
| `anneal --root /Users/morgan/code/murail/.design find "" --status=living --json` | Murail | `0` | `30` | `583` |
| `anneal --root /Users/morgan/code/murail/.design find "" --kind=label --json` | Murail | `0` | `3,000` | `64,895` |

Assessment:

- `find` is safe only when the query or filters are narrow.
- Broad queries are one of the easiest ways to dump huge output by accident.
- Empty query is especially dangerous because it acts like "return everything".

Rationale:

- `cmd_find()` performs unrestricted case-insensitive substring matching and returns all matches in `src/cli.rs:486`.
- There is no built-in limit, pagination, or summary mode.

Suggestions:

- Consider rejecting empty query unless a limit or explicit `--all-matches` flag is supplied.
- Consider adding `--limit`.
- Consider making JSON output return counts plus the first `N` matches by default.
- Consider a separate `find --summary` mode for agent use.

### `anneal init`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal init --dry-run` | self | `0` | `53` | `780` |
| `anneal init --dry-run --json` | self | `0` | `64` | `1,294` |
| `anneal --root /Users/morgan/code/herald/.design init --dry-run` | Herald | `0` | `97` | `1,379` |
| `anneal --root /Users/morgan/code/herald/.design init --dry-run --json` | Herald | `0` | `106` | `2,084` |
| `anneal --root /Users/morgan/code/murail/.design init --dry-run` | Murail | `0` | `131` | `1,882` |
| `anneal --root /Users/morgan/code/murail/.design init --dry-run --json` | Murail | `0` | `140` | `2,741` |

Assessment:

- `init` is compact and safe by default.
- It scales with inferred config complexity, but remained small across all tested corpora.

Suggestions:

- No urgent change needed.
- If agent-focused polish is desired, `init --json` could optionally expose a compact diff-like summary of inferred settings in addition to the full config.

### `anneal impact`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal impact anneal-spec.md` | self | `0` | `4` | `97` |
| `anneal impact anneal-spec.md --json` | self | `0` | `5` | `67` |
| `anneal --root /Users/morgan/code/herald/.design impact README.md` | Herald | `0` | `4` | `97` |
| `anneal --root /Users/morgan/code/herald/.design impact README.md --json` | Herald | `0` | `5` | `62` |
| `anneal --root /Users/morgan/code/murail/.design impact LABELS.md` | Murail | `0` | `4` | `140` |
| `anneal --root /Users/morgan/code/murail/.design impact LABELS.md --json` | Murail | `0` | `7` | `121` |

Assessment:

- `impact` looks safe on the sampled handles.
- It is still structurally unbounded because both direct and indirect lists are returned in full, but it does not appear to be a practical problem on the tested corpora.

Rationale:

- `ImpactOutput` serializes the full `direct` and `indirect` vectors in `src/cli.rs:560`.

Suggestions:

- No urgent change needed.
- Consider adding counts to output, which would make it easier to understand blast radius before reading full lists.
- Consider sampling or limiting indirect lists in JSON if future corpora reveal very large dependency fanout.

### `anneal map`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal map` | self | `0` | `71` | `2,918` |
| `anneal map --json` | self | `0` | `6` | `3,058` |
| `anneal map --around=anneal-spec.md` | self | `0` | `3` | `37` |
| `anneal map --around=anneal-spec.md --json` | self | `0` | `6` | `108` |
| `anneal --root /Users/morgan/code/herald/.design map` | Herald | `0` | `3,394` | `245,305` |
| `anneal --root /Users/morgan/code/herald/.design map --json` | Herald | `0` | `6` | `248,812` |
| `anneal --root /Users/morgan/code/murail/.design map` | Murail | `0` | `10,269` | `749,196` |
| `anneal --root /Users/morgan/code/murail/.design map --json` | Murail | `0` | `6` | `759,858` |
| `anneal --root /Users/morgan/code/murail/.design map --around=LABELS.md --depth=1` | Murail | `0` | `493` | `6,339` |
| `anneal --root /Users/morgan/code/murail/.design map --around=LABELS.md --depth=2` | Murail | `0` | `616` | `13,438` |
| `anneal --root /Users/morgan/code/murail/.design map --around=LABELS.md --depth=2 --json` | Murail | `0` | `6` | `14,127` |

Assessment:

- Full `map` is structurally unsafe on real corpora.
- `map --json` is worse than it looks because it returns a short JSON wrapper around a very large `content` string.
- `map --around` can be useful and reasonably bounded at shallow depth, but BFS growth is real.

Rationale:

- `MapOutput` includes `content: String` in `src/cli.rs:1233`.
- Neighborhood extraction expands with BFS depth in `src/cli.rs:1252`.

Suggestions:

- Consider making JSON `map` return structured node and edge data, not a rendered graph blob.
- Consider renaming the current JSON shape to something like `--json-rendered` if the intent is "serialize the human graph view".
- Consider adding node and edge caps or a warning banner when a map is large.
- Consider making `--around` the recommended default for graph inspection in docs and help.

### `anneal diff`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal diff` | self | `0` | `4` | `126` |
| `anneal diff --json` | self | `0` | `19` | `345` |
| `anneal --root /Users/morgan/code/herald/.design diff` | Herald | `0` | `4` | `126` |
| `anneal --root /Users/morgan/code/herald/.design diff --json` | Herald | `0` | `19` | `345` |
| `anneal --root /Users/morgan/code/murail/.design diff` | Murail | `0` | `4` | `126` |
| `anneal --root /Users/morgan/code/murail/.design diff --json` | Murail | `0` | `19` | `345` |

Assessment:

- `diff` is compact and agent-safe.
- It is a strong candidate for "resume where I left off" defaults.

Suggestions:

- No urgent change needed.
- If desired, `diff --json` could expose an even more compact summary mode, but the current shape is already safe.

### `anneal obligations`

Representative measurements:

| Invocation | Corpus | Exit | Lines | Bytes |
| --- | --- | --- | --- | --- |
| `anneal obligations` | self | `0` | `1` | `51` |
| `anneal obligations --json` | self | `0` | `6` | `95` |
| `anneal --root /Users/morgan/code/herald/.design obligations` | Herald | `0` | `1` | `51` |
| `anneal --root /Users/morgan/code/herald/.design obligations --json` | Herald | `0` | `6` | `95` |
| `anneal --root /Users/morgan/code/murail/.design obligations` | Murail | `0` | `1` | `51` |
| `anneal --root /Users/morgan/code/murail/.design obligations --json` | Murail | `0` | `6` | `95` |

Assessment:

- Safest command in the CLI.
- The current output shape is compact and stable.

Suggestions:

- No change needed.
- This is a good example of what an agent-safe JSON surface looks like.

## Suggested Risk Labels

If `anneal` ever wants to describe output surfaces more explicitly, the current commands naturally group into these labels:

| Label | Meaning | Current commands |
| --- | --- | --- |
| Compact summary | Good default for agents | `status`, `status --json`, `diff`, `diff --json`, `obligations`, `obligations --json` |
| Bounded inspection | Usually safe if the target is narrow | plain `get`, plain `impact`, `init --dry-run`, narrow `find`, `map --around` |
| Full dump | Can consume large context windows quickly | `check --json`, broad `find`, full `map`, `map --json`, `get --json` on hub handles |

## Highest-Value Suggestions

These suggestions are framed in descending order of likely impact.

1. Consider making `check --json` summary-first and moving `extractions` behind an explicit opt-in flag.
2. Consider changing `map --json` so it returns structured graph data or summary fields instead of embedding full rendered `content`.
3. Consider adding `--limit` and empty-query safeguards to `find`.
4. Consider giving `get --json` the same edge-capping behavior that human mode already uses.
5. Consider annotating help/docs with "compact" versus "full dump" guidance so agent users can choose safe commands without reverse-engineering payload size.

## Recommended Agent Defaults

Today, without changing the code, the safest defaults for agent-oriented workflows are:

- `anneal status --json`
- `anneal check --active-only`
- `anneal diff`
- `anneal obligations`
- `anneal find <query>` only when paired with a narrowing query or filters
- `anneal map --around=<handle> --depth=1` instead of full `map`

Commands to avoid as defaults:

- `anneal check --json`
- `anneal find ""`
- `anneal find "" --json`
- `anneal map`
- `anneal map --json`

## Closing Note

The central insight from this audit is that `anneal` does not have a general "JSON problem". It has a small number of output shapes that are currently optimized for completeness rather than bounded consumption.

That is good news. It means a few focused changes could make the CLI much more agent-safe without weakening the existing human-oriented workflows.
