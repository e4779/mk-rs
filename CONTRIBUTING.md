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

Quality gates are split by cost — fast checks run on every commit,
slow checks run on every push, and all of them mirror in CI for safety.
This keeps the commit loop snappy without sacrificing coverage, and
means a contributor without local hooks still gets caught.

| Tier | Where | What | Time |
|------|-------|------|------|
| **Fast** | `.githooks/pre-commit` | `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` | ~5-10s |
| **Coverage ratchet** | `.githooks/pre-push` | `cargo llvm-cov --workspace`, compared against `.coverage-baseline` (auto-ratchets up, blocks on >0.10% regression) | ~15s |
| **CI mirror** | `.gitverse/workflows/ci.yml` | Same fmt + clippy + test + coverage-ratchet, on every push/PR/tag. Slower than local but runs even when hooks aren't installed. | ~1-3min per job |

Local hooks skip automatically when no `.rs`/`.toml`/`Cargo.lock` files
are staged/being pushed (docs-only changes go through instantly). The
CI mirror does *not* skip — it is the safety net for changes that
slipped past the local layer (e.g. contributor without hooks, or an
agent in a fresh worktree).

Baseline: **90.17% line coverage** as of v0.2.3. Stored in
`.coverage-baseline` at repo root. The local pre-push ratchet bumps it
automatically when coverage improves; commit the bumped file separately
(hint printed by the hook).

### Installing the hooks

```bash
git config core.hooksPath .githooks
```

This must be done once per clone. The hooks are tracked in `.githooks/`
(not `.git/hooks/`, which git ignores) so they survive across machines
and contributors.

### Required tools

```bash
cargo install cargo-llvm-cov   # for the pre-push coverage gate
```

`cargo-llvm-cov` requires `llvm-tools-preview` rustup component — it
will prompt to install on first run.

### Bypassing (emergencies only)

```bash
git commit --no-verify ...      # skip pre-commit
git push --no-verify ...        # skip pre-push
COVERAGE_SKIP=1 git push ...    # skip just the coverage gate
```

Don't make this a habit — the gates exist because regressions are harder
to clean up than to prevent. If a gate is wrong, fix the gate.

### Working with the coverage ratchet

The pre-push hook compares total line coverage against `.coverage-baseline`.

- **Coverage within ±0.10% of baseline** → push proceeds, baseline unchanged.
- **Coverage improved by >0.10%** → push proceeds, baseline auto-bumped in
  `.coverage-baseline`. Commit it: `git add .coverage-baseline`.
- **Coverage dropped by >0.10%** → push blocked. Either add tests, or
  (if the drop is intentional, e.g. removed dead code) update baseline:
  `echo 89.40 > .coverage-baseline && git add .coverage-baseline`.

The 0.10% tolerance absorbs non-deterministic test jitter (~0.04%
observed). Without it the ratchet would flap on every push.

### Verbose mode

```bash
CARGO_HOOKS_VERBOSE=1 git commit ...
```

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

`CHANGELOG.md` is **auto-generated** — never hand-edit. The release procedure
is code-ified in `scripts/release.sh`:

```bash
./scripts/release.sh patch            # 0.2.X → 0.2.(X+1)
./scripts/release.sh patch --dry-run   # show what would happen, no changes
# minor / major analogously
```

The script runs pre-checks (clean tree, in sync with origin, gates pass),
bumps version in all 4 workspace Cargo.toml files, regenerates CHANGELOG via
`git-cliff` (using `cliff.toml`), pauses for a manual `Current focus` update
in PLAN.md / TODO.md, then commits, tags, and pushes both remotes (origin =
gitverse canonical with publish CI; github = mirror). `cargo publish` itself
is NOT run locally — CI auto-publishes on `v*` tag push, see
`.gitverse/workflows/ci.yml`. Do not bypass the pre-checks with `--no-verify`
on release commits; if gates fail, fix them first.

### Binary releases

CI also builds a stripped release binary and uploads it to **both** remotes:

- **gitverse releases** (canonical) — via the Public API
  (`POST /repos/{owner}/{repo}/releases/{id}/assets`, multipart).
- **GitHub releases** (mirror) — `cargo-binstall` target, because binstall
  needs a stable URL template and gitverse's per-asset download URL embeds
  an opaque `asset_id`. GitHub's `/releases/download/<tag>/<file>` shortcut
  is stable, so `[package.metadata.binstall].pkg-url` points at the mirror.

Today the matrix is Linux x86_64 only (covers the zola CI use case).
PLAN §Next 3 has mk-rust as Unix-first; extend the matrix in
`.gitverse/workflows/ci.yml` when macOS/Windows runners are available.

**Required gitverse CI secrets** (configure once per repo under
Settings → Secrets and variables):

| Secret | Purpose |
|---|---|
| `CARGO_REGISTRY_TOKEN` | crates.io publish (existing) |
| `GV_API_TOKEN` | create release + upload asset via `api.gitverse.ru` |
| `GH_RELEASE_TOKEN` | mirror asset to GitHub release (PAT with `repo` scope) |

Verify after a release: `cargo binstall -y mk-rs && mk --version`.

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
