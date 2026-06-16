# mk-rs

> A faithful Rust port of `mk` — the dependency-driven build tool by Andrew Hume.
> *"Mk: a successor to make" (1987)*

Pattern-based metarules. Transitive closure. Parallel execution. No built-in magic.

## What is mk?

**mk** is a dependency-driven build tool originally created by Andrew Hume at Bell Labs in the late 1980s for Plan 9. It reads a *mkfile* (a declarative file describing targets, prerequisites, and recipes) and executes shell commands to bring targets up to date.

Unlike GNU Make, mk builds the **entire dependency graph before executing any recipe**, supports **pattern-based metarules with transitive closure** (`%`, `&`, `R:` regex), and runs recipes **in parallel** (controlled by `$NPROC`).

mk-rust ports these semantics to Rust with zero unsafe code, a library-first architecture, and full test coverage.

## Quick start

### Install

```bash
# Install from crates.io
cargo install mk-rs

# Optional: with duckscript support
cargo install mk-rs --features duckscript
```

Or build from source:
```bash
git clone https://github.com/your-org/mk-rust.git
cd mk-rust
cargo build --release
cargo install --path crates/mk-cli
```

### Write a mkfile

Create a file called `mkfile`:

```mkfile
# Build a C program
CC = cc
CFLAGS = -Wall -O2

prog: main.o util.o
	$CC $CFLAGS -o $target $prereq

%.o: %.c
	$CC $CFLAGS -c $stem.c

all:V: prog

# Note: :V: (and :Q:, :N:, etc.) are rule attributes, not name suffixes.
# When referencing a virtual target as a prereq, use just the name:
run: all        # ✅ correct
run: all:V:     # ❌ parser sees "all" as bogus attributes, not a prereq
```

### Run mk

```bash
mk           # build the first target (prog)
mk all       # build the "all" virtual target
mk -n        # dry-run: print what would be done
mk -e        # explain why targets are being rebuilt
mk -j 4      # build with 4 parallel jobs (set $NPROC)
```

### Use as a library

```toml
[dependencies]
mk-core = "0.1"
```

```rust
use mk_core::lex::{tokenize, ShellMode};
use mk_core::parse::parse;
use mk_core::graph::build_graph;
use mk_core::sched::{execute, ResolvedRule, SchedOptions};
use mk_shell::ShShell;

fn main() -> anyhow::Result<()> {
    let mkfile = std::fs::read_to_string("mkfile")?;
    let tokens = tokenize(&mkfile, ShellMode::Sh)?;
    let stmts = parse(&tokens)?;
    let mut graph = build_graph(&stmts, &["prog".into()])?;
    let rules = /* build rules map from stmts */;
    let outcome = execute(
        &mut graph, &rules, &ShShell,
        &std::env::current_dir()?,
        &std::collections::HashMap::new(),
        &SchedOptions::default(),
    )?;
    println!("Built: {:?}", outcome.built);
    Ok(())
}
```

## Features

- **Plan 9 mk semantics** — faithful port of mk's parser, DAG builder, and scheduler
- **Pattern metarules** — `%` (greedy), `&` (single-component), and `R:` regex metarules with transitive closure
- **Variable system** — `${VAR}`, `$VAR`, `$$`, recursive expansion, namelist transforms (`${SRC:%.c=%.o}`), backtick command substitution
- **9 rule attributes** — `V` (virtual), `Q` (quiet), `N` (no-exec), `U` (unconditional), `D` (delete on error), `E` (exclusive), `P` (custom comparison), `R` (regex metarule), `n` (no-virtual)
- **Parallel execution** — `$NPROC` controls job count; dependency-aware scheduling
- **Include system** — `< mkfile` and `<| command` with circular include detection
- **Archive member syntax** — `lib.a(member.o)` auto-generates member dependencies
- **CLI flags** — `-n` (dry-run), `-e` (explain), `-t` (touch), `-a` (all), `-k` (keep going), `-s` (silent), `-i` (force intermediates), `-f` (mkfile)
- **Library-first** — `mk-core` crate usable without the CLI
- **Shell abstraction** — trait-based, supports `/bin/sh` (production) and `duckscript` (optional feature)
- **Cross-platform** — works on Linux, macOS, and Windows (with sh available)

## Comparison

### vs GNU Make

