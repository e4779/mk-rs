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

**Types** (one of):

| Type | When |
|------|------|
| `feat` | New feature (user-facing) |
| `fix` | Bug fix (user-facing) |
| `docs` | Documentation only (PLAN/README/docs/*/wiki) |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf` | Performance improvement |
| `test` | Adding or correcting tests |
| `testdata` | Corpus / fixture additions |
| `build` | Build system, dependencies, cliff.toml |
| `ci` | CI configuration |
| `chore` | Misc repo tooling (hooks, scripts) |
| `style` | Formatting, whitespace (cargo fmt hygiene) |
| `release` | Version bumps and release commits |

**Scope** (optional, lowercase, no spaces) — the affected area:

| Scope | Covers |
|-------|-------|
| `core` | `crates/mk-core/` (lex, parse, graph, var, sched, recipe, attr, include) |
| `shell` | `crates/mk-shell/` (Shell impls) |
| `cli` | `crates/mk-cli/` (binary, flags) |
| `graph` | `crates/mk-graph/` (visualization) |
| `plan` | PLAN.md strategy |  
| `agents` | AGENTS.md / APPEND_SYSTEM |  
| `infra` | git hooks, CI, build tooling |  
| (none) | cross-cutting | 

**Breaking changes:** add `!` after type/scope AND a `BREAKING CHANGE:` footer.

```
feat(core)!: change default shell from sh to rc

BREAKING CHANGE: mkfiles with sh syntax now need `MKSHELL=sh`.
```

This convention feeds `git-cliff` to auto-generate `CHANGELOG.md`. Keep
commit subjects in the imperative mood ("add", not "added"). Subjects
should read as a sentence completion: "If applied, this commit will _____".

### Generating CHANGELOG.md

`CHANGELOG.md` is **auto-generated** — never hand-edit.

```bash
# During release:
git-cliff -o CHANGELOG.md           # regenerate from history
git add CHANGELOG.md && git commit -m "release: v0.X.Y"
git tag v0.X.Y
```

`cliff.toml` at repo root configures section grouping, scopes, and GitHub
link generation.

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
