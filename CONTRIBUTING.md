# Contributing to mk-rust

mk-rust is a faithful Rust port of Plan 9 `mk`. Contributions welcome — bug
reports, fixes, ports of missing plan9port behavior, test cases from real
mkfiles.

## Setup (one minute)

```bash
git clone <repo> && cd mk-rust
cargo build                                   # verify it compiles
cargo test --workspace                        # verify 342 tests pass
git config core.hooksPath .githooks           # install fast gates (see below)
```

Rust 1.92+ required (the `mk-graph` crate depends on `ascii-dag`).

## Tiered gates

Quality gates are split by cost — fast checks run on every commit, slow
checks run in CI. This keeps the commit loop snappy without sacrificing
coverage.

| Tier | Where | What | Time |
|------|-------|------|------|
| **Fast** | `.githooks/pre-commit` | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` | ~10-30s |
| **Coverage ratchet** | CI (planned, see roadmap) | `cargo llvm-cov --fail-under-lines N` against a baseline | minutes |

The fast hook skips automatically when no `.rs`/`.toml`/`Cargo.lock` files
are staged (docs-only commits go through instantly).

### Installing the hooks

```bash
git config core.hooksPath .githooks
```

This must be done once per clone. The hooks are tracked in `.githooks/`
(not `.git/hooks/`, which git ignores) so they survive across machines
and contributors.

### Bypassing (emergencies only)

```bash
git commit --no-verify ...
```

Don't make this a habit — the gates exist because regressions are harder
to clean up than to prevent. If a gate is wrong, fix the gate.

### Verbose mode

```bash
CARGO_HOOKS_VERBOSE=1 git commit ...
```

Shows full cargo output (useful when a gate fails and the summary is too terse).

## Commit conventions

We use **Conventional Commits** — `type(scope): subject`. Examples from
this repo's history:

```
fix(core): virtual target (:V:) is always stale (Bug 4)
docs(plan): delete §3 Module design — migrated to cargo doc
release: v0.2.2 — Bug 4 + F-063
feat(lex): rc-style backtick {cmd} lexer support
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `build`, `ci`,
`perf`, `style`, `release`.

This convention feeds `git-cliff` to auto-generate `CHANGELOG.md` (planned;
see roadmap). Keep commit subjects in the imperative mood ("add", not
"added").

## Architecture orientation

- `crates/mk-core/` — all build logic (lex, parse, graph, var, sched, recipe)
- `crates/mk-shell/` — Shell trait impls (sh, duckscript behind feature)
- `crates/mk-cli/` — thin CLI wrapper
- `crates/mk-graph/` — visualization/diagnosis tool (JSON/DOT export)

Run `cargo doc --no-deps --workspace --open` for full API docs. Module-level
`//!` comments carry the architecture invariants (arena DAG, worker pool,
variable expansion semantics).

For the spec driving each feature, see `docs/mk-spec.md` (F-001 … F-070)
and `TRACEABILITY.md` (feature → module → status matrix).

## Filing changes

- Bug or behavior divergence from plan9port `mk` → open an issue with a
  minimal mkfile reproducer and the reference `/usr/local/plan9/bin/mk`
  output.
- New feature → check `docs/mk-spec.md` first; if it's a Plan 9 mk feature
  not yet implemented, the F-xxx number lives there.
- Keep PRs focused — one feature or one bug per PR makes review tractable.
