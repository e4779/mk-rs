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

## 1. Project vision

mk-rust is a faithful, high-quality Rust port of `mk` — Andrew Hume's successor to make. Not a clone of GNU Make, not a reimagining with Lua, not a general task runner.

What mk-rust **is**:

- A dependency-driven build tool that reads mkfiles and runs recipes in parallel
- A direct port of Plan 9 mk semantics: pattern-based metarules, transitive closure, attribute system, `$stem`/`$target`/`$prereq` variables
- A library-first crate (`mk-core`) with a thin CLI wrapper (`mk-cli`)
- Fast, safe, portable — leverages Rust's ownership model where C used raw pointers
- 100% compatible with existing mkfiles intended for plan9port mk (sh recipes, duckscript optional via `$MKSHELL`)

What mk-rust is **not**:

- A build system for Cargo/Rust projects (use `cargo` for that)
- A general-purpose task runner (use `just`, `cargo-make`, or shell scripts)
- A Lua/JS/Python-based build system (duckscript may power *recipes* for power users, but the core tool is pure Rust)
- GNU Make compatible — no `.PHONY`, no pattern substitution `$(patsubst ...)`, no `--eval`
- A package manager, a daemon, or a file watcher 

The cat-v.org philosophy applies: mk is a tool for maintaining files. It should be small, composable, and free of accidental complexity. The mkfile is machine-readable documentation of your pipeline. mk-rust honors that.

---

## 2. Architecture overview

### 2.1 Crate structure

```
mk-rust/                     # workspace root
├── Cargo.toml               # workspace: [workspace] members = [...]
├── PLAN.md                  # this file
├── crates/
│   ├── mk-core/             # library: lex + parse + graph + var + sched + shell + attr + archive + include
│   ├── mk-shell/            # Shell trait + sh/duckscript implementations
│   └── mk-cli/              # binary: clap CLI, thin wrapper around mk-core
```

| Crate | Purpose | Dependencies |
|-------|---------|-------------|
| `mk-core` | All build logic. Exposes `build(mkfile_path, opts) -> Result<BuildOutcome>`. No I/O in public API surface — takes a `shell: &dyn Shell` and file system via a `FileSystem` trait (testable). | `regex`, `glob`, `serde` (optional, for AST debugging), `thiserror`, `log` |
| `mk-shell` | `Shell` trait definition (in mk-core), plus `sh::Shell`, `duckscript::Shell` implementations. | `duct` (for sh), `duckscript` + `duckscriptsdk` (optional feature) |
| `mk-cli` | CLI entry point. Argument parsing, loading mkfile, calling `mk-core::build()`, formatting output. | `clap` (derive), `mk-core`, `mk-shell`, `env_logger` |

### 2.2 Key dependencies

| Crate | Used in | Purpose |
|-------|---------|---------|
| `regex` | mk-core (parse, graph) | Compiled regex for `R:` metarules, regex-based stem extraction |
| `clap` (derive) | mk-cli | CLI argument parsing (`-f`, `-n`, `-e`, `-t`, `-a`, `-p`, `-k`, etc.) |
| `thiserror` | mk-core | Structured error types (`MkError`) |
| `duct` | mk-shell (sh, rc) | Process execution with environment passing, stderr capture |
| `glob` | mk-core (graph) | Path globbing for targets/prereqs in rules |
| `serde` + `serde_json` | mk-core (optional) | AST serialization (debugging, future LSP, mkfile formatter) |
| `log` + `env_logger` | mk-cli | Verbose/debug logging |
| `crossbeam` | mk-core (sched) | Parallel job scheduling (channel-based worker pool) |
| `tempfile` | mk-core (recipe) | Temp files for inline recipe scripts |
| `filetime` | mk-core (graph) | File modification time comparison (out-of-date checks) |

### 2.3 Data flow

```
                      ┌──────────────┐
                      │   mkfile(s)   │  (user-authored text)
                      └──────┬───────┘
                             │
                    ┌────────▼────────┐
                    │    lex::Lexer   │  char-by-char → token stream
                    │  (tokenizer)    │  handles: words, colons, =, <, |, newlines, indents, #comments, backticks
                    └────────┬────────┘
                             │  TokenStream (Iterator<Item = Token>)
                    ┌────────▼────────┐
                    │  parse::Parser  │  recursive descent → AST
                    │                 │  Rules, Assignments, Includes, MetaRules
                    └────────┬────────┘
                             │  AST (Vec<Stmt>)
                    ┌────────▼────────┐
                    │   var::Scope    │  expand variables, resolve symbol table
                    │                 │  $VAR, ${VAR}, ${VAR:pat=sub}, namelists
                    └────────┬────────┘
                             │  expanded AST
                    ┌────────▼────────┐
                    │  graph::Builder │  AST → DAG
                    │                 │  apply meta-rules, transitive closure, pruning
                    └────────┬────────┘
                             │  Graph (nodes + arcs)
                    ┌────────▼────────┐
                    │ graph::Checker  │  out-of-date check (mtime comparison)
                    │  (staleness)    │  mark stale nodes, handle virtual targets
                    └────────┬────────┘
                             │  BuildPlan (topologically sorted stale nodes)
                    ┌────────▼────────┐
                    │  sched::Engine  │  parallel DAG traversal
                    │                 │  NPROC worker pool, job queue
                    └────────┬────────┘
                             │  dequeued Job
                    ┌────────▼────────┐
                    │ recipe::Runner  │  feed recipe to shell
                    │                 │  set $target, $prereq, $stem, env vars
                    └────────┬────────┘
                             │  exit code
                    ┌────────▼────────┐
                    │   BuildOutcome  │  success, partial (with -k), or failure
                    └─────────────────┘
```

Pipeline stages are distinct and sequential within a single `build()` call. Each stage produces an owned output consumed by the next stage. No shared mutable state across stages.

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

