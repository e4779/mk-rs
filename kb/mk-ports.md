# Plan 9 `mk` Ports & Alternatives Survey

## Overview

Three GitHub projects related to Plan 9 `mk`:

| Project | Stars | Language | Last Commit | Status |
|---------|-------|----------|-------------|--------|
| [dcjones/mk](https://github.com/dcjones/mk) | ★182 | Go | 2015-03-25 | **Original port — abandoned** |
| [henesy/mk](https://github.com/henesy/mk) | ★4 | Go | 2020-02-07 | Fork of dcjones — 1 commit, abandoned |
| [ctSkennerton/mk](https://github.com/ctSkennerton/mk) | ★5 | Go | 2020-12-21 | Fork of dcjones — **most patched mk port** |
| [zyedidia/knit](https://github.com/zyedidia/knit) | ★192 | Go (+ Lua) | 2023-09-01 | **Active, Lua-based reimagining** |

---

## Detailed Analysis

### 1. henesy/mk — "make remade"

- **Fork of**: dcjones/mk (the original Go port)
- **Total commits**: 1 (just `ignore mkfile`)
- **Last activity**: Feb 2020 — **abandoned**
- **Lines of Go**: 2,375 across 7 files
- **No go.mod** — pre-modules Go project
- **No tests** at all
- **Recipe language**: Shell (`sh -c`) with non-shell `S[interpreter]` attribute
- **Recipe format**: Any indentation (not just tabs), blank lines allowed
- **Parallelism**: `sync.Cond`-based job pool, default **4 workers**, `-p=N` flag
- **Exclusive subprocess**: `reserveExclusiveSubproc()` — blocks all workers for a single recipe (for resource-hungry rules)
- **Out-of-date check**: Timestamp-only (`u.t.Before(prereqs[i].t)`)
- **Notable features**: Colors via `-C`, interactive mode `-i`
- **Missing features**: No go.mod, no man page, no S3/remote support, no tests, no `${foo}` expansion
- **Pros**: Minimal, faithful to Plan 9 mk semantics, simple codebase
- **Cons**: Single commit fork — effectively unmaintained, no go.mod, no tests, no community

### 2. ctSkennerton/mk — Fix-heavy fork

- **Fork of**: dcjones/mk, via merging contributions from multiple authors
- **Total commits**: Multiple (includes patches from DiablosOffens, galexite, rjkroege, henesy, pauldgrandis)
- **Last activity**: Dec 2020 — **abandoned**
- **Lines of Go**: ~3,273 (code) + ~1,000 (tests)
- **go.mod**: Yes, Go 1.13, with `aws-sdk-go`, `isatty`, etc.
- **Has tests**: 3 test files (expand, parse, rules, mk)
- **Recipe language**: Shell (`sh -c`) + `S[interpreter]` non-shell attribute
- **Recipe format**: Same as henesy — any indentation, blank lines in recipes
- **Parallelism**: Same `sync.Cond` pool, default = `runtime.NumCPU()` (not hardcoded 4)
- **Unique features**:
  - **Remote files** — `"s3://..."` and `"https://..."` as prerequisites, with Last-Modified comparison
  - **`-l` flag** — recursion limit on rule application
  - **`-C directory`** — chdir before building
  - **`-color` flag** — on/off/auto (uses isatty)
  - **Environment variable passthrough** — parses `os.Environ()` into mk variables
  - **Man page** — `mk.1.md`
- **Missing**: No `${foo}` expansion (same TODO as dcjones), no `$newprereq`/`$alltargets`
- **Pros**: The most complete and patched Plan 9 mk port in Go. Tests exist. Remote file support is unique. Proper go.mod.
- **Cons**: Still has known unimplemented features (same TODO since 2013). Abandoned since Dec 2020. Small userbase.

### 3. zyedidia/knit — Lua-based reimagining

- **Not a fork** — independent project inspired by mk
- **Last activity**: Sep 2023 — **stale but most recent**
- **Lines of Go**: ~5,170 across 10+ packages
- **go.mod**: Go 1.19, extensive dependency tree (gopher-lua, fasthash, shellquoting, etc.)
- **Has tests**: 34 test directories, CI badge
- **Recipe language**: **Lua** (embedded gopher-lua VM). Make-like `$ target: prereq` syntax is syntactic sugar parsed into Lua objects.
- **Recipe format**: `$ target: rule` syntax within Lua `return b{ ... }` blocks
- **Parallelism**: Go goroutine-based executor, `-j N` flag, default 8 workers. Uses channels + `sync.WaitGroup` for job scheduling.
- **Out-of-date check**: **Hash-based** (default, SHA-like via fasthash) — can fall back to timestamps with `--hash=false`. **Dynamic task elision** — if a rebuilt prerequisite produces identical output, dependent rules are skipped.
- **Recipe change tracking**: Implicit dependency on recipe content — change a recipe/variable and rules auto-rebuild.
- **Sub-builds**: Namespaced in-process sub-builds via Lua modules (no `make -C` subprocess spawning).
- **Built-in sub-tools**: `knit -t graph` (dot), `knit -t clean` (auto-remove outputs), `knit -t compdb` (compile_commands.json), `knit -t commands` (export as Makefile/Ninja/shell), `knit -t status`
- **Cross-platform**: Internal shell fallback when `sh` not found
- **Pros**: Most feature-rich. Hash-based incremental builds with dynamic elision. Real meta-programming via Lua. Sub-builds are in-process. Good docs and examples. 192 stars — clear community traction.
- **Cons**: Lua is a significant dependency for a build tool (~18KB vm.go + gopher-lua). Full blown scripting language where most users just want simple rules. Syntax is non-standard (Lua `return b{...}` + `$` rules). Bigger cognitive load. Last commit Sep 2023 — not actively developed.

---

## Comparison Table

| Feature | henesy/mk | ctSkennerton/mk | zyedidia/knit |
|---------|-----------|-----------------|---------------|
| **Language** | Go (no mod) | Go 1.13 | Go 1.19 |
| **Stars** | ★4 | ★5 | ★192 |
| **Last commit** | 2020-02 | 2020-12 | 2023-09 |
| **Lines of code** | ~2,375 | ~4,300 (w/ tests) | ~5,170 |
| **Recipe syntax** | mkfile (indent-based) | mkfile (indent-based) | Lua + `$` sugar |
| **Recipe interpreter** | `sh -c` (default) | `sh -c` (default) | `sh` or internal shell |
| **Non-shell recipes** | `S[interp]` attribute | `S[interp]` attribute | Lua functions |
| **Parallelism (default)** | 4 workers | CPU count | 8 workers |
| **Parallelism (mechanism)** | sync.Cond pool | sync.Cond pool | goroutines + channels |
| **Exclusive recipe support** | Yes | Yes | No (but per-recipe failure handling) |
| **Out-of-date check** | Timestamps | Timestamps | **Hashes** (default) + timestamps |
| **Dynamic task elision** | No | No | Yes |
| **Recipe change tracking** | No | No | Yes |
| **Remote files (S3/HTTP)** | No | Yes | No |
| **Sub-builds** | No | No | Yes (in-process) |
| **Tests** | None | 4 test files | 34 test dirs, CI |
| **Man page** | No | Yes (`mk.1.md`) | Yes (`man/`) |
| **Sub-tools** | No | No | graph, clean, compdb, targets, status, commands |
| **Export to Make/Ninja** | No | No | Yes |
| **${var} expansion** | Not implemented | Not implemented | N/A (Lua) |
| **go.mod** | No | Yes | Yes |
| **Dependencies** | None | AWS SDK, isatty | gopher-lua, fasthash, pflag, etc. |
| **License** | BSD 2-Clause | BSD 2-Clause | MIT |

---

## Recommendation for `mk-rust`

### If you want a **faithful Plan 9 mk port**: `ctSkennerton/mk`

It's the closest to a maintained, fixed-up port of the original Plan 9 mk semantics. Remote file support is unique. It has proper Go module support. The code structure is clear and self-contained (7 core .go files). It's effectively the "latest" dcjones fork with all known patches applied.

### If you want **modern features + community**: `zyedidia/knit`

Knit is far more capable — hash-based builds, dynamic elision, Lua metaprogramming, sub-builds, export tools. But it's also far more complex. The Lua layer adds both power and weight. The user's concern ("Lua is excessive") is valid — for a simple build system, pulling in a full scripting VM is a lot.

### Key takeaways for a Rust port:

1. **Start from ctSkennerton/mk** — it's the cleanest Plan 9 mk reference in Go. ~2,400 lines of core logic, well-structured into lex/parse/graph/expand/recipe/rules.
2. **The parallelism model is simple** — `sync.Cond` pool with N workers. Can be replicated in Rust with `tokio::sync::Semaphore` or `crossbeam` channels.
3. **Hash-based staleness** (from knit) is a significant improvement over timestamps — worth adopting.
4. **Don't need Lua** — the mkfile format (indent-delimited recipes, `$target: $prereq:`, attributes `:V`, `:Q`, `:N`, `:E`) is perfectly adequate on its own.
5. **Consider remote files** — ctSkennerton's S3/HTTP support is a nice differentiator.
6. **Sub-builds** — knit's in-process sub-build approach avoids the `make -C` pattern but adds complexity. A simple `include` mechanism might suffice.
