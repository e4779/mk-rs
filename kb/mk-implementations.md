# Plan 9 mk: Implementations & Family Tree

## Family Tree / Lineage

```
                          ┌─────────────────────────────┐
                          │  Original Plan 9 mk (C)     │
                          │  Bell Labs, ~1992           │
                          │  Andrew Hume, Bob Flandrena │
                          └─────────────┬───────────────┘
                                        │
                    ┌───────────────────┼───────────────────┐
                    ▼                   ▼                    ▼
    ┌──────────────────────────┐  ┌──────────────┐  ┌───────────────┐
    │  Inferno mk (Limbo/C)    │  │ 9front mk C  │  │ Plan 9 from   │
    │  Vita Nuova              │  │ (active dev) │  │ Bell Labs mk  │
    └──────────────────────────┘  └──────┬───────┘  └───────┬───────┘
                                         │                  │
                                         │    ┌─────────────┘
                                         │    │
                                         ▼    ▼
                          ┌──────────────────────────┐
                          │  plan9port mk (C)        │
                          │  Russ Cox, 2004-present  │
                          │  ~4,350 LOC, 31 files    │
                          └─────────────┬────────────┘
                                        │
                    ┌───────────────────┼───────────────────────┐
                    ▼                   ▼                       ▼
    ┌─────────────────────┐  ┌──────────────────┐  ┌────────────────────┐
    │ dcjones/mk (Go)     │  │ hmk (Haskell)    │  │ Standalone builds  │
    │ github.com/dcjones/ │  │ mboes/hmk        │  │ of plan9port mk    │
    │ mk                  │  │ ~700 LOC         │  │                    │
    │ ~2,285 LOC          │  │ 2009-2016        │  │ razvanm/plan9mk    │
    │ 2015                │  │                  │  │ RadioNoiseE/       │
    └────────┬────────────┘  └──────────────────┘  │ plan9mk            │
             │                                     └────────────────────┘
       ┌─────┴──────┐
       ▼            ▼
  ┌──────────┐ ┌──────────────────┐
  │ henesty/ │ │ ctSkennerton/mk │
  │ mk       │ │ (Go)            │
  │ (Go)     │ │ ~3,273 LOC      │
  │ ~2,535   │ │ 2020            │
  │ LOC      │ │ + S3/http URLs  │
  │ 2020     │ │ + rc shell      │
  │ + rc     │ │ + tests         │
  │ shell    │ └──────────────────┘
  └──────────┘
```

## Implementation Details

### 1. Original Plan 9 mk (C)

| Attribute | Value |
|-----------|-------|
| Language  | C (Plan 9 dialect: `#include <libc.h>`, no POSIX) |
| Author    | Andrew Hume, Bob Flandrena (Bell Labs) |
| Year      | ~1992 |
| Recipe interpreter | rc (Plan 9 shell) by default |
| Key innovation | First clean-room rewrite of make |
| Paper | "Maintaining Files on Plan 9 with Mk" (mk.pdf in plan9port) |

**Why mk was created:** Make had accumulated decades of cruft. The Plan 9 team
wanted a clean, principled build tool. Key differences from make:
- Recipes delimited by *any* indentation, not just tabs
- Variables like `$target`, `$prereq`, `$stem` instead of `$@`, `$^`, `$*`
- Rule attributes instead of magic `.SUFFIXES:` targets
- Pattern rules use `%` (like make) **and** regular expression rules
- Meta-rules matched against targets, not prereqs
- No special `.PHONY` target — use `V` attribute
- `$stem` from pattern matching is a *word list*, not just a string

### 2. plan9port mk (C) — The Reference Implementation

