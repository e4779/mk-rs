# mk-rust: Plan

> *"The Unix philosophy: Write programs that do one thing and do it well."* — Doug McIlroy
>
> *"Mk is an efficient general tool for describing and maintaining dependencies between files or programs."* — Andrew Hume

---

## Current focus

PLAN.md restructuring — slimming from 1380 → ~130 lines. v0.2.2 shipped
(F-045, F-063, Bug 4); infrastructure landed (hooks, coverage ratchet,
git-cliff, conventional commits). Next: finish docs, merge Bug A/B
(`bugfix/pattern-rule-stem-extraction`), then `-s` flag resolution.

---

## Architecture

Pipeline (mkfile → lex → parse+expand → graph → sched → recipe → BuildOutcome),
crate roster, and per-module design notes live in **`cargo doc`** — see the
crate-level `//!` in `crates/mk-core/src/lib.rs`, rendered at
<https://docs.rs/mk-rs-core>. Derivable from code; not duplicated here.

---

## Constraints

What must hold true. What must never happen.

- **No unsafe code.** `#![forbid(unsafe_code)]` in mk-core and mk-shell
  `lib.rs` — compile-time enforced, cannot be `#[allow]`'d.
- **No daemon / watch mode.** mk is a build tool. Compose with `watchexec` /
  `cargo watch` / shell one-liner. (Won't-do — see AGENTS.md decision 5.)
- **Library-first.** `mk-core` exposes `build(mkfile_path, opts)`. The CLI is
  a thin wrapper, not the primary interface.
- **Plan 9 mk compatibility.** mkfiles intended for plan9port mk must work
  unchanged (sh recipes). GNU Make compatibility is explicitly not a goal.

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

