# Known bugs & resolved issues — mk-rust

This file tracks bugs found in real-world use (e.g. the invest-research
pipeline) and their resolution. For unimplemented spec features, see
[`TRACEABILITY.md`](TRACEABILITY.md) (rows marked `◐` / `—`) — those are
gaps by design, not bugs.

## Current status (2026-06-16)

| Gate | Status |
|------|--------|
| `cargo test` | ✅ 335 passing, 0 failed, 1 ignored |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ Clean |
| `cargo build` | ✅ Clean |

**No active bugs.** All three entries below are resolved.

---

## Resolved

### Bug 3: Variable expansion in rule headers & assignment RHS was broken (F-045) — FIXED

**Found:** 2026-06-15 (invest-research pipeline — `DATA_TOONS = \`{fd ...}` → `target: $DATA_TOONS` treated as literal, breaking incrementality)

**Expected (plan9port reference mk):** Rule-headers and assignment RHS use
read-time variable expansion. `SRCS=foo.txt; target: $SRCS` → prereqs =
`[foo.txt]`. `A=world; B=hello $A` → B = `hello world`.

**Actual (before fix):** mk-rust stored `$SRCS` and `$A` literally in the
AST. Variables were never expanded in rule headers or assignment RHS — only
backtick ran in `Scope::set`.

**Root cause:** `Scope::set` only called `expand_backtick`, never resolved
`$VAR` references. Rule headers in `parse.rs` stored raw tokens without
expansion. `TRACEABILITY.md` marked F-045 as `✓` — this was false and misled
an agent (session `019ecdd2`).

**Fix:** Full F-045 implementation (6-phase, see `docs/design-f-045.md`):

1. `var.rs`: `Scope::set` now fully expands (backtick → variable → namelist);
   added `set_raw` for literal storage; replaced depth-10 cap in `expand`
   with seen-set cycle detector (deep chains resolve, cycles yield
   empty/partial — never error, never hang).
2. `parse.rs`: Added `parse_with_scope(tokens, &mut Scope)`; refactored
   parser to carry scope; assignments expand RHS at parse time.
3. `parse.rs`: Rule headers (targets/prereqs) expanded through scope,
   whitespace-split for multi-word vars (S11a). Metarule detection runs on
   EXPANDED targets (S13). Attribute tokens NOT expanded (S9).
4. `include.rs`: Include path/command expanded through scope (S8).
5. `main.rs`: Scope built BEFORE parse; CLI `VAR=value` parsed FIRST with
   `CommandLine` precedence (sticky-override S10); post-parse assign loop
   removed; `parse_with_scope` used instead of `parse`.
6. Docs: TRACEABILITY F-045/F-039/F-042 honest; BUGS.md (this entry).

**Tests:** +27 new tests (309→335): S1 read-time, S2 assign RHS, S4 word
list, S5 namelist header, S6 `$$` in header, S7 recipe-var empties, S9
attr-no-expand, S12 deep chain/cycles, S13 var-to-metarule/multi-target,
S11a/b word-split, S8 include expand, S10 CLI sticky-override, QU-1 env
literal `$`.

**Known gaps:** S11b/c (literal-glue word splitting with multi-word values)
→ tracked as F-003a quoting divergence.

**Commit:** (this commit)

---

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