| Attribute | Value |
|-----------|-------|
| Language  | C (POSIX port of Plan 9 C, using Plan 9 libs: libc, libfmt, libbio, libregexp, libutf) |
| Author    | Russ Cox (Plan 9 Foundation) |
| Year      | 2004 (initial plan9port) |
| LOC       | ~4,350 (31 .c/.h files) |
| Recipe interpreter | sh by default, rc if `FORCERCFORMK` is set, or `$MKSHELL` |
| Build system | mk itself (builds with mkfile) |
| Dependencies | Plan 9 libraries: bio (buffered I/O), fmt, regexp, utf |

#### Key Data Structures (from mk.h)

```c
/* ── Rule: a single mkfile rule ── */
typedef struct Rule {
    char    *target;    /* one target */
    Word    *tail;      /* constituents of targets */
    char    *recipe;    /* the recipe body */
    short   attr;       /* attributes (META, VIR, REGEXP, QUIET, etc.) */
    short   line;       /* source line */
    char    *file;      /* source file */
    Word    *alltargets;/* all the targets */
    int     rule;       /* rule number */
    Reprog  *pat;       /* compiled regexp for REGEXP rules */
    char    *prog;      /* custom comparison program */
    struct Rule *chain; /* hash chain per target */
    struct Rule *next;
    Shell   *shellt;    /* shell to use with this rule */
    Word    *shellcmd;
} Rule;

/* ── Node: a target in the dependency graph ── */
typedef struct Node {
    char    *name;
    long    time;       /* mtime */
    unsigned short flags; /* VIRTUAL, CYCLE, READY, MADE, etc. */
    Arc     *prereqs;   /* dependency edges */
    struct Node *next;  /* list for a rule */
} Node;

/* ── Arc: a dependency edge ── */
typedef struct Arc {
    short   flag;       /* TOGO */
    struct Node *n;     /* prerequisite node */
    Rule    *r;         /* rule that generated this edge */
    char    *stem;      /* stem from pattern match */
    char    *prog;      /* comparison program */
    char    *match[NREGEXP]; /* regexp submatches */
    struct Arc *next;
} Arc;

/* ── Job: one build job in flight ── */
typedef struct Job {
    Rule    *r;         /* master rule */
    Node    *n;         /* list of node targets */
    char    *stem;
    char    **match;
    Word    *p;         /* prerequistes */
    Word    *np;        /* new prerequistes */
    Word    *t;         /* targets */
    Word    *at;        /* all targets */
    int     nproc;      /* slot number */
    struct Job *next;
} Job;

/* ── Shell: abstraction for different recipe interpreters ── */
typedef struct Shell {
    char *name;
    char *termchars;     /* used in parse.c for assignment attributes */
    int  iws;            /* inter-word separator in environment */
    char *(*charin)(char*, char*);         /* find unescaped chars */
    char *(*expandquote)(char*, Rune, Bufblock*);  /* extract escaped token */
    int  (*escapetoken)(Biobuf*, Bufblock*, int, int); /* input escaped token */
    char *(*copyq)(char*, Rune, Bufblock*); /* check for quoted strings */
    int  (*matchname)(char*);              /* does name match? */
} Shell;
```

#### How the Parser Works (parse.c)

