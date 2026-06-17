# AGENTS.md — mk-rs

Explore the project: `ls`, `cargo test`, `cat Cargo.toml`. Read README.md for features and overview.

## Architecture decisions (rationale)

> **Transitional location.** These why-not entries close the nyosegawa regression
> gap (agent starting blank must not re-propose rejected alternatives). They
> migrate to `APPEND_SYSTEM.md` as part of #30 (AGENTS slimming) — kept here
> only until that lands.

1. **Arena-based DAG (Vec + indices), not Rc/RefCell.** Contiguous memory, no
   cycles, deterministic drop. `NodeIndex(usize)` and `ArcIndex(usize)` newtypes
   for safety. After build, the graph is immutable.
   → `docs/implementation-comparison.md` §6.2, `docs/mk-spec.md` F-006
   Rejected: `Rc<RefCell<Node>>` — interior mutability, runtime borrow panics,
   pointer chasing, no clear ownership.

2. **Crossbeam/sync threads, not tokio.** Recipe execution is inherently
   blocking (`Command::status()`). No async runtime needed.
   `sync::thread::scope` for NPROC-based worker pool.
   Rejected: tokio — ~20 extra deps, async I/O irrelevant for fork/exec workloads.

3. **sh as default shell, not rc.** Validated by 9base research (also chose sh).
   Duckscript is an optional feature-gated alternative via `MKSHELL=duckscript`
   (or any shell via `MKSHELL=node -e`, `MKSHELL=python3 -c`).
   → `README.md`
   Rejected: rc — not available by default on Linux/macOS. Duckscript as
   *default* — can't run arbitrary binaries (no gcc/python/R), only built-in ops.

4. **Separate mk-graph binary for visualization.** Keeps `mk` lean (no serde
   dep). JSON/DOT export, dead-end detection, recipe text — all in mk-graph.
   → `README.md`
   Rejected: feature-gating serde behind a cargo flag — leaks into the mk-core
   public API surface (Graph types would need conditional serde derives).

5. **No daemon / watch mode (won't-do).** mk is a build tool, not a daemon.
   Plan 9 mk never had this. Compose with `watchexec` / `cargo watch` / a
   shell one-liner (`while inotifywait .; do mk; done`).
   Rejected: in-tree `mk --watch` — bloats mk-core (signal handling in
   threads, file-watching dep) for ergonomics that external tools cover.

## Gotchas

- Recipe lines are passed verbatim to the shell. `$target`/`$prereq` are
  injected as env vars, not mk-variable expanded.
- `MKSHELL` splits on whitespace: first token is binary, rest are flags.
  `MKSHELL=node -e` → `node -e "recipe"`.
- `=` and `:` inside recipe text are NOT split by the lexer (`in_recipe` flag).
  Recipe lines after TAB are opaque.
- Virtual targets must be explicit (`:V:`) — the graph builder doesn't auto-mark
  orphan prereqs as virtual.
- `:V:` (and `:Q:`, `:N:`, etc.) are RULE ATTRIBUTES, not part of the target
  name. When referencing a virtual target as a prereq, use just the name:
  `run: build` (not `run: build:V:`). The parser greedily interprets `Word:`
  after a target header as attributes — `build:V:` as a prereq gets parsed as
  target `build` with bogus attribute chars `V`.
- Glob expansion (`*.toon`) happens at graph-build time, not at recipe
  execution time.
- `cargo publish` creates a tarball without `.git`. build.rs reads `GIT_HASH`
  file first, falls back to `git rev-parse`. CI creates `crates/mk-cli/GIT_HASH`
  before publish.
