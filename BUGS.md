# Known bugs & resolved issues — mk-rust

This file tracks bugs found in real-world use (e.g. the invest-research
pipeline) and their resolution. For unimplemented spec features, see
[`TRACEABILITY.md`](TRACEABILITY.md) (rows marked `◐` / `—`) — those are
gaps by design, not bugs.

## Current status (2026-06-15)

| Gate | Status |
|------|--------|
| `cargo test` | ✅ 305 passing, 0 failed, 1 ignored |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ Clean |
| `cargo build` | ✅ Clean |

**No active bugs.** Both entries below are resolved.

---

## Resolved

### Bug 1: `$$` did NOT escape `$` in recipes — FIXED

**Found:** 2026-06-15 (invest-research pipeline refactoring)

**Expected:** `$$prereq` in a mkfile recipe → `$prereq` in shell → expands to
the prereq list.

**Actual (before fix):** `$$prereq` → `1234prereq` in shell (PID + "prereq") —
`$$` was passed verbatim to `sh -c`, which expands it to the process ID.

**Reproduction (historical):**
```makefile
# mkfile
test1: prereq.txt
	echo $$prereq should be "prereq.txt", got: $prereq
```

**Root cause:** Recipe text was passed verbatim to `sh -c`. `$$` was not
converted to `$` during recipe processing.

**Fix:** [`escape_dollar_dollar`](crates/mk-core/src/recipe.rs) now runs on
every recipe script before handing it to the shell (recipe.rs:113). Converts
`$$` → `$` pairwise, left-to-right (`$$$` → `$$`, `$$$$` → `$$`).

**Commit:** [`a9208b3`](https://) `fix: $ escape in recipes + reject GNU Make syntax in prereqs`

**Tests:** `dollar_dollar_escape_in_recipe`, `dollar_dollar_at_end_of_recipe`,
`triple_dollar_in_recipe`, `quad_dollar_in_recipe` (recipe.rs).

**Impact of the workaround note:** The previous workaround ("use `$1` directly
without `$$`") is no longer needed — `$$` now escapes correctly, so
`set -- $prereq; echo $1` works as written in the mkfile.

---

### Bug 2: `$(wildcard ...)` in prereqs caused perpetual staleness — FIXED

**Found:** 2026-06-15 (invest-research pipeline refactoring)

**Expected:** A target using wildcard-matched prereqs is not rebuilt when the
matched files haven't changed.

**Actual (before fix):** The target was ALWAYS considered stale and rebuilt
every run. Additionally, `wildcard: command not find` appeared during parsing —
`$(...)` was passed through to the shell as command substitution.

**Reproduction (historical):**
```makefile
# mkfile
wildcard-test: prereq.txt $(wildcard /tmp/test/*.txt)
	touch $target
```
`mk wildcard-test` built repeatedly instead of skipping.

**Root cause:** `$(...)` is GNU Make syntax. mk has no such function — it uses
native glob patterns (`*.txt`, `dir/*.c`) in prereqs, expanded at graph-build
time. The `$(...)` form was not handled, so it leaked to the shell.

**Fix:** The parser now [rejects `$(...)` in prereqs](crates/mk-core/src/parse.rs)
with a clear error pointing the user at mk's glob syntax (parse.rs:295-307):

> `GNU Make syntax $(...) in prereq '...' is not supported`

Replace `$(wildcard /tmp/test/*.txt)` with just `/tmp/test/*.txt`.

**Commit:** [`a9208b3`](https://) `fix: $ escape in recipes + reject GNU Make syntax in prereqs`

**Tests:** `rejects_gnu_make_wildcard_syntax`,
`rejects_dollar_paren_in_any_prereq`, `allows_dollar_without_paren_in_prereq`,
`allows_parentheses_without_dollar_in_prereq` (parse.rs). The last two confirm
the check is narrow — it does not reject legitimate `$VAR` refs or literal
parentheses.

---

## Environment

- mk-rust built from `~/dev/mk-rust/` (HEAD `71a2e76` or later)
- Default shell: `/bin/sh`
- OS: Linux
