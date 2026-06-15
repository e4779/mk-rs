# Pre-Release Review — mk-rust — 2026-06-15

## Gates

| Gate | Status |
|------|--------|
| `cargo clippy --all-targets -- -D warnings` | ✅ Clean |
| `cargo test` (293 tests) | ✅ All passing |
| `cargo build` | ✅ Clean |

---

## 🔴 Bug found

### 1. Metarule `$alltarget` contains pattern, not concrete target name

**File:** `crates/mk-cli/src/main.rs:245`

**Code:**
```rust
if match_simple(&node.name, pat).is_some() {
    rules.insert(node.name.clone(), ResolvedRule {
        recipe: r.recipe.clone().unwrap_or_default(),
        attributes: r.attributes,
        all_targets: r.targets.clone(),  // ← BUG: pattern, not concrete
    });
}
```

**Problem:** When a metarule like `%.o: %.c` is resolved for graph node `hello.o`,
the `all_targets` field is set to `["%.o"]` (the pattern from `r.targets`).
It should be `["hello.o"]` (the actual target).

The Plan 9 mk spec says `$alltarget` in a recipe contains the concrete target
name(s). For metarule resolutions, there is always exactly one concrete target:
the node being resolved.

**Impact:** If a metarule recipe references `$alltarget`, it will see the pattern
string (`%.o`) instead of the actual target name (`hello.o`). In practice this
is rare — metarule recipes typically use `$target` or `$stem` — but it is
semantically wrong.

**Same bug in test helper:** `crates/mk-core/src/sched.rs:640` has the same
`all_targets: r.targets.clone()` in its metarule resolution loop. Not currently
triggered because no sched.rs test uses metarules with `$alltarget`, but will
surface if such a test is added.

**Fix:** Change both occurrences to use the concrete node name:
```rust
all_targets: vec![node.name.clone()],
```

**Severity:** Low. Unlikely to affect real-world mkfiles because:
- `$alltarget` is rarely used in metarule recipes (users prefer `$stem`/`$target`)
- Metarules always have a single target, so `$alltarget` ≅ `$target` anyway

---

## 🟡 Pre-existing (not from this session)

### 2. `io_other_error` clippy warnings in mk-shell (duckscript feature)

**File:** `crates/mk-shell/src/lib.rs:208,216`

```rust
std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
// use std::io::Error::other(e.to_string()) instead
```

Only surfaces with `--all-features` (duckscript feature). Not touched in this
session's 3 commits. The default build (without --all-features) is clean.

**Severity:** Trivial, pre-existing.

---

## Not findings (confirmed correct)

- ✅ `escape_dollar_dollar` — pairwise conversion correct for all edge cases ($, $$, $$$, $$$$)
- ✅ `check_stale` short-circuit fix — full traversal, no regressions
- ✅ `$NREP` wiring — reads from scope, guards `.max(1)`, propagates to build_graph_with_nrep
- ✅ `N` attribute enforcement — checked BEFORE recipe execution, returns success
- ✅ `$(...)` rejection — catches GNU Make syntax in all prereqs, preserves archive member syntax
- ✅ `keep_going` dedup — `rem.remove().is_some()` guard prevents duplicate failed entries
- ✅ Build scripts — clippy `.or_else(git_short)` fix applied
- ✅ `to_dot` — `if let Some(prog)` replaces unwrap
- ✅ `matches!` macro — replaces match_like_matches_macro pattern
- ✅ No unsafe unwrap() calls in production code (all mutex/arc unwraps justified)
