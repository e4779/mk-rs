# AGENTS.md — mk-rs

Explore the project: `ls`, `cargo test`, `cat Cargo.toml`. Read README.md for features and overview.

## Architecture decisions (rationale)

1. **Arena-based DAG (Vec + indices), not Rc/RefCell.** Contiguous memory, no cycles, deterministic drop. `NodeIndex(usize)` and `ArcIndex(usize)` newtypes for safety. After build, the graph is immutable. → `docs/implementation-comparison.md` §6.2, `docs/mk-spec.md` F-006

2. **Crossbeam/sync threads, not tokio.** Recipe execution is inherently blocking (`Command::status()`). No async runtime needed. `sync::thread::scope` for NPROC-based worker pool. → `PLAN.md` §6.1

3. **sh as default shell, not rc.** Validated by 9base research (also chose sh). Duckscript is an optional feature-gated alternative via `MKSHELL=duckscript` (or any shell via `MKSHELL=node -e`, `MKSHELL=python3 -c`). → `PLAN.md` §6.3, `README.md`

4. **Separate mk-graph binary for visualization.** Keeps `mk` lean (no serde dep). JSON/DOT export, dead-end detection, recipe text — all in mk-graph. → `README.md`

## Gotchas

- Recipe lines are passed verbatim to the shell. `$target`/`$prereq` are injected as env vars, not mk-variable expanded.
- `MKSHELL` splits on whitespace: first token is binary, rest are flags. `MKSHELL=node -e` → `node -e "recipe"`.
- `=` and `:` inside recipe text are NOT split by the lexer (`in_recipe` flag). Recipe lines after TAB are opaque.
- Virtual targets must be explicit (`:V:`) — the graph builder doesn't auto-mark orphan prereqs as virtual.
- Glob expansion (`*.toon`) happens at graph-build time, not at recipe execution time.
