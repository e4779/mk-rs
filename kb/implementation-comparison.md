# What We Took From Each mk Implementation

> Documenting design decisions: sources of inspiration, conscious omissions.

## Implementation Landscape

| Implementation | Language | LOC | Shell | Status | Our usage |
|---------------|----------|-----|-------|--------|-----------|
| **plan9port mk** (Russ Cox) | C | ~4,350 | rc/default, sh opt | Active | Primary reference |
| **9base mk** (suckless) | C | ~3,000 | **sh** default | Abandoned (2019) | Validation |
| **dcjones/mk** | Go | ~2,285 | sh | Abandoned (2015) | Lexer pattern |
| **ctSkennerton/mk** | Go | ~3,273 | sh+rc, HTTP/S3 | Abandoned (2020) | Remote files idea |
| **zyedidia/knit** | Go+Lua | ~5,170 | Lua VM | Active (2023) | Hash-based staleness |
| **Vita Nuova/Quick C--** | C | — | **sh** (rc variant) | Dead (2016) | sh validation |
| **cargo-make** | Rust | — | duckscript | Active | Embedding pattern |
| **mk-rs** (ours) | Rust | ~4,500 | sh, duckscript opt | Active | — |

## Feature by feature: where it came from

### Core mkfile syntax → plan9port, Hume87
- `target: prereqs` + indented recipe
- `$VAR`, `${VAR}`, `${VAR:A%B=C%D}` namelists
- `%`, `&`, `R:` metarules
- `< file`, `<| command` includes
- Attributes: `V`, `Q`, `N`, `U`, `D`, `E`, `P`, `R`, `n`
- **All syntax directly from plan9port mk man page and Hume87 paper.**

### Variables-as-environment → plan9port
- mk doesn't substitute variables in recipes — exports them to shell env
- `$target`, `$prereq`, `$stem`, `$newprereq`, `$alltarget`, `$pid`, `$nproc`
- **This is the defining difference between mk and make.** Verified by HN discussion.

### sh as default shell → 9base, Vita Nuova
- Both 9base and Quick C-- chose sh over rc for Unix portability
- We followed the same path, independently
- rc support: intentionally omitted (dead ecosystem)

### duckscript as optional → cargo-make
- 3-call embedding: `Context::new()` → `load_sdk()` → `run_script()`
- Feature-gated, doesn't increase core size
- **Not in any mk port — our innovation.**

### Parallel execution → plan9port, ctSkennerton
- `$NPROC` controls parallelism
- plan9port: `fork()`+`waitpid()`, events[] array
- Ours: `std::thread::scope` + ready-queue
- **Same model, Rust-native implementation.**

### DAG model → plan9port
- Build full graph before executing any recipe
- Transitive closure on metarules
- Cycle detection and rejection
- **Direct port of plan9port semantics, Rust-native data structures (Vec+indices).**

### Hash-based staleness (planned) → knit
- Knit defaults to content hashing (not mtime)
- We'll add as optional `--hash` flag
- **Adopted from knit, not plan9port.**

### Archive aggregates → plan9port
- `lib(member)` syntax for `ar` archives
- Auto-rule generation
- **Plan 9 specific, implemented for compatibility.**

### Shell abstraction → plan9port, 9base
- Both have `Shell` struct with function pointers
- We translated to Rust `trait Shell`
- **Pattern validated by two independent C implementations.**

## What we consciously did NOT take

| Feature | Source | Why omitted |
|---------|--------|-------------|
| rc shell | plan9port, 9base | Dead ecosystem. duckscript replaces it. |
| Plan 9 signals (notes) | plan9port | Unix-only target. |
| `$O`/`$objtype` multi-arch | plan9port | Plan 9 specific. |
| `membername` utility | plan9port | External rc script, not part of mk. |
| Watcher/daemon mode | knit | Separate concern. Use `watchexec`. |
| Lua VM | knit | Excessive for a build tool. duckscript is lighter. |
| Plugin system | cargo-make | Over-engineered. Recipes are plugins. |
| Task inheritance | cargo-make | Unnecessary complexity. |
| Profiles | cargo-make | mk is simpler than that. |
| HTTP/S3 remote files | ctSkennerton | Interesting but not core. Maybe later. |
| Dynamic stdout-as-mkfile | Hume87 | Never used in practice. |
| Cyclic dependencies | GNU Make | mk explicitly rejects cycles. |

## What we added that's new

| Feature | Why |
|---------|-----|
| duckscript Shell | Alternative to rc. In-process, cross-platform. |
| `$MKFLAGS` / `$MKARGS` | Useful for recipes. Not in original mk. |
| `--color` flag | Modern UX. Not in any mk port. |
| `envmnt`-compatible expansion | Cargo-make validated approach. |
| 266 tests | No mk port has tests. |
| Man page | plan9port has one; ours is updated for mk-rs. |

## UX lesson from invest-research session

The agent struggled because `mk: build aborted due to errors` didn't show **which command failed** or **what the error was** (e.g., shell escaping issue with `$$secid`).

**Fix applied:** `RecipeError::CommandFailed` now includes stderr. Error messages are: `recipe command failed with exit code 1: /bin/sh: line 1: ...`

## Verdict

mk-rs is a **synthesis**: syntax and semantics from plan9port, shell choice validated by 9base, embedding pattern from cargo-make, test coverage from no one (we invented it). The result is smaller, safer, and better documented than any prior mk port.
