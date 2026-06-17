# mk-rs

> *"The Unix philosophy: Write programs that do one thing and do it well."* — Doug McIlroy
>
> *"Mk is an efficient general tool for describing and maintaining dependencies between files or programs."* — Andrew Hume

A faithful Rust port of `mk` — Andrew Hume's successor to make. Pattern-based
metarules, transitive closure, parallel execution, no built-in magic.

## What mk-rust is (and is not)

**mk-rust is:**

- A dependency-driven build tool that reads mkfiles and runs recipes in parallel
- A direct port of Plan 9 mk semantics: pattern-based metarules, transitive closure, attribute system, `$stem`/`$target`/`$prereq` variables
- A library-first crate (`mk-core`) with a thin CLI wrapper (`mk-cli`)
- Fast, safe, portable — zero `unsafe`, leverages Rust's ownership model where C used raw pointers
- 100% compatible with existing mkfiles intended for plan9port mk (sh recipes, duckscript optional via `$MKSHELL`)

**mk-rust is not:**

- A build system for Cargo/Rust projects (use `cargo` for that)
- A general-purpose task runner (use `just`, `cargo-make`, or shell scripts)
- A Lua/JS/Python-based build system (duckscript may power *recipes* for power users, but the core tool is pure Rust)
- GNU Make compatible — no `.PHONY`, no `$(patsubst ...)`, no `--eval`
- A package manager, a daemon, or a file watcher

The cat-v.org philosophy: mk is a tool for maintaining files. Small, composable,
free of accidental complexity. The mkfile is machine-readable documentation of
your pipeline.

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
git clone https://github.com/e4779/mk-rs.git
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
use mk_core::parse::{parse, parse_with_scope};
use mk_core::graph::build_graph;
use mk_core::sched::{execute, ResolvedRule, SchedOptions};
use mk_core::var::{builtin_scope, import_env, Precedence};
use mk_shell::ShShell;

fn main() -> anyhow::Result<()> {
    let mkfile = std::fs::read_to_string("mkfile")?;
    let tokens = tokenize(&mkfile, ShellMode::Sh)?;

    // parse() is a convenience wrapper that builds a fresh scope
    // (builtins + env). For pre-seeded scopes (e.g., CLI vars),
    // use parse_with_scope:
    //   let mut scope = builtin_scope();
    //   import_env(&mut scope);
    //   scope.set_raw("VAR", "value", Precedence::CommandLine);
    //   let stmts = parse_with_scope(&tokens, &mut scope)?;
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
# Requirements: Rust 1.92+ (the `mk-graph` crate depends on `ascii-dag`)
git clone https://github.com/e4779/mk-rs.git
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
# Default: terminal-friendly ASCII art (auto-laid-out via Sugiyama)
mk-graph

# Mermaid block — renders inline on GitHub/GitLab/Obsidian, LLM-friendly
mk-graph --format mermaid

# Graphviz DOT — pipe to `dot -Tsvg > graph.svg`
mk-graph --dot

# JSON — programmatic inspection
mk-graph --json

# Subgraph reachable from a specific target
mk-graph --target data/processed/result.json

# Check for dead-end targets and orphan prerequisites
mk-graph --check
```

Formats are also selectable via `-F ascii|mermaid|dot|json`; `--ascii` is the
default. Edge labels show `meta` for metarule-derived arcs or the mkfile line
number (`L9`) where the rule is defined.

## License

MIT OR Apache-2.0
