# Bug report — from invest-research

Found during frontier pipeline refactoring (2026-06-15).
Reproduced with minimal test cases below.

## Bug 1: `$$` does NOT escape `$` in recipes

**Expected:** `$$prereq` in mkfile recipe → `$prereq` in shell → expands to prereq list.

**Actual:** `$$prereq` → `1234prereq` in shell (PID + "prereq").

**Reproduction:**
```makefile
# mkfile
test1: prereq.txt
	echo $$prereq should be "prereq.txt", got: $prereq
```
```bash
echo data > prereq.txt && mk test1
# Output: "1230953prereq should be prereq.txt, got: prereq.txt"
#          ^^^^^^^^ PID + "prereq" — $$ not converted to $
```

**Impact:** Cannot use `$1`, `$2`, etc. in recipes (e.g. `set -- $prereq; echo $1`). The workaround is to use `$1` directly (without `$$`), but this relies on mk NOT interpreting `$1` as a variable — which happens to work but feels fragile.

**Root cause:** Recipe text is passed verbatim to `sh -c`. `$$` is not converted to `$` during recipe processing in `mk-core/src/recipe.rs` or the parser.

---

## Bug 2: `$(wildcard ...)` in prereqs causes perpetual staleness

**Expected:** Target not rebuilt when wildcard-matched files haven't changed.

**Actual:** Target is ALWAYS considered stale and rebuilt every time.

**Reproduction:**
```makefile
# mkfile
wildcard-test: prereq.txt $(wildcard /tmp/test/*.txt)
	touch $target
```
```bash
mkdir -p /tmp/test
echo data > /tmp/test/a.txt
echo data > prereq.txt
mk wildcard-test      # builds
mk wildcard-test      # should SKIP, but BUILDS AGAIN
mk wildcard-test      # BUILDS AGAIN (infinite)
```

**Without wildcard** — caching works correctly:
```makefile
no-wildcard: prereq.txt /tmp/test/a.txt
	touch $target
```
```bash
mk no-wildcard         # builds
mk no-wildcard         # SKIPS ✓
```

**Additional symptom:** Bash error `wildcard: command not found` when mkfile is first parsed, suggesting `$(wildcard ...)` is being interpreted by the SHELL as command substitution rather than by mk.

**Impact:** Cannot use wildcard expansion in prereqs. Workaround: list files explicitly or use a sentinel (e.g. `data/bars/.done`).

**Root cause (hypothesis):** `$(...)` syntax is not handled by mk's variable expansion — it passes through to the shell which tries to execute `wildcard` as a command.

---

## Environment

- mk-rust built from `~/dev/mk-rust/` (commit 801cb37 or later)
- Default shell: `/bin/sh`
- OS: Linux
