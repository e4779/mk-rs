# Final Review — mk-rust — 2026-06-15

## Gate results

| Gate | Status |
|------|--------|
| `cargo clippy --all-targets -- -D warnings` | ✅ Clean (one `useless_format!` fixed) |
| `cargo test` (293 tests) | ✅ All passing |
| `cargo build` | ✅ Clean |

---

## 🔴 Real bugs

### 1. `check_stale` short-circuit misses existing-but-stale sibling prereqs

- **File:** `crates/mk-core/src/graph.rs:661`
- **Code:** `let prereq_stale = node.arcs_in.iter().any(|&arc_idx| { ... })`
- **Problem:** `Iterator::any()` short-circuits on the first stale prereq. Remaining prereqs are never visited by `check_stale`, so they aren't added to the result set and aren't memoized.

**Impact:** When a target has 2+ prereqs and the first is stale, later prereqs that are also stale BUT have existing files are silently skipped. The fix-up pass in `sched.rs:202-210` only adds nodes with `mtime.is_none()` (missing files), so existing-but-stale files are missed.

**Example scenario:**
```
target: a b
a: c          # c is newer → a is stale
b: d          # d is newer → b is stale, b exists on disk
```
→ `check_stale(target)` visits `a` first, finds it stale → short-circuits
→ `b` is never visited, never added to stale set
→ `b`'s recipe never runs even though `d` changed
→ target is built using stale `b`

**Fix:** Replace `any()` with a full traversal:
```rust
let mut prereq_stale = false;
for &arc_idx in &node.arcs_in {
    if check_stale(graph, graph.arcs[arc_idx.0].from, memo, result, in_result, force_intermediates) {
        prereq_stale = true;
        // Don't break — continue so all prereqs are visited and added to result
    }
}
```

**Severity:** Medium. Only triggers with metarule-generated multi-prereq targets where sibling prereqs are independently stale. Concrete rules rarely have multiple independently-stale prereqs.

---

## 🟡 Missing coverage / spec gaps

### 2. No test for `check_stale` sibling-short-circuit

No test covers the scenario where a node with 2+ prereqs has both prereqs stale (existing files, not missing). The existing `missing_intermediate_cascades_to_dependents` test only covers missing intermediates.

**Suggestion:** Add a test with concrete file prereqs where both are independently stale.

---

### 3. `N` (NO_EXEC) attribute not checked during recipe execution

- **File:** `crates/mk-core/src/recipe.rs` `run()` function
- **Problem:** `recipe.attributes.is_no_exec()` is never called. The `N` attribute is parsed and stored but has no effect on execution. (The `-n` CLI flag is a separate mechanism handled by `opts.no_exec`).

In Plan 9 mk, the `N` attribute means "treat target as updated without running the recipe." This is used when a recipe updates a side-effect file.

**Severity:** Low. The attribute is parsed and the infrastructure exists. Likely deferred to a later phase per PLAN.md.

---

### 4. Duplicate failed entries in keep_going path

- **File:** `crates/mk-core/src/sched.rs:512-515`
- **Problem:** When keep_going is true and a node has multiple failed prereqs, the dependent is added to `failed` once per failed prereq. It's removed from `remaining` on the first failure, but re-added to `failed` on subsequent prereq failures.

```rust
if !success {
    if let Some(deps) = dependents_ref.get(&node_idx.0) {
        let mut f = failed.lock().unwrap();
        let mut rem = remaining.lock().unwrap();
        for &dep_idx in deps {
            let dep_name = graph_ref.nodes[dep_idx.0].name.clone();
            f.push((dep_name.clone(), format!("prerequisite '{}' failed", name)));
            rem.remove(&dep_idx.0);
        }
    }
    continue;
}
```

**Impact:** A target with 2 failed prereqs appears twice in `BuildOutcome.failed`. Cosmetic — doesn't affect correctness.

**Fix:** Check if the dependent is already removed from `remaining` before pushing to `failed`.

---

## 🟢 Suggestions

### 5. `build_node` has too many parameters (8)

- **File:** `crates/mk-core/src/graph.rs:286`
- **Suggestion:** Bundle graph state into a `BuildContext` struct to reduce parameter count and improve readability. This is a cleanliness issue, not a bug.

### 6. Missing test: `NREP=0` edge case

`NREP` defaults to 1, and `nrep_limits_recursion_depth_1` tests NREP=1. `NREP=0` would mean "no metarule expansion at all" — what happens? It's parsed from `$NREP` which could theoretically be set to 0 in the environment (though mk convention is that NREP >= 1). Consider adding a test or a `.max(1)` guard on the NREP value.

---

## Clean

- ✅ `$$→$` escape logic in `recipe.rs:escape_dollar_dollar` — correct, well-tested (normal, end-of-string, triple, quad)
- ✅ `$(...)` rejection in `parse.rs` — correct, tested with positive and negative cases
- ✅ `$NREP` — default to 1, `build_graph_with_nrep` propagates correctly through recursion
- ✅ `$alltarget` — injected correctly in `recipe::run()`, tested for single and multi-target rules
- ✅ No unsafe `unwrap()` calls in production code (all mutex/arc unwraps are justified)
- ✅ No panic paths in graph builder — all errors use Result
- ✅ All 9 attributes correctly parsed and stored