1. **Lexical line assembly** (`lex.c`): `assline()` reads runes from `Biobuf`,
   handling escaped newlines (backslash-newline → space for sh, elision for rc),
   quoting (`'`, `"`, `` ` ``), backquote shell substitutions, and comments (`#`).

2. **Rule header parsing** (`parse.c:rhead()`): Splits a line at the first
   `:`, `=`, or `<` delimiter. Parses attributes between two colons (e.g.,
   `target:V: prereq` → VIRTUAL attribute). Separates targets from prereqs.

3. **Recipe body** (`rbody()`): Assembles all subsequent indented lines into
   a single recipe string. Lines at column 0 end the recipe.

4. **Assignment handling**: `=` triggers variable assignment with shell
   attribute detection (currently only `U` for unexported).

5. **Include handling**: `< file` includes another mkfile; `<| command` pipes
   command output as mkfile input.

6. **Grammar**: Recursive descent based on a single `switch` on the
   delimiter character (`:`, `=`, `<`, `|`). No parser generator needed.

#### How Parallelism Works (run.c + mk.c)

The parallelism model is refreshingly simple:

1. `NPROC` variable controls parallelism (default 1). Set it dynamically.
2. A fixed-size `events[]` array holds running jobs indexed by slot.
3. `nproc()` allocates event slots; `nextslot()` finds a free one.
4. `run()` enqueues a job; `sched()` starts a job if slots are free.
5. `work()` is the core: it recursively walks the DAG, launching jobs
   for any node whose prereqs are all built.
6. `waitup()` reaps children and schedules more work.
7. Parallelism is achieved through `fork()`+`exec()` — each recipe runs
   in a separate process. No threads, no async.

Key: This is **implicitly parallel** — mk automatically parallelizes
independent dependency chains. No manual parallel recipes.

#### Shell Architecture (shell.c, rc.c, sh.c)

The `Shell` struct is an elegant **strategy pattern** in C:

- `shshell`: For POSIX sh — handles `\` escaping, `''` and `""` quoting
- `rcshell`: For Plan 9 rc — handles `''` quoting (rc has no `""`), single-quote
  doubling for literal quotes

Each shell has function pointers for:
- Finding special characters in strings (`charin`)
- Extracting escaped tokens (`expandquote`, `escapetoken`)
- Matching quoted strings (`copyq`)
- Determining if a shell name matches (`matchname` — `rcmatchname` checks for
  "rc" in the executable name)

Default shell is `sh`. Set `$MKSHELL` to switch, or `FORCERCFORMK` env var.

#### Key Design Decisions Visible in the Code

1. **Symtab for everything**: Symbol table (`symtab.c`) is a hash table used
   for variables, targets, nodes, mtimes, PIDs — everything. Unified lookup.

2. **Rule reuse**: If a rule with same target+prereqs already exists, it's
   reused (overwritten) rather than duplicated. Prevents ambiguity from parsed
   `mkfile` re-evaluations.

3. **DAG construction is recursive**: `applyrules()` recursively expands
   targets → prereqs → prereqs' prereqs, building the full graph in one pass.
   Simple but can blow the stack on deep dependency trees.

4. **Separate meta-rules and concrete rules**: `rules` list for concrete,
   `metarules` list for patterns. Both checked during graph construction.

5. **Pruning**: After graph construction, `vacuous()` and `ambiguous()` prune
   unneeded meta-rule edges. Elegant — build everything, then trim.

6. **No Makefile compatibility at all**: Not even a nod to GNU Make or BSD
   Make syntax. Clean break.

7. **Recipe variables at execution time**: `$target`, `$prereq`, `$stem`, etc.
   are set just before recipe execution via `buildenv()`.

8. **Signal handling**: `catchnotes()` translates POSIX signals to Plan 9-style
   notes and kills child processes.

### 3. dcjones/mk (Go) — First Go Port

| Attribute | Value |
|-----------|-------|
| Language  | Go |
| Author    | Daniel Jones (dcjones) |
| Year      | 2015 |
| LOC       | ~2,285 (7 .go files) |
| Recipe interpreter | sh (hardcoded), shell selectable via `S:` attribute |
| Repo      | github.com/dcjones/mk (one commit on master) |
| License   | MIT |
| Status    | Unfinished — TODO.md has many unimplemented features |

**Key architecture:**

- **Lexer**: Channel-based (`chan token`) with lexer state functions. Clean
  Go-style lexer (similar to Rob Pike's Go lexer template).
- **Parser**: State functions that transition on token type. Clean recursive
  descent without parser generator.
- **Graph**: `graph.go` implements `buildgraph()` — `applyrules()` recursively
  builds the DAG with `Rule`s, `Node`s, and `Edge`s.
- **Parallelism**: Go channels + goroutines. `mkNode()` walks the graph,
  launching goroutines for each prerequisite. `reserveSubproc()` uses
  `sync.Cond` for backpressure. Much cleaner than fork/exec.
- **Variable expansion**: `expand.go` handles `$var`, `${var}`, `${var:pat=sub}`,
  `${var:pat=%}` substitutions.
- **Regex rules**: Uses Go's `regexp` package (perl-like), not Plan 9 regex.

**Improvements over plan9port mk:**
- Parallel by default (default `-p=4`)
- Go regex (familiar to most devs)
- Blank lines allowed in recipes (like Python blocks)
- `S:` attribute for non-sh recipe shells
- Pretty colors

**Missing features (per TODO.md):**
- `${foo}` bracketed expansion
- Unit tests
- `$newprereq`, `$alltargets` variables
- Namelist syntax
- Environment variable import
- Man page

### 4. henesy/mk (Go) — Fork with rc Shell Support

| Attribute | Value |
|-----------|-------|
| Language  | Go |
| Author    | henesy |
| Year      | 2020 |
| LOC       | ~2,535 (same 7 .go files + .gitignore) |
| Repo      | github.com/henesy/mk |
| Status    | Single commit — a fork of dcjones with modifications |

**Changes from dcjones/mk:**
- Added `$shell` variable support — `$shell` in mkfile sets the recipe shell
- `defaultShell` global: `-s` flag (default `"sh -c"`)
- `dontDropArgs` flag (`-F`) for retaining shell args when no recipe takes them
- `expandShell()` function parses shell into cmd+args (handles `"sh -c"` →
  `["sh", "-c"]`, `"rc -v"` → `["rc", "-v"]`)
- `-C` flag for colorized output (colors off by default)
- Uses `$shell` variable from mkfile in backtick expansions

### 5. ctSkennerton/mk (Go) — Extended Fork with Remote Files

| Attribute | Value |
|-----------|-------|
| Language  | Go |
| Author    | Connor Skennerton (ctSkennerton) |
| Year      | 2020 |
| LOC       | ~3,273 (11 .go files + tests + man page) |
| Repo      | github.com/ctSkennerton/mk |
| License   | BSD 2-Clause |
| Status    | One commit, but includes tests, man page, testdata |

**Notable features:**
- **Remote/hTTP/S3 timestamps**: `remote.go` — `Node.updateTimestamp()` can
  fetch `Last-Modified` from HTTP URLs and S3 objects via AWS SDK. Wild.
- **`$prereq1`, `$prereq2`, etc**: Numbered prereq variables.
- **Environment variables**: Exports mkfile variables as process environment
  with `\x01` separator (rc-style array encoding).
- **Tests**: `expand_test.go`, `parse_test.go`, `mk_test.go`, `rules_test.go`
- **Man page**: `mk.1.md` — full Unix man page in markdown.
- **rc shell support**: Uses rc array separator (`\x01`) in environment.
- **`-nocolor`**: Auto-detects whether terminal supports color.
- **Bug fixes**: Fixed type in `equivRecipe()` receiver, quote parsing bugs.

### 6. hmk (Haskell) — Pure Haskell Port

| Attribute | Value |
|-----------|-------|
| Language  | Haskell (Literate Haskell — .lhs) |
| Author    | Mathieu Boespflug (mboes) |
| Year      | 2009–2016 |
| LOC       | ~704 (4 .lhs + Setup.hs) |
| Recipe interpreter | system shell (via `system`) |
| Repo      | github.com/mboes/hmk |
| License   | BSD 3-Clause |
| Status    | Incomplete — README says "alpha quality" |

**Architecture:**
- `Main.lhs`: CLI entry point, argument parsing
- `Parse.lhs`: Parser for mkfile syntax (uses Parsec)
- `Eval.lhs`: Dependency graph construction, rule matching, recipe execution
- `Metarule.lhs`: Meta-rule matching (pattern rules with `%`)

**Notable**: The smallest implementation. Uses lazy evaluation for graph
construction.

### 7. Standalone plan9port mk Builds

| Project | Description |
|---------|-------------|
| `razvanm/plan9mk` | Bazel build of plan9port's mk with extracted libs (libbio, libfmt, libregexp, libutf) |
| `RadioNoiseE/plan9mk` | Standalone build script for plan9port mk on Linux |

Both of these just extract the mk binary from plan9port, stripping the rest of
the Plan 9 userland. Useful if you want the real C mk without the rest of
plan9port.

### 8. Inferno / Vita Nuova mk (C, sh variant)

The Inferno OS (Vita Nuova) includes its own mk, written in a mix of C and
Limbo. It's a direct descendant of the Plan 9 mk.

**Key detail**: Vita Nuova produced a **standalone C version** of mk that replaced
`rc` with **`sh`** as the recipe interpreter — making it portable to Unix systems
without requiring the Plan 9 rc shell. This is the version that was:
- Distributed via the Quick C-- project (`cminusminus.org`)
- Packaged as the FreeBSD port `devel/mk` (version 1.5, added 2002)
- Used by Quick C-- as its build system

**Two variants existed**:
1. **sh version** — from Vita Nuova, used by Quick C--, the one in FreeBSD ports
2. **rc version** — available separately from the ports maintainer

**FreeBSD port details**:
- Port added: 2002-06-26 by William Josephson
- Version: 1.5
- Upstream: `cminusminus.org` (Quick C-- project)
- Status: BROKEN (unfetchable since 2015), DEPRECATED, EXPIRED 2016-07-04
- Installed: `bin/mk`, `man/man1/mk.1.gz`
- Patches:
  - `patch-src__Posix.c`: Removed `maketmp()` which used unsafe `tmpnam()`. Replaced with `mkstemp()`.
  - `patch-src__main.c`: Replaced `maketmp()`/`tmpnam()`/`create()` pattern with `mkstemp()` + `unlink()`. Fixed temp file handling for command-line assignments.

This version is significant because it proves mk **can work with sh** — the rc
requirement was never fundamental, just Plan 9-idiomatic. The plan9port mk later
took the same approach: default to sh, support rc via `$MKSHELL`.

### FreeBSD / OpenBSD mk

The user mentioned mk in FreeBSD repos. The FreeBSD base system does **not**
include mk. Searching the FreeBSD source tree (`freebsd/freebsd-src`) shows no
`usr.bin/mk` directory. However:

- **pkgsrc** (NetBSD's package system, also used on macOS via MacPorts) ships
  `devel/mk` which installs plan9port's mk.
- **Homebrew** has `plan9port` which includes mk.
- **OpenBSD ports** has `plan9port`.
- The original Plan 9 mk source from Bell Labs is mirrored at various
  locations (`plan9port/src/cmd/mk/`).

So mk is available on these systems via packages, not as a base system tool.

## What's Worth Porting / Redesigning for Rust

### Keep (port directly from C/Go):

| Feature | From | Notes |
|---------|------|-------|
| Shell strategy pattern (`Shell` struct) | plan9port C | Clean abstraction, port naturally to trait |
| Rule/Node/Arc graph model | plan9port C | Well-tested DAG structure |
| Meta-rule with `%` patterns | plan9port C | Core feature, must preserve semantics |
| Regex rule support (`R:` attribute) | dcjones Go | Use Rust `regex` crate |
| Pruning (vacuous + ambiguous) | plan9port C | Essential for meta-rule correctness |
| `$target`, `$prereq`, `$stem` variables | All | Must support at recipe execution time |
| Attribute system (`V`, `Q`, `U`, `D`, etc.) | plan9port C | Identical mapping to Rust enum |
| Parallel DAG traversal | dcjones Go | Channels/goroutines → Tokio/async |
| Recipe variable expansion | All | `${var}`, `${var:pattern=sub}` |
| Lexer state machine | dcjones Go | Clean design, Rust-friendly |
| Implicit parallelism | All | Auto-parallelize independent chains |

### Redesign / Improve:

| Area | Why change | Rust approach |
|------|-----------|---------------|
| **Shell abstraction** | C's function pointers work but are unsafe | `trait Shell` with `fn char_in()`, `fn expand_quote()` etc. Registry of built-in shells + custom shell scripts |
| **DAG building** | Recursive `applyrules` can stack-overflow | Iterative with explicit stack or work-stealing deque |
| **Memory management** | `Malloc`/`free` everywhere, leaks | Rust's ownership + `RefCell` for graph with cycles, or arena allocation |
| **Globals** | Plan 9 C uses global state everywhere (`rules`, `metarules`, `jobs`) | Pass `&BuildContext` through, or use thread-local with `tokio::task_local!` |
| **Error handling** | `fprint(2, ...); Exit()` — abort on error | `Result<T, MkError>` with structured errors, `-k` (keep-going) as default? |
| **Parallelism** | Fork/exec is heavy; Go version uses goroutines | `tokio::process::Command` + async task graph; `NPROC` as semaphore |
| **Regex** | Plan 9 regex is unusual; Go's is standard | Use `regex` crate; support both literal patterns and regex |
| **File timestamps** | `stat()` syscall per node | `mtime` via `std::fs::Metadata`, cached per graph |
| **Include system** | Recursive `parse()` calls | Stack of include contexts, protect against circular includes |
| **Assignment parsing** | Uses shell's `charin` to find assignment attributes | Separate cleanly — attribute tokens are mk syntax, not shell |
| **Environment** | `exportenv()` on fork; Go version lags | Build `HashMap<String, String>` from vars, pass to child process |
| **Remote files** | ctSkennerton's HTTP/S3 support is neat | `reqwest` for HTTP, optional S3 via feature flag |
| **Extensibility** | C has no plugin system | Possibly WASM-based recipe runners? Or just shell commands |
| **Tests** | C: minimal; Go: partial; Haskell: none | Comprehensive test suite from day one |

### Key Rust-Specific Design Choices:

1. **No global state**: `BuildContext` struct passed through all phases
2. **Arena allocation for graph nodes**: `typed_arena` or `generational-arena`
   for the DAG, using indices instead of raw pointers
3. **Enum for attributes**: `Attributes { meta, virtual, quiet, ... }` as
   bitflags or `struct Flags(u16)`
4. **Result-based error handling**: Collect non-fatal errors for `-k` mode
5. **Async task graph**: `tokio` with `Semaphore` for parallelism control
6. **Shell trait + built-ins**: `trait Shell` with `sh::Shell`, `rc::Shell`,
   and user-configurable via `$MKSHELL`
7. **Separate concerns**: Lexer → Parser → GraphBuilder → Scheduler → Executor
   as clear pipeline stages

### Minimal Viabl Product Scope:

Phase 1: Parse mkfile, build DAG, execute recipes serially with sh
Phase 2: Add parallel execution, `$stem`, `$target`, `$prereq`
Phase 3: Meta-rules (pattern matching, `%` patterns)
Phase 4: Regex rules, custom comparison programs (`P:` attribute)
Phase 5: rc shell support, remote files, full attribute set

## Sources

- plan9port source: `/tmp/plan9port/src/cmd/mk/`
- plan9port mk paper: `mk.pdf` (69KB, included in plan9port)
- dcjones/mk README: "Mk is a reboot of the Plan 9 mk command"
- henesty/mk: fork of dcjones with rc shell support
- ctSkennerton/mk: fork of dcjones with HTTP/S3 support, tests, man page
- mboes/hmk: "A pure Haskell implementation of Plan9's mk"
- "Maintaining Files on Plan 9 with Mk" (cat-v.org)
- Doc: http://doc.cat-v.org/plan_9/4th_edition/papers/mk