| Feature | GNU Make | mk-rust |
|---------|----------|---------|
| Metarules | `%` patterns, `.SECONDEXPANSION` for complex cases | `%`, `&`, `R:` regex — all first-class |
| Graph construction | Incremental during execution | Whole DAG built first |
| Parallelism | `-j N` flag | `$NPROC` variable |
| Variables | Recursively expanded, `:=` for immediate | Expanded at parse time + recipe time |
| Phony targets | `.PHONY:` | `V` attribute on any target |
| Include | `include` directive | `< mkfile` and `<| command` |
| Functions | `$(shell ...)`, `$(wildcard ...)`, 20+ builtins | Backtick substitution, `$stem`, `$newprereq` |
| Error handling | `.DELETE_ON_ERROR`, `.IGNORE` | `D` attribute (delete on error), `-k` flag |
| Build-in-progress | `--jobs`, `-O` | Exclusively via `$NPROC` |
| LOC (core) | ~40,000 (C) | ~3,500 (Rust) |

### vs plan9port mk (original C)

| Feature | plan9port mk | mk-rust |
|---------|-------------|---------|
| Language | Plan 9 dialect C (~4,350 LOC) | Rust (~3,500 LOC) |
| Safety | Manual memory management, raw pointers | Zero `unsafe`, ownership model |
| Portability | Requires plan9port libc layer | Cross-compiles via cargo |
| Shell support | rc (native), sh (via /bin/sh) | sh (primary), rc (planned), duckscript (optional) |
| Tests | No test suite | 266 tests, TDD |
| Library API | None (monolithic binary) | `mk-core` crate with public API |
| Compatibility | Reference implementation | Aims for 100% mkfile compatibility |

### vs other mk ports

| Tool | Language | Status | Notes |
|------|----------|--------|-------|
| dcjones/mk | Go | Abandoned (2015) | Partial implementation |
| ctSkennerton/mk | Go | Abandoned | 381 commits, most complete Go port |
| Knit | Go | Abandoned (2024) | Lua VM embedded, conflates build + scripting |
| mk-rust | Rust | Active | Pure Rust, library-first, 266 tests |

## Building from source

```bash
# Requirements: Rust 1.80+
git clone https://github.com/your-org/mk-rust.git
cd mk-rust

# Build (debug)
cargo build

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Build release binary
cargo build --release
# Binary at: target/release/mk
```

## Architecture

```
crates/
├── mk-core/     # Core library: lexer, parser, DAG, scheduler, variables
├── mk-shell/    # Shell implementations: ShShell, DuckShell (optional)
└── mk-cli/      # CLI binary: argument parsing, glue code

mk-core modules:
  lex      — Character-by-character tokenizer (comments, quoting, backtick, line continuation)
  attr     — Rule attribute bitflags (V, Q, N, U, D, E, P, R, n)
  error    — Unified error type (thiserror), no panics in library code
  parse    — Token→AST parser (rules, assignments, includes, attributes)
  var      — Variable scope with precedence, recursive expansion, namelist transforms
  graph    — DAG builder, metarule matching, cycle detection, staleness checker
  shell    — Shell trait abstraction (execute, quote, find_unescaped)
  recipe   — Recipe execution glue (elision, printing, attribute handling)
  sched    — Build scheduler: topological sort, serial + parallel execution
  include  — Recursive mkfile includes with circular dependency detection
  archive  — lib.a(member.o) syntax parsing
```

### Pipeline

```
mkfile text → lex (tokens) → parse (AST) → graph (DAG) → sched (execute) → done
                                         ↕
                                      var scope
                                   (expansion)
```

## Project status

| Phase | Features | Status |
|-------|----------|--------|
| 1a | Parser, DAG, serial exec, core variables, attributes | ✅ Complete |
| 1b | Includes, recipe-time vars, missing intermediates, CLI flags | ✅ Complete |
| 2 | %/&/R metarules, NPROC parallel, namelists, pruning | ✅ Complete |
| 3 | Aggregates, P attribute, debug flags, duckscript, polish, graph export | ✅ Complete |

See [`TRACEABILITY.md`](TRACEABILITY.md) for the full feature matrix mapped to mk spec features.

See [`PLAN.md`](PLAN.md) for the project vision, design decisions, and future plans.

## Dependency graph

The `mk-graph` companion binary (installed alongside `mk`) visualizes the
dependency graph. It keeps `mk` itself focused on building.

```bash
# Show entire graph in DOT format
mk-graph

# Show subgraph reachable from a specific target
mk-graph --target data/processed/result.json

# Render to SVG
mk-graph | dot -Tsvg > graph.svg

# JSON export (for programmatic inspection)
mk-graph --json

# Check for dead-end targets and orphan prerequisites
mk-graph --check
```

Edge labels show the mkfile line number where the rule is defined.

## License

MIT OR Apache-2.0
