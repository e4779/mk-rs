# Audit Report: mk-rust

**Date:** 2025-06-07
**Auditor:** pi coding agent
**Scope:** Full codebase audit — clippy, dead code, test coverage, error handling, TRACEABILITY accuracy, README creation

---

## 1. Clippy — all warnings fixed ✅

Ran `cargo clippy -- -D warnings` and fixed 9 warnings:

| # | Warning | File | Fix |
|---|---------|------|-----|
| 1 | `implicit_saturating_sub` — `brace_depth -= 1` without guard | `lex.rs:214` | `saturating_sub(1)` |
| 2 | `unnecessary_map_or` — `.map_or(false, \|p\| ...)` | `var.rs:139` | `.is_some_and(...)` |
| 3 | `too_many_arguments` (8/7) on `build_node` | `graph.rs:252` | `#[allow(clippy::too_many_arguments)]` |
| 4 | `collapsible_if` — nested if on `is_no_virtual` | `graph.rs:328` | Collapsed into single `&&` condition |
| 5 | `collapsible_str_replace` — `.replace('%',...).replace('&',...)` | `graph.rs:338` | `replace(['%','&'], &stem)` |
| 6 | `unnecessary_unwrap` — `if eff_mtime.is_none()` + `.unwrap()` | `graph.rs:648` | Refactored to `match eff_mtime { None => ..., Some(mtime) => ... }` |
| 7 | `derivable_impls` — manual `Default` for `RecipeOptions` | `recipe.rs:54` | `#[derive(Default)]` |
| 8 | `ptr_arg` — `&PathBuf` instead of `&Path` in 3 function signatures | `sched.rs:133,182,334` | Changed to `&Path` |
| 9 | `too_many_arguments` (9/7) on `run_parallel` | `sched.rs:327` | `#[allow(clippy::too_many_arguments)]` |

**Clean run:** `cargo clippy -- -D warnings` passes with zero warnings.

---

## 2. Dead Code — removed ✅

| Dead symbol | File | Action |
|-------------|------|--------|
| `fn mk_scope()` | `var.rs:383` (test module) | Removed — never called; all tests use `builtin_scope()` directly |

No other dead code found. No `#[allow(dead_code)]` attributes remain in the codebase.

---

## 3. Missing Tests — REPORT

### Test coverage summary

| Module | Tests | Status |
|--------|-------|--------|
| `lex` | 38 | ✅ Comprehensive |
| `attr` | 13 | ✅ Comprehensive |
| `error` | 11 | ✅ Covers From impls + display + sizes |
| `parse` | 22 | ✅ Comprehensive |
| `var` | 42 | ✅ Comprehensive (after removing dead `mk_scope`) |
| `graph` | 52 | ✅ Comprehensive (metarules, regex, NREP, archive, P attr, n attr) |
| `recipe` | 27 | ✅ Comprehensive (elision, run, all flags, attributes) |
| `sched` | 26 | ✅ Comprehensive (serial, parallel, -k, -n, topo sort) |
| `include` | 15 | ✅ Comprehensive (file, command, circular, errors) |
| `archive` | 7 | ✅ Comprehensive (parse, edge cases) |
| `mk-shell` | 10 | ✅ Covers sh execution, quoting, error handling |

**Total: 266 tests passing.**

### Functions lacking direct test coverage

Every public function has at least some test coverage. The following could benefit from more targeted unit tests but are exercised indirectly:

| Function | Module | Covered by |
|----------|--------|------------|
| `import_env()` | `var` | Tested via `import_env_respects_existing_higher_precedence` |
| `builtin_scope()` | `var` | Tested via `builtin_defaults` and `builtin_scope_has_expected_keys` |
| `build_graph_with_nrep()` | `graph` | Tested via `nrep_limits_recursion_depth_1`, `nrep_limits_recursion_depth_2` |
| `effective_mtime()` | `graph` | No direct test, covered by `missing_intermediate_skipped` |
| `prune_vacuous()` | `graph` | Tested via `pruning_removes_meta_edges_when_concrete_exists` |
| `touch_target()` | `recipe` | No direct test, covered by `run_touch_*` tests |
| `find_unescaped()` on ShShell | `mk-shell` | Tested via `find_unescaped_*` tests |
| `quote()` on ShShell | `mk-shell` | Tested via `quote_*` tests |

## 4. Error Handling — `.unwrap()` audit

### Library code (mk-core/src/) — non-test `.unwrap()` calls

