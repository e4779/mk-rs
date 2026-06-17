# mk-rust: Plan

---

## Current focus

**v0.2.3 shipped** — Bug A/B (pattern-rule stem, & metarule resolution),
placeholder URLs fixed, `forbid(unsafe_code)` in all 4 crates,
`scripts/release.sh` code-ified release procedure, pre-push coverage
ratchet bug fixed (stdin field parsing). Docs + infra waves complete
(PLAN 1380 → 94 lines, AGENTS ultra-thin 47 lines, gotchas migrated,
conventional commits + git-cliff CHANGELOG, pre-commit/pre-push hooks).
Next: P2 hygiene (review-*.md cleanup, epigraph dedup), skeptic audit
on PLAN/AGENTS/gotchas, then F-003a quoting if it becomes blocking.

---

## Architecture

Pipeline (mkfile → lex → parse+expand → graph → sched → recipe → BuildOutcome),
crate roster, and per-module design notes live in **`cargo doc`** — see the
crate-level `//!` in `crates/mk-core/src/lib.rs`, rendered at
<https://docs.rs/mk-rs-core>. Derivable from code; not duplicated here.

---

## Constraints

What must hold true. What must never happen.

- **No unsafe code.** `#![forbid(unsafe_code)]` in all four crates
  (mk-core, mk-shell lib.rs; mk-cli, mk-graph main.rs) — compile-time
  enforced, cannot be `#[allow]`'d.
- **No daemon / watch mode.** Compose with external tools
  (`watchexec`, `cargo watch`, shell one-liner). Why-not: Decisions §5.
- **Library-first.** `mk-core` exposes `build(mkfile_path, opts)`. The CLI is
  a thin wrapper, not the primary interface.
- **Plan 9 mk compatibility.** mkfiles intended for plan9port mk must work
  unchanged (sh recipes). GNU Make compatibility is explicitly not a goal.

## Decisions

Architectural choices with rejected alternatives. An agent starting blank must
read these before proposing architectural changes — re-proposing a rejected
option is the most common form of regression. Format: `Decision — Rejected:
alternative, reason`.

1. **Arena-based DAG (Vec + indices).** `NodeIndex(usize)` and `ArcIndex(usize)`
   newtypes; graph is immutable after build.
   Rejected: `Rc<RefCell<Node>>` — interior mutability, runtime borrow panics,
   pointer chasing, no clear ownership.

2. **Crossbeam/sync threads.** Recipe execution is inherently blocking
   (`Command::status()`). `sync::thread::scope` for NPROC-based worker pool.
   Rejected: tokio — ~20 extra deps, async I/O irrelevant for fork/exec.

3. **sh as default shell.** Validated by 9base (also chose sh). Duckscript
   optional via `MKSHELL=duckscript`; any shell via `MKSHELL=node -e`.
   Rejected: rc — not available by default on Linux/macOS. Duckscript as
   *default* — can't run arbitrary binaries (no gcc/python/R), only built-ins.

4. **Separate mk-graph binary for visualization.** Keeps `mk` lean (no serde
   dep). JSON/DOT export, dead-end detection, recipe text — all in mk-graph.
   Rejected: feature-gating serde behind a cargo flag — leaks into the mk-core
   public API surface (Graph types would need conditional serde derives).

5. **No daemon / watch mode.** mk is a build tool, not a daemon. Compose with
   `watchexec` / `cargo watch` / shell one-liner.
   Rejected: in-tree `mk --watch` — bloats mk-core (signal handling in threads,
   file-watching dep) for ergonomics that external tools cover.

---

## Next milestones

Ordered. Concrete, not a wishlist — what we will actually do next.

1. **`-s` flag resolution** (plan9port compat). plan9port `-s` = sequential
   (force `NPROC=1`); mk-rust `-s` = silent. Rename silent → `-q`, reserve
   `-s` for sequential. `Q` attribute already handles per-rule silence.
   Tracked as a TODO epic.

2. **Content-hash staleness** (optional `--hash` flag). mtime is the default
   (matches plan9port + GNU Make); blake3 content hashing as an opt-in for
   accurate rebuilds (touch-immune, recipe-change detection via `.mk.state`).
   `P:` attribute already supports custom comparison programs. Demand-driven.

3. **Windows support** (long-term). Unix-first today (Linux + macOS). The
   `Shell` trait + `std::process::Command` provide a path; main hurdles: no
   `/bin/sh` by default (needs `cmd.exe` shell), path separators, no `fork()`
   for NPROC. Phase 4 if there is demand.

---

