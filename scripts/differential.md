# Differential receipts

Run the same command with two materialized binaries against content-pinned inputs:

```sh
just differential --left /tmp/before/anneal --right /tmp/after/anneal --root .design --output /tmp/identity-receipt -- --root '{root}' context identity
```

The output directory must be new and outside the inputs. `receipt.json` records
both binary paths and SHA256 digests, actual argv/cwd, corpus content digest and
file count, load averages, wall time, exit status, stdout/stderr paths, declared
exclusions and comparable artifacts. `left/` and `right/` retain stdout, stderr
and compared bytes. Exit status is 0 for equal, 1 for different, 2 for refused.
The script requires Python 3.11 or later and does not build revisions, snapshot a
workspace, invoke a shell, or move bookmarks. Materialize immutable refs using
the normal workspace workflow and supply their binaries.

`--mode ndjson` sorts complete JSON object rows and keys, retaining duplicate
rows and array order. Number tokens retain exact spelling/precision (numeric
spellings such as `1.0` and `1e0` remain distinct); no float coercion can hide a
difference. Malformed JSON, duplicate object keys, nonfinite values,
non-object rows and zero rows refuse comparison. Blank lines do not count as
rows. `--mode raw` compares exact bytes; its row count is the number of byte
lines, including an unterminated final line. Empty bytes refuse comparison.

A comparable artifact has a positive row count, SHA256, mode and exclusion list.
Equality compares all four. A refused side has no comparable artifact, with its
side label and reason in the receipt. In particular two empty streams cannot
produce an equal outcome, and equal hashes with different row counts differ.
Stdout from a nonzero or timed-out command remains evidence, never a comparable
artifact; stderr is captured separately even on success.

Declare field exclusions explicitly, for example `--exclude /identity/native_id`.
These are exact object-field JSON pointers; `~0` escapes `~`, `~1` escapes `/`.
There are no wildcards or array-element deletions. The receipt records the entire
list and removal count per side/pointer. An exclusion matching no fields reports
zero removals. Raw mode rejects exclusions. Differences in any undeclared field
remain differences. Exclusion counts may differ, so read them as well as the
comparison status.

Inputs include all files below `--root`, including hidden files and symlink target
contents. No ignore rules apply. Empty roots, missing files, unreadable entries,
special files and cyclic directory links refuse. Declare additional dependencies
with repeated `--input-root PATH` (directories or individual files), including
mounted corpora and any wrapper script. Digests are checked before and after each
invocation. This tests stability at those boundaries; it is not a filesystem
snapshot or proof that an undeclared dependency was pinned. Choose a bounded
corpus root rather than a project directory full of unrelated build output.

Arguments normally follow the binary. An explicit command template containing
`{binary}` can adapt a file-producing extractor into a stdout stream:

```sh
just differential --left /tmp/old-extractor --right /tmp/new-extractor --root /tmp/source-snapshot --input-root /tmp/stream.py --output /tmp/extraction-receipt -- python3 /tmp/stream.py '{binary}' '{root}'
```

The argv template is an argument vector, not shell syntax. The wrapper must
actually invoke the supplied binary and propagate failures; its code is part of
the review evidence. The declared binary digest and wrapper input digest are in
the receipt. Wall times are observations only; this is not a paired performance
instrument.

`python3 scripts/test-differential.py` runs adversarial controls. `just check`
runs that same test file: empty sides, nonzero exits, fabricated equal-hash/count
mismatch, undeclared exclusions, duplicate loss, invalid JSON, raw-byte changes,
corpus mutation, missing binaries and timeouts must fail their comparison.