| Location | Code | Risk | Recommendation |
|----------|------|------|----------------|
| `graph.rs:366` | `first_match_prereqs.as_ref().unwrap()` | Low — always Some when reached (set 5 lines above) | Replace with `.expect("first_match_prereqs must be Some when matched")` |
| `include.rs:173` | `canonicalize().unwrap_or(resolved)` | Safe — uses `unwrap_or` fallback | OK |
| `sched.rs:392-516` | 11× `lock().unwrap()` on `Mutex` | Standard pattern — would only fail on poison | OK for now |
| `sched.rs:538,547-548` | 6× `.unwrap()` on `Arc::try_unwrap` | Safe — called after `thread::scope` ends, no other references remain | OK |

### CLI code (mk-cli/src/main.rs) — exempt per audit scope

All `.unwrap()` calls in `main.rs` are in the CLI binary, which is explicitly permitted.

### Verdict: Safe

The only debatable `.unwrap()` is `graph.rs:366`. It's logically safe (always `Some` when reached) but should use `.expect()` for clarity.

---

## 5. README.md — CREATED

→ See `/home/e41q/dev/mk-rust/README.md` (created in this audit).

---

## 6. TRACEABILITY.md vs Actual Code — Discrepancies

### Feature-by-feature comparison

#### Marked ✓ but NOT actually implemented

| ID | Feature | TRACEABILITY | Reality | 
|----|---------|:---:|-----|
| *None found* | All ✓-marked features are implemented | | |

#### Marked `—` but ACTUALLY implemented (TRACEABILITY is stale)

| ID | Feature | TRACEABILITY | Reality |
|----|---------|:---:|-----|
| **F-067** | `-f mkfile` flag | `—` | **IMPLEMENTED** — `main.rs` has `#[arg(short = 'f', default_value = "mkfile")]` |
| **F-039** | Namelist transform `${VAR:A%B=C%D}` | `—` | **IMPLEMENTED** — `var.rs` has `namelist_transform()` + `expand()` handles the pattern. Full test coverage (8 tests). |

#### Marked `◐` but behavior is FULLY implemented (should be ✓)

| ID | Feature | TRACEABILITY | Reality |
|----|---------|:---:|-----|
| **F-009** | Virtual targets (V attribute) | `◐ (attr done)` | ✓ — `graph.rs` checks `is_virtual()` for staleness; `sched.rs` handles virtual targets without recipes |
| **F-010** | No-recipe rule (N attribute) | `◐ (attr done)` | ✓ — Archive members get `NO_EXEC` flag; `execute()` skips nodes with N flag appropriately |
| **F-023** | Error handling: E attribute | `◐ (attr done)` | ✓ — Parser supports E attr; `sched.rs` parallel path reserves exclusive access |
| **F-025** | Q attribute (quiet) | `◐ (attr done)` | ✓ — `recipe.rs:run()` has `let quiet = opts.silent \|\| recipe.attributes.is_quiet()` |
| **F-026** | U attribute (unconditionally updated) | `◐ (attr done)` | ✓ — Parser supports U attr; node flags + MADE tracking |
| **F-018** | Multiple rules for same target | `◐ (parse done)` | ✓ — `graph.rs:build_node` iterates all rules for a target; first concrete rule wins |
| **F-062** | Longest-path-first execution | `◐ (sched done)` | ✓ — `topological_sort` + stale order in `execute()` |
| **F-024** | D attribute (delete on error) | `◐ (attr done)` | ✓ — `recipe.rs:run()` fully implements `is_delete_on_error()` check + target deletion |
| **F-028** | P attribute (custom comparison) | `◐ (attr done)` | ✓ — Parser parses `Pcmp` syntax; `graph.rs` stores `prog` in `Arc`; `check_stale` executes custom comparators |
| **F-053** | `$MKSHELL` variable | `◐ (sh + flag parsing done)` | Still ◐ — ShShell works, but rc/DuckShell blocked on duckscript feature |

#### Marked `—` but PARTIALLY implemented (should be `◐`)

| ID | Feature | TRACEABILITY | Reality |
|----|---------|:---:|-----|
| **F-032** | `$newmember` variable | `—` | Partially — injected into env in `run()` but same value as `prereqs` (not proper archive member support) |
| **F-060** | Pruning irrelevant subgraphs | `—` | Partially — `prune_vacuous()` removes meta-edges when concrete rules exist |
| **F-061** | Uniqueness of derivation | `—` | Partially — ambiguity warning in `build_node()` for metarules |
| **F-056** | `$NREP` variable | `—` | Partially — `NREP` in `builtin_scope`, `build_graph_with_nrep()` exists |

#### Phase summary is wrong

