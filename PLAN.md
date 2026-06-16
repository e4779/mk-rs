# mk-rust: Plan

> *"The Unix philosophy: Write programs that do one thing and do it well."* — Doug McIlroy
>
> *"Mk is an efficient general tool for describing and maintaining dependencies between files or programs."* — Andrew Hume

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
- A package manager, a daemon, or a file watcher (though a daemon mode is listed under §6 future considerations)

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

## 4. TDD strategy

### 4.1 Test pyramid

```
            ┌─────────────┐
            │  Property   │  ← fuzz parsing, random mkfile generation, graph invariants
            │   tests     │      (small set, high value)
            └──────┬──────┘
       ┌───────────┴───────────┐
       │   Integration tests   │  ← end-to-end mkfile builds: C compilation, data pipelines
       │   (testdata/*.mk)     │      (medium set, documentation value)
       └───────────┬───────────┘
   ┌───────────────┴───────────────┐
   │        Unit tests              │  ← per-module: lex, parse, graph, var, attr, sched
   │   (every pub fn, edge cases)   │      (large set, fast, CI gate)
   └───────────────────────────────┘
```

### 4.2 Unit test examples by module

**`lex` unit tests:**
- Empty input → `[Eof]`
- Single word → `[Word("hello"), Eof]`
- Rule header: `target: prereq` → `[Word("target"), Colon, Word("prereq"), Newline, Eof]`
- Comment: `# this is a comment\nword` → `[Word("word"), Eof]`
- Escaped newline (sh mode): `foo\\\nbar` → `[Word("foo"), Word("bar"), Eof]`
- Escaped newline (rc mode): `foo\\\nbar` → `[Word("foobar"), Eof]`
- Backtick: `` `echo $HOME` `` → `[Word("`echo $HOME`"), Eof]`
- Single-quoted string with spaces: `'hello world'` → `[Word("'hello world'"), Eof]`
- Recipe indentation: indent → `[Indent, Word("cmd"), Newline, Eof]`
- Double newline terminates recipe: `target:\n\tcmd\n\nnext` → correct tokens

**`parse` unit tests:**
- Simple rule: `target: prereq` → `Rule { targets: ["target"], prereqs: ["prereq"], recipe: None }`
- Rule with recipe:
  ```
  target: prereq
      echo hello
  ```
  → `Rule { targets: ["target"], prereqs: ["prereq"], recipe: Some("echo hello\n") }`
- Multiple targets: `a b: c d` → `Rule { targets: ["a", "b"], prereqs: ["c", "d"], ... }`
- Attributes: `target:VQ: prereq` → `Rule { attributes: VIRTUAL | QUIET, ... }`
- Assignment: `CC = gcc` → `Assign { name: "CC", value: "gcc" }`
- Assignment with unexport: `CC:U = gcc` → `Assign { name: "CC", value: "gcc", attributes: UNEXPORTED }`
- Include: `< subdir/mkfile` → `Include::File("subdir/mkfile")`
- Include command: `<| gcc -M *.c` → `Include::Command("gcc -M *.c")`
- Metarule: `%.o: %.c` → `Rule { targets: ["%.o"], prereqs: ["%.c"], is_metarule: true }`
- Regex metarule: `([a-z]+)\\.o:R: \\1.c` → `Rule { is_regex: true, ... }`
- Error: missing colon → `ParseError::ExpectedColon`

**`graph` unit tests:**
- Single node, no prereqs: `a:` → graph with 1 node, 0 arcs
- Chain: `a: b`, `b: c` → 3 nodes, arcs: b→a, c→b
- Diamond: `a: b c`, `b: d`, `c: d` → 4 nodes, 4 arcs
- Cycle: `a: b`, `b: a` → `GraphError::Cycle`
- Metarule match: `%.o: %.c` applied to target `foo.o` → edge from `foo.c` to `foo.o` with stem `foo`
- Pruning: metarule edge removed when concrete rule exists for same target
- Virtual target: `V` attribute → node marked virtual, not checked for file existence
- Transitive closure: `target: a`, `a: b`, `b` exists on disk → graph includes `target → a → b`, stops at `b`

**`var` unit tests:**
- Simple: `$FOO` where `FOO=bar` → `"bar"`
- Bracketed: `${FOO}` → `"bar"`
- Recursive: `A=$B`, `B=hello` → `$A` → `"hello"`
- Substitution: `${FOO:.c=.o}` where `FOO=src/main.c` → `"src/main.o"`
- Pattern substitution: `${FOO:src/%/main.c=%}` → stem replacement
- `$$` → literal `$`
- Undefined variable: `$UNDEFINED` → `""` (mk convention) + warning
- Namelist: `A=1 2 3`, `$A` → `"1 2 3"`, `$A(1)` → `"1"`, `$A(2)` → `"2"`
- Environment import: `$HOME` → from `std::env::var("HOME")`

