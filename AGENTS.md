# AGENTS.md — mk-rs

Read README.md for features, API, and project overview.

## Architecture decisions (rationale)

1. **Arena-based DAG (Vec + indices), not Rc/RefCell.** Contiguous memory, no cycles, deterministic drop. `NodeIndex(usize)` and `ArcIndex(usize)` newtypes for safety. After build, the graph is immutable.

2. **Crossbeam/sync threads, not tokio.** Recipe execution is inherently blocking (`Command::status()`). No async runtime needed. `sync::thread::scope` for NPROC-based worker pool.

3. **sh as default shell, not rc.** Validated by 9base research (also chose sh). Duckscript is an optional feature-gated alternative via `MKSHELL=duckscript` (or any shell via `MKSHELL=node -e`, `MKSHELL=python3 -c`).

4. **Separate mk-graph binary for visualization.** Keeps `mk` lean (no serde dep). JSON/DOT export, dead-end detection, recipe text — all in mk-graph.

## Workspace structure

```
crates/mk-core/    — lex, parse, graph, var, sched, recipe, shell, attr, archive, include
crates/mk-shell/   — Shell trait + sh/duckscript implementations
crates/mk-cli/     — `mk` binary (clap CLI, thin wrapper)
crates/mk-graph/   — `mk-graph` binary (DOT/JSON export, dead-ends, orphans)
```

## Testing

- **mk-core**: 265 unit tests (parse, graph, lex, var, attr, sched, recipe)
- **mk-shell**: 13 tests (sh shell, custom shells)
- **mk-graph**: 4 tests (JSON export, dead-ends, orphans)
- **testdata/**: 43 real-world mkfiles from plan9port, 9legacy, ctSkennerton, mksite

## Gotchas

- Recipe lines are passed verbatim to the shell. `$target`/`$prereq` are injected as env vars, not mk-variable expanded.
- `MKSHELL` splits on whitespace: first token is binary, rest are flags. `MKSHELL=node -e` → `node -e "recipe"`.
- `=` and `:` inside recipe text are NOT split by the lexer (`in_recipe` flag). Recipe lines after TAB are opaque.
- Virtual targets must be explicit (`:V:`) — the graph builder doesn't auto-mark orphan prereqs as virtual.
- Glob expansion (`*.toon`) happens at graph-build time, not at recipe execution time.