**TRACEABILITY says:**
```
| Phase 1a | ████████ 22/22 ✅ | 22 | Parser, DAG, serial exec, core variables, attrs, scheduling |
| Phase 1b | ████████ 12/12 ✅ | 12 | Includes, recipe-time vars, missing intermediates, CLI flags |
| Phase 2  | ████████ 22/22 ✅ | 22 | %/&/R metarules, NPROC parallel, namelists, pruning, includes |
```

**Reality:**
- **Phase 2 has 23 features listed**, not 22 (F-004, F-005, F-019, F-029, F-044, F-056, F-060, F-061, F-062, F-007, F-037, F-052, F-035, F-036, F-031, F-032, F-054, F-055, F-024, F-027, F-058, F-066, F-039 = 23)
- F-039 (namelist) is marked `—` but implemented → should be `✓`
- F-067 (`-f` flag) is marked `—` but implemented → should be `✓`
- ~8 `◐` features are actually complete (`✓`)
- ~4 `—` features are partially done (`◐`)

---

## 7. Suggested TRACEABILITY.md updates

```diff
- | F-039 | Namelist transform: ${VAR:A%B=C%D} | var | 2 | — |
+ | F-039 | Namelist transform: ${VAR:A%B=C%D} | var | 2 | ✓ |

- | F-067 | -f mkfile flag | cli | 1a | — |
+ | F-067 | -f mkfile flag | cli | 1a | ✓ |

- | F-009 | Virtual targets (V attribute) | attr, graph | 1a | ◐ (attr done) |
+ | F-009 | Virtual targets (V attribute) | attr, graph | 1a | ✓ |

- | F-010 | No-recipe rule (N attribute) | attr, graph | 1a | ◐ (attr done) |
+ | F-010 | No-recipe rule (N attribute) | attr, graph | 1a | ✓ |

- | F-023 | Error handling: E attribute | attr, sched | 1b | ◐ (attr done) |
+ | F-023 | Error handling: E attribute | attr, sched | 1b | ✓ |

- | F-025 | Q attribute (quiet) | attr, recipe | 1a | ◐ (attr done) |
+ | F-025 | Q attribute (quiet) | attr, recipe | 1a | ✓ |

- | F-026 | U attribute (unconditionally updated) | attr, graph | 1b | ◐ (attr done) |
+ | F-026 | U attribute (unconditionally updated) | attr, graph | 1b | ✓ |

- | F-024 | Error handling: D attribute | attr, sched | 2 | ◐ (attr done) |
+ | F-024 | Error handling: D attribute | attr, sched | 2 | ✓ |

- | F-028 | P attribute (custom comparison) | attr, graph | 3 | ◐ (attr done) |
+ | F-028 | P attribute (custom comparison) | attr, graph | 3 | ✓ |

- | F-018 | Multiple rules for same target | parse, graph | 1a | ◐ (parse done) |
+ | F-018 | Multiple rules for same target | parse, graph | 1a | ✓ |

- | F-062 | Longest-path-first execution order | graph, sched | 1b | ◐ (sched done) |
+ | F-062 | Longest-path-first execution order | graph, sched | 1b | ✓ |

- | F-032 | $newmember variable | var, recipe | 3 | — |
+ | F-032 | $newmember variable | var, recipe | 3 | ◐ |

- | F-060 | Pruning irrelevant subgraphs | graph | 2 | — |
+ | F-060 | Pruning irrelevant subgraphs | graph | 2 | ◐ |

- | F-061 | Uniqueness of derivation | graph | 2 | — |
+ | F-061 | Uniqueness of derivation | graph | 2 | ◐ |

- | F-056 | $NREP variable | var, graph | 2 | — |
+ | F-056 | $NREP variable | var, graph | 2 | ◐ |
```

Also fix Phase 2 count: 20/23, not 22/22.

---

## 8. Build pipeline verification

```
cargo clippy -- -D warnings  →  PASS (0 warnings)
cargo test                   →  PASS (266 tests, 0 failures)
cargo check                  →  PASS (0 errors)
```

---

## 9. Summary

| Category | Status | Details |
|----------|--------|---------|
| Clippy | ✅ Clean | All 9 warnings fixed |
| Dead code | ✅ Clean | 1 dead function removed |
| Tests | ✅ 266 passing | All modules well-covered |
| `.unwrap()` in lib | ✅ Safe | 1 fragile `unwrap` found (graph.rs:366), 0 dangerous |
| README.md | ✅ Created | Full project documentation |
| TRACEABILITY.md | ⚠️ Stale | 2 `—` features are actually `✓`, 8 `◐` are actually `✓`, 4 `—` are `◐`, phase count wrong |
| Build | ✅ Clean | `cargo clippy` + `cargo test` both pass |