**`sched` unit tests (with mock shell):**
- Single target, one job → job dispatched, returns success
- Chain: a→b→c, all stale → jobs run in correct order (c first, then b, then a)
- Diamond: independent branches d→b and d→c → both can run in parallel (if NPROC≥2)
- NPROC=1 → sequential execution, topological order
- NPROC=4 → up to 4 concurrent jobs
- `E` attribute → exclusive job blocks other workers
- `-k` flag → failure in one job doesn't abort others (as long as prereqs satisfied)
- Recipe error → node marked failed, prereqs that depend on it not built
- `-n` flag → no recipes actually execute, all jobs "succeed"
- `-t` flag → mtime updated on targets, no recipes executed

### 4.3 Integration test examples

Test fixtures in `testdata/` directory:

**`testdata/hello/` — simplest C compilation:**
```
mkfile:
    hello: hello.c
        cc -o hello hello.c
hello.c:
    int main() { return 0; }
```
→ `make testdata/hello/hello.json` builds, checks hello exists and is newer than hello.c

**`testdata/library/` — object file compilation with metarules:**
```
mkfile:
    CC = cc
    CFLAGS = -Wall -O2
    %.o: %.c
        $CC $CFLAGS -c $stem.c -o $stem.o
    prog: main.o util.o
        $CC -o $target $prereq
```
→ builds prog from two .o files, each compiled from .c

**`testdata/data-pipeline/` — data transformation (Mike Bostock style):**
```
mkfile:
    data/processed.csv: data/raw.csv
        python transform.py $prereq > $target
    report.html: report.Rmd data/processed.csv
        R -e "rmarkdown::render('$stem.Rmd')"
```
→ simulates a data science workflow

**`testdata/includes/` — nested mkfile includes:**
```
mkfile:
    < subdir/common.mk
    target: prereq
        cmd
common.mk:
    CC = gcc
```
→ verifies include chaining, variable scope isolation

**`testdata/regex/` — regex metarules:**
```
mkfile:
    ([a-z]+)\.o:R: \1.c
        cc -c $stem1.c -o $target
```
→ verifies regex rule matching and grouped stem variables (`$stem1`, `$stem2`)

### 4.4 Property tests

- **Parse roundtrip**: generate random valid AST → serialize to mkfile text → parse back → assert AST equal
- **Graph invariants**: generated graphs must be acyclic, no dangling arcs, transitive closure is idempotent
- **Variable expansion**: random variable names → expansion is always deterministic (same input → same output)
- **Shell quoting**: any string, when quoted and then parsed by the shell, should produce the original string (roundtrip through `shell.quote()`)

---

## 5. Implementation phases

> See `TRACEABILITY.md` for the full F-xxx → module → phase mapping.

### Phase overview

| Phase | Features | Effort | What you can do after | Progress |
|-------|:---:|--------|-----------------------|:--------:|
| **1a** — Core MVP | 22 | ~2 weeks | Build a C program from explicit rules | 22/22 ✅ |
| **1b** — Variables & includes | 12 | ~1.5 weeks | Multi-file projects with `< file` includes | 12/12 ✅ |
| **2** — Metarules & parallel | 22 | ~2.5 weeks | `%.o: %.c` patterns, NPROC parallel builds | 22/22 ✅ |
| **3** — Aggregates & polish | 10 | ~2 weeks | Full plan9port mk compatibility | 8/10 █████░ |
| **Deferred** — Plan 9 specifics | 4 | — | `$O`, `membername`, stdout-as-mkfile | 0/4 |

*Phase 1a modules done: `lex` (✓), `attr` (✓), `error` (✓). Next: `parse`, `graph`.*

### Phase 1a: Core parser + serial execution (REAL MVP)

**Goal:** Parse simple mkfiles, build DAG from concrete rules only, execute recipes sequentially with sh. This is the smallest thing that could possibly work — you can build a C program from an explicit mkfile.

**Estimated effort:** ~2 weeks (one person, part-time)

**Features:** 22 specs (F-001..F-003, F-006, F-008..F-011, F-014..F-016, F-018, F-020..F-021, F-025, F-040..F-042, F-045, F-059, F-064..F-065, F-067..F-068)

| Module | Scope |
|--------|-------|
| `lex` | Tokenizer: words, colons, equals, indents, comments, escaped newlines (sh+rc), backticks, `<`/`|` include tokens, single/double quotes |
| `parse` | Rules (single+multi target), assignments, attributes (all 9: V/Q/N/U/D/E/P/R/n), recipes, metarule detection (is_metarule/is_regex flags). **No** include resolution yet (parser sees `<` but doesn't call include module) |
| `graph` | DAG from concrete rules. Transitive closure. Staleness via `mtime`. Cycle detection. Virtual targets. Missing intermediate optimization. **No** metarule application, **no** pruning |
| `var` | `$VAR`, `${VAR}`, `$$`. Environment import. Precedence (cmdline > file > env > builtin). Built-in defaults (CC, NPROC, etc.). `Scope::export()`. Recipe-time injection ($target, $prereq, $pid). **No** namelists, **no** substitution patterns, **no** $stem |
| `shell` | `Shell` trait in mk-core. `ShShell` in mk-shell using `std::process::Command` with `/bin/sh -ec`. `find_unescaped()`, `quote()`. **No** RcShell, **no** MKSHELL selection |
| `recipe` | First-char elision (utility function, not called by run() — parser handles it). Q/E/D attribute support. -n/-e/-t/-s flags. `$target`/`$prereq`/`$pid` injection. `touch_target()` |
| `sched` | Serial execution via topological sort (post-order DFS). SchedOptions: keep_going, no_exec, explain, touch, silent, all, force_intermediates. `ResolvedRule` type. **No** NPROC parallelism |
| `attr` | All 9 attributes: V/Q/N/U/D/E/P/R/n. Bitflags with `is_*` methods. `parse_attributes()`. ATTR_HELP |
| `include` | `IncludeContext` with chain-based circular detection. `include_file()`. **Not yet wired into parser** |
| `cli` | clap derive: -f, -n, -e, -t, -a, -k, -i, -s, -d, -w, -C. Reads mkfile, builds scope, executes. Scope::export() for env. Thin wrapper |

**Tests:**
- Unit: lex (token roundtrip), parse (rule/assign), graph (3-node chain, cycle detection), var (expansion), attr (bitflags)
- Integration: `testdata/hello` — single C file → binary
- Integration: `testdata/hello-rebuild` — touch source → rebuild; no changes → up-to-date

**Exit criteria:**
- `cargo run -- -f mkfile hello` builds a C program
- Changing source → rebuilds target
- Unchanged source → "hello is up to date"
- Cycle in mkfile → error with file:line
- Missing prerequisite and no rule → error
- `-n` prints recipes without executing
- All unit tests green (target: >90% coverage on lex, parse, graph)

---

### Phase 1b: Variables, includes, basic attributes

**Goal:** Round out the core: recipe-time variables, `< file` includes, missing intermediate optimization, and CLI flags that don't require parallelism.

**Estimated effort:** ~1.5 weeks

**Features:** 12 specs (F-013, F-017, F-022..F-024, F-026, F-031, F-033..F-034, F-038, F-046..F-049, F-051, F-062, F-069)

| Module | Scope |
|--------|-------|
| `lex` | Backtick tokens (`` `cmd` ``). `<` and `|` tokens for includes |
| `parse` | Multi-target rules. Include directive tokens recognized. Recipe-less rules (prereq merging). **Include resolution still pending** |
| `graph` | Missing intermediate optimization. `-i` flag via `force_intermediates`. `stale_nodes()` with optional pretend timestamps |
| `var` | `$target`, `$prereq`, `$pid` builtins (injected at recipe execution). Short-circuit eval (parse vs execution time). `Scope::export()` |
| `include` | `< file` with recursive parsing. Circular include detection via chain tracking. **Not yet wired into parser** |
| `sched` | `-k` keep-going. `-t` touch mode. `-a` force rebuild (`SchedOptions.all`). Leaf nodes without rules skipped gracefully |
| `recipe` | `touch_target()` rewrites file to update mtime. `-t`/`-n`/`-e` flags |
| `cli` | All flags wired: `-k`, `-t`, `-a`, `-i`. Silent renamed from sequential. `scope.export()` |

**Tests:**
- Unit: include (recursive, circular), var ($target/$prereq in recipe context), parse (multi-target)
- Integration: `testdata/includes` — nested mkfiles, variable scoping
- Integration: `testdata/intermediates` — missing .o file, pretend-timestamp optimization

**Exit criteria:**
- `< subdir/mkfile` includes and merges rules correctly
- Circular include → error with chain printed
- `$target` and `$prereq` expand to correct values in recipe
- Missing intermediate .o is skipped if dependents are up to date
- `-k` continues after a recipe failure
- `-t` touches targets without running recipes

---

### Phase 2: Parallelism + metarules + full variables

**Goal:** Achieve feature parity with plan9port mk's core functionality: parallel builds, pattern metarules, and all recipe-time variables.

**Estimated effort:** ~2.5 weeks

**Features:** 22 specs (F-004..F-005, F-007, F-019, F-027, F-029, F-035..F-037, F-039, F-044, F-052..F-056, F-058, F-060..F-061, F-063, F-066)

**Deliverables:**

| Area | Scope |
|------|-------|
| `sched` | NPROC-based worker pool (crossbeam channels + threads). Multiple jobs run concurrently. Semaphore for backpressure. Exclusive (`E`) job support. Progress echo (`target: ...`) |
| `parse` | `%` metarules (single `%` match), `&` metarules (ampersand matchers), `R:` regex metarules. Wire `< file` includes to include module. Backtick expansion in assignments |
| `graph` | Metarule application: for each unknown target, try `%` metarules, then `&`, then `R`. Pruning: vacuous + ambiguous edge removal |
| `var` | `$stem`, `$target`, `$prereq`, `$newprereq`, `$alltarget`. Namelist access: `$stem(1)`, `$stem(N)`. Recipe-time variable expansion |
| `shell` | `duckscript::Shell` in mk-shell (feature-gated). Shell selection via `$MKSHELL`. **No** RcShell — sh + duckscript covers all use cases |
| CLI | `-p N` / `$NPROC` for parallelism. `-i` for missing intermediates. `-k` for keep-going |

**Tests:**
- Parallel execution: diamond DAG with sleep recipes → verify concurrent execution via timestamps
- Metarule matching: `%.o: %.c` with 3 .c files → 3 rules applied, correct stems
- Regex metarule: complex pattern with multiple capture groups → `$stem1`, `$stem2`
- Variable edge cases: recursive `$target` in prereqs, `$prereq` with multiple words
- Integration: `testdata/library` with metarules (%.o: %.c)
- Integration: `testdata/regex` with R: metarules

**Exit criteria:**
- `NPROC=4` builds diamond DAG with demonstrable parallelism (wall time < sum of recipe times)
- `%` and `R:` metarules work identically to plan9port mk
- `$stem`, `$target`, `$prereq` expand correctly inside recipes
- `-i` flag builds missing intermediate targets automatically
- `<| command` executes command and includes its stdout as mkfile
- `${VAR:%.c=%.o}` namelist transforms work
- All plan9port mk regression tests (basic rule set) pass

---

### Phase 3: Includes + dynamic features + polish

**Goal:** Full feature parity with plan9port mk, plus Rust-native improvements. Production-ready.

**Estimated effort:** ~2 weeks

**Features:** 10 specs (F-024, F-028, F-030, F-032, F-043, F-050, F-058)

**Deliverables:**

| Area | Scope |
|------|-------|
| `include` | `<| command` includes (execute command, parse stdout as mkfile). Circular include detection. Include stack for error messages. Dynamic mkfile generation |
| `graph` | Hash-based staleness (optional, from knit): `--hash` flag uses blake3 hash instead of mtime. Recipe-change tracking: if recipe text changes, target is stale |
| `archive` | `lib(member)` auto-rule generation. Archive mtime tracking |
| `recipe` | Stdout-as-mkfile: if recipe produces mkfile-parseable output on stdout, it can be treated as an include (dynamic dependency generation). Error messages with source location |
| `var` | Full `shsub()`-compatible substitution. All edge cases from plan9port test suite |
| CLI | All flags: `-d` (debug), `-k` (keep-going), `-s` (silent), `-w` (what-if), `-C dir` (chdir). `--color auto/always/never`. `--json` for machine-readable output. `--version`, `--help` |
| Docs | Man page (`mk.1.md`). README with quick-start. Integration test READMEs. API docs for mk-core |

**Exit criteria:**
- Passes all plan9port mk regression tests (ported to Rust test format)
- `cargo install --path crates/mk-cli` installs on Linux/macOS
- Man page in `man/man1/mk.1`
- `< 2 seconds to check 100-target project with no changes
- Zero panics in normal operation — all errors propagate as `Result`
- `lib(member)` syntax generates archive rules automatically
- `-d[egp]` debug output matches plan9port format
- Recipe stdout can feed back as mkfile input (dynamic includes)

---

## 6. Open design questions

### 6.1 Threading model: crossbeam threads vs tokio async

**Question:** Should parallelism use std::thread via crossbeam channels, or tokio async tasks?

| Approach | Pros | Cons |
|----------|------|------|
| **crossbeam threads** | Simple, matches plan9port fork() model. No runtime dependency. Deterministic. Lower compile times. Channels map well to job queue. | Threads are heavier than tasks. 100 targets with NPROC=100 would be 100 threads. |
| **tokio async** | Lightweight tasks — NPROC=100 is fine. Can use `tokio::process::Command` for recipe execution. `Semaphore` for backpressure. | Adds tokio dependency (~20+ crates). Async I/O is irrelevant for mk (it's CPU-bound fork/exec). Mixing sync File I/O with async can cause issues. |

**Background:** plan9port mk uses `fork()`+`exec()`. The Go ports use goroutines. For a Rust port, the job structure is:
1. Wait for a free slot (semaphore)
2. Spawn a child process (recipe execution)
3. Wait for child to exit (blocking)
4. Signal completion, free slot

This is inherently blocking. Async adds no benefit and adds complexity.

**Tentative answer:** **crossbeam threads.** The parallelism model is a fixed-size thread pool (NPROC threads max), each blocking on `Command::status()`. Simple, fast, no async runtime.

---

### 6.2 Graph memory model: arena (Vec + indices) vs Rc/RefCell

**Question:** How to store the DAG where nodes have bidirectional references?

| Approach | Pros | Cons |
|----------|------|------|
| **Arena (Vec<Node>, Vec<Arc>, usize indices)** | Contiguous memory, cache-friendly. Clear ownership (Graph owns everything). No Rc cycles. Deterministic drop order. Matches plan9port C model (arrays of structs). | Manual index management. Need to be careful not to hold indices across mutations. `NodeIndex` is just `usize` — no type safety against wrong arena. |
| **Rc<RefCell<Node>>** | Each node is an independent allocation. Back-references via `Weak`. No index errors. Rust-idiomatic at first glance. | Interior mutability required for graph building → RefCell borrow panics at runtime. Cycles need Weak. Allocation per node. Slower iteration (pointer chasing). |

**Background:** plan9port C uses a linked list of `Node` structs, each with an `Arc*` to its prereqs and a `Node* next` for the rule's target list. The Go ports use `map[string]*Node` with `[]*Edge`. The Rust port should improve on both.

**Tentative answer:** **Arena (Vec + indices).** Use `NodeIndex(usize)` and `ArcIndex(usize)` newtypes for type safety. Graph building is a single phase — after the graph is built, it's immutable (no more mutations). This eliminates the main risk of index invalidation. Benchmarks will confirm if the contiguous layout outperforms pointer chasing (expected: yes, especially for DAG traversal).

---

### 6.3 Recipe interpreter: external shell vs embedded duckscript

**Question:** Should recipe execution use an external shell process, or embed a scripting language directly?

| Approach | Pros | Cons |
|----------|------|------|
| **External shell (sh/rc)** | Faithful to Plan 9 mk. Users already know sh. No new dependencies. Recipes use existing tools. Simple error model. | Fork/exec per recipe. Environment passing overhead. Platform-specific shell availability. |
| **Embedded duckscript** | No subprocess — faster. Built-in file ops (cp, mv, glob). Cross-platform (no /bin/sh dependency). Already proven in cargo-make. | Adds a dependency. New syntax for users to learn. Not a real shell — can't run arbitrary binaries without `exec`. |
| **Hybrid**: use duckscript as an optimization, external shell as default | Best of both. Simple recipes run in-process. Complex recipes (calling gcc, python, R) still use sh. Configurable per rule. | Two code paths. Complexity in shell selection. |

**Background:** The Plan 9 mk philosophy is that recipes are shell scripts. Both `sh` and `rc` are first-class. The `S:` attribute already supports custom interpreters. Duckscript would just be another interpreter.

**Tentative answer:** **External shell as default, duckscript as optional.** `$MKSHELL` defaults to `sh -c`. `S[duckscript]` attribute enables in-process execution for recipes that only need file ops. Feature-gate duckscript behind a Cargo feature flag (`duckscript`). This keeps the core crate lean while offering a performance option.

**cargo-make validation (June 2026):** Confirmed the pattern. duckscript integration is 3 function calls: `Context::new()` → `duckscriptsdk::load()` → `run_script()`. No deep coupling. The `envmnt` crate handles `${VAR}`/`$VAR` expansion (same syntax as Plan 9 mk) and could simplify our var.rs module.

---

### 6.4 Staleness detection: mtime vs hash-based

**Question:** Should mk-rust use mtime comparison (like plan9port mk) or content hashing (like knit)?

| Approach | Pros | Cons |
|----------|------|------|
| **mtime only** | Fast — just stat() each file. Simple. Standard behavior — matches plan9port mk and GNU Make. | Touch breaks it. Unchanged content triggers rebuilds. Clock skew issues. No recipe-change detection. |
| **Hash-based (blake3)** | Accurate — only rebuild when content changes. Enables dynamic task elision (if rebuilt prereq produces identical content, dependents skip). Recipe-change triggered rebuilds. | Each build reads every file to compute hash (I/O cost). Slower for large files. Different behavior from Plan 9 mk → compat concern. |

**Background:** GNU Make and plan9port mk use mtime. Knit defaults to hashes. The mtime approach is "good enough" for most use cases. Hash-based is more correct but slower.

**Tentative answer:** **mtime by default, hash-based as an option.** `--hash` flag switches to blake3 content hashing. `P:` attribute (custom comparison) already supports arbitrary staleness logic. Recipe-change tracking can be implemented by hashing the recipe text and storing the hash in `.mk.state` — independent of target file hashing.

---

### 6.5 File watching / daemon mode

**Question:** Should mk-rust support a daemon mode that watches files and auto-rebuilds?

| Approach | Pros | Cons |
|----------|------|------|
| **Daemon mode (`mk -w` or `mk --watch`)** | Useful for rapid iteration. Matches tools like `fswatch`, `cargo watch`. Could leverage inotify/kqueue for efficiency. | Scope creep. mk is a build tool, not a daemon. Plan 9 mk never had this. Adds complexity (signal handling in threads, file watching dependency). |
| **Separate tool (`mk-watch`)** | Clean separation. Use `notify` crate for file watching, run `mk` on changes. Single-purpose Unix tool. | Extra binary. Duplication of target resolution logic. |
| **Don't build it** | Keeps mk-rust focused. Users can script: `while inotifywait .; do mk; done`. | User ergonomics. |

**Tentative answer:** **Don't build it.** mk is a build tool, not a daemon. Plan 9 mk never had this feature. Users who want watch mode can use `watchexec`, `cargo watch`, or a shell one-liner. If demand is high, a separate `mk-watch` tool using the `notify` crate is better than bloating mk-core.

### 6.7 `-s` flag semantic: silent vs sequential

**Conflict:** plan9port mk uses `-s` to mean "sequential" (force NPROC=1). Our implementation uses `-s` to mean "silent" (suppress recipe printing). Phase 2 parallelism makes this a real conflict.

**Options:**
1. Rename our `-s` to `-q` (quiet) and implement `-s` as sequential in Phase 2
2. Keep `-s` as silent, use a different flag for sequential (e.g., `-j 1` or just set NPROC=1)
3. Keep both: `-s` = sequential, silence via `Q` attribute on rules

**Tentative answer:** Option 1. Rename silent to `-q` (quiet), reserve `-s` for sequential mode matching plan9port behavior. The `Q` attribute already handles per-rule silence.

---

### 6.6 Cross-platform support: Windows

**Question:** How much effort should go into Windows support?

- Plan 9 mk assumes Unix: `fork()`+`exec()`, `/bin/sh`, symlinks, signal handling
- The Go ports are Unix-only
- Rust can be cross-platform, but the effort is non-trivial

**Tentative answer:** **Unix-first, Windows-later.** Phase 1–3 target Linux and macOS. Windows support can be explored as a Phase 4 if there's demand. The `Shell` trait and `std::process::Command` abstractions already provide a path. The main hurdles: no `/bin/sh` by default (would need `cmd.exe` shell), different path separators, no `fork()` semantics for NPROC.

---

## 7. Rust idioms to embrace

### 7.1 Error handling: `Result<T, MkError>` everywhere

```rust
// Centralized error type via thiserror
#[derive(Error, Debug)]
pub enum MkError {
    #[error("lex error at line {line}: {message}")]
    Lex { line: usize, message: String },
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("variable error: {0}")]
    Var(#[from] VarError),
    #[error("graph error: {0}")]
    Graph(#[from] GraphError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    // ...
}
```

No panics in library code. Every fallible operation returns `Result`. The CLI layer is the only place where `unwrap()` or `expect()` are acceptable (and even then, prefer `?` in `main()`).

### 7.2 Builder pattern for Rule, Node, Recipe

```rust
let rule = Rule::builder()
    .target("hello.o")
    .prereq("hello.c")
    .recipe("cc -c hello.c -o hello.o")
    .attribute(Attributes::VIRTUAL)
    .build()?;
```

Complex structs with optional fields should have builders. This avoids constructor overloads and makes test code readable.

### 7.3 Trait-based Shell abstraction

The `Shell` trait cleanly abstracts the strategy pattern that plan9port C implements via function pointers — the most natural Rust translation. (Architecture notes live in the `mk-core::shell` and `mk-shell` crate docs.)

### 7.4 Iterators for graph traversal

```rust
impl Graph {
    /// Iterate over prereqs of a node.
    pub fn prereqs(&self, node: NodeIndex) -> impl Iterator<Item = &Node> { ... }

    /// Topological sort of stale nodes (Kahn's algorithm).
    pub fn topological_sort(&self, roots: &[NodeIndex]) -> Vec<NodeIndex> { ... }

    /// Walk the DAG in dependency order.
    pub fn walk(&self, root: NodeIndex) -> DagWalk { ... }
}
```

Avoid manual index loops in client code. Provide iterator adapters. This makes scheduler logic declarative.

### 7.5 Serde for AST (debugging, future LSP)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule { ... }
```

`serde` `Serialize`/`Deserialize` derives on all AST types. This enables:
- `mk --print-ast --json` → dump parsed AST as JSON (debugging, tooling)
- Future: mkfile formatter (`mk fmt`) — deserialize, reformat, serialize
- Future: LSP server — parse mkfile, serve AST queries

Feature-gated behind `serde` feature flag to keep the default dependency footprint small.

### 7.6 Clap derive for CLI

```rust
#[derive(Parser)]
#[command(name = "mk", about = "maintain dependencies between files")]
struct Cli {
    /// Mkfile to read (default: mkfile)
    #[arg(short = 'f', default_value = "mkfile")]
    file: PathBuf,

    /// Print recipes but do not execute
    #[arg(short = 'n')]
    no_exec: bool,

    /// Touch targets instead of running recipes
    #[arg(short = 't')]
    touch: bool,

    /// Keep going after errors
    #[arg(short = 'k')]
    keep_going: bool,

    /// Maximum parallel jobs (overrides $NPROC)
    #[arg(short = 'p')]
    nproc: Option<usize>,

    /// Targets to build (default: first target in mkfile)
    targets: Vec<String>,
}
```

Clean, self-documenting, auto-generated `--help`. Aligns with Rust CLI conventions.

### 7.7 Criterion for benchmarks

```rust
// benchmarks/graph_benchmark.rs
fn bench_dag_build_1000_nodes(c: &mut Criterion) {
    c.bench_function("dag_build_1000_nodes", |b| {
        b.iter(|| {
            let rules = generate_rules(1000);
            let mut builder = Builder::new(rules, vec![]);
            builder.build(&["all"]).unwrap()
        });
    });
}
```

Use `criterion` for benchmarks of:
- Lexing large mkfiles
- DAG construction (metarule matching)
- Variable expansion of deeply nested references
- Hash-based staleness on large file trees

Not a development gate — just visibility into performance regression.

### 7.8 No unsafe code

Target `#![forbid(unsafe_code)]` on `mk-core`. The only potential need for `unsafe` would be optimization of hot paths — and that can be deferred until benchmarks prove it necessary. The C codebase has many raw pointer casts; Rust's ownership model eliminates the need for almost all of them.

---

*This plan is a living document. Decisions marked "Tentative answer" are subject to change as implementation reveals new constraints. Open questions in §6 should be resolved before reaching the relevant phase.*

---

## 8. Implementation Insights (post-Phase 1)

### Test fixtures from real-world mkfiles

43 real-world mkfiles collected in `testdata/external/` from 4 sources. Phase 2 testing strategy:

**Phase 2a — ctSkennerton tests first (TDD driver)**
These 17 files are small (avg 8 lines), focused, single-feature. Use them as acceptance tests:
- `test2.mk` → $prereq/$stem/$target expansion → wire into graph+sched
- `test13.mk` → `${var:%=...}` namelist transforms → implement in var.rs
- `test17.mk` → `:R:` regex metarule → implement in parse+graph
- `test6.mk`—`test9.mk` → `< file` includes → wire include module into parser
- `test12.mk` → `:V:` virtual targets → already works, confirm
- `test14.mk` → `<| command` dynamic mkfile → implement in include

**Phase 2b — plan9port integration tests**
Larger multi-file mkfiles for end-to-end validation:
- `mkfile.test` → mk's own test suite (150 lines) — ultimate acceptance test
- `src/lib9/mkfile` → large file lists, multi-directory metarules
- `src/cmd/devdraw/mkfile` → conditional install, `:Q:`, `<|cmd`

**Phase 2c — archive aggregates (9legacy bootmkfile)**
- `bootmkfile` → `$BOOTLIB(%.$O):N:`, `$newprereq` — archive pattern

### What worked well

1. **Arena (Vec + indices) for DAG** — NodeIndex/ArcIndex newtypes proved clean and fast. No Rc/RefCell pain. Graph mutation is localized to build_graph() and execute(); after that, the graph is read-only.

2. **Parallel sub-agents with precise prompts** — Three agents simultaneously writing lex, attr, error produced correct, compiling code in one shot. Key: each agent got exact types, exact tests, and "ONLY write this file" constraint.

3. **Centralized error types** — Having all errors in error.rs with `#[from]` auto-conversions eliminated duplication. When shell.rs accidentally defined its own ShellError, the fix was trivial: `pub use crate::error::ShellError`.

4. **`Scope::export()` method** — Added during implementation, not in original plan. Flattens scope chain to HashMap. Simplified CLI code significantly. Should be adopted as a pattern: add convenience methods when you find yourself repeating the same 3-line pattern twice.

### What surprised us

1. **Double elision bug** — Parser strips indent (by consuming Indent token), but recipe::run() also called elide_first_char(). Net result: `cc` → `c`. Root cause: unclear contract between parser and recipe module. Fix: parser owns indent stripping; recipe never elides. Lesson: document which module owns each transformation.

2. **stale_nodes short-circuit** — `Iterator::any()` in check_stale() stops at first stale prereq, never visiting sibling prereqs. This caused -k tests to fail because some stale nodes were never detected. Temporary fix in sched.rs; permanent fix should be in graph.rs. Lesson: topological algorithms need full traversal, not short-circuit.

3. **touch_target didn't update mtime** — Original implementation only created files if missing. `-t` flag was silently broken for existing files. Fixed by reading+rewriting file content. Lesson: stateless functions that look correct can have hidden bugs — test with existing files, not just missing ones.

4. **`-s` flag semantic conflict** — plan9port's `-s` = sequential (NPROC=1). Our `-s` = silent (suppress output). When Phase 2 adds parallelism, this becomes a real conflict. Recommendation: rename our flag to `-q` (quiet) and reserve `-s` for sequential mode.

### Gaps to address before Phase 2

1. **Include module not wired into parser** — The `include` module exists with 14 tests, but the parser doesn't call `include_file()` when it sees `<` tokens. A parsed `< file` directive currently errors as "ExpectedColon". Phase 2 should integrate the include module into the parser.

2. **Integration test fixtures needed** — We have 194 unit tests but only one manual integration test (testdata/hello). Phase 2 needs: `testdata/library/` (metarules), `testdata/includes/` (nested mkfiles), `testdata/regex/` (R: metarules).

3. **The `-s`/`-q` flag cleanup** — Before adding parallelism, resolve the semantic conflict. Either rename current `-s` (silent) to `-q` (quiet), or make `-s` control sequencing (NPROC=1) and use a different mechanism for silence.

4. **Recipe-time variable expansion order** — Currently $target, $prereq, $pid are injected in recipe::run(). But $stem requires graph context (metarule matching). Phase 2 must thread stem through graph → sched → recipe. The Recipe struct already has space for this.

5. **The plan's Phase 1a/1b module descriptions are outdated** — They describe what we PLANNED to implement, not what we ACTUALLY implemented. E.g., lex handles backticks and `<` tokens; parse handles multi-target rules. These should be updated to match reality.

### Post-Phase 2 insights (cargo-make research)

1. **duckscript embedding is trivial** — 3 function calls: `Context::new()`, `duckscriptsdk::load()`, `run_script()`. No architectural changes needed — just a new `DuckShell` impl of the `Shell` trait.

2. **`envmnt` crate worth evaluating** — Handles `${VAR}`/`$VAR` expansion with defaults (`${VAR:-default}`). Could simplify or replace our `var.rs` expansion logic. Phase 3 decision: evaluate vs keep custom code.

3. **cargo-make uses DFS, not petgraph** — Validates our simple recursive DAG approach. No need for a graph library.

4. **CLI flags to adopt**: `--loglevel` (debug levels), `--env KEY=VALUE` (override), `--print-steps` (like `mk -n`), `--cwd` (working dir).

5. **Explicitly NOT adopting**: plugin system, task inheritance (`extend`/`clear`), profiles, workspaces, Shell2Batch, inline `@rust`.

## 9. Release preparation

### Pre-publication checklist

Based on toonq v0.2.4 release experience:

- [x] Version bump in `crates/mk-cli/Cargo.toml` (0.1.0)
- [x] `mk --version` / `mk-graph --version` show git hash (build.rs done)
- [x] Git hash survives `cargo publish` (GIT_HASH file + fallback to git rev-parse)
- [x] `cargo publish --dry-run -p mk-rs` — no errors, no warnings
- [x] CI pipeline (`.gitverse/workflows/ci.yml`): build + test on push, publish on tags
- [x] crates.io token in GitVerse secrets (`CARGO_REGISTRY_TOKEN`)
- [x] `git tag v0.1.0` + push tag triggers publish
- [x] GitHub mirror: push to `git@github.com:e4779/mk-rs.git`
- [x] README: install instructions, quick start, mk-graph (DOT/JSON), links
- [x] CHANGELOG.md or release notes
- [x] Man page (`docs/mk.1.md`) complete
- [x] License (MIT) in Cargo.toml + LICENSE file
- [x] Keywords/categories in Cargo.toml
- [x] Documentation link (docs.rs or repo docs/)
- [x] No stale TODO comments or debug prints in lib code
- [x] No unwrap() without justification (sched mutex + graph checks are justified)
- [x] Clippy clean: `cargo clippy -- -D warnings`
- [x] All documentation in English
- [x] AGENTS.md: 4 architecture decisions + 5 gotchas (20 lines)
- [x] Workspace crates (mk-rs-core, mk-rs-shell) have correct versions

### CI pipeline

`.gitverse/workflows/ci.yml`:
- **test**: `cargo build --verbose` + `cargo test --verbose` on every push/PR
- **publish**: verify version matches tag, dry-run, publish to crates.io on `v*` tags

### Version policy

Semantic versioning. Current: `0.1.0`.
Patches (bugfixes, docs, infra) → `0.1.x`. Minor (new features) → `0.x.0`.
mk-cli (binary crate) version is the public version.
mk-core and mk-shell are internal workspace crates — not published to crates.io.
Only `cargo publish -p mk-rs` (the CLI).
