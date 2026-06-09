# plan9port mk — C implementation code map

> Based on commit from `https://github.com/9fans/plan9port`, `src/cmd/mk/`.
> Total: ~3,700 LOC across 22 .c files and 4 headers.

---

## File inventory

| File | LOC | Purpose |
|------|-----|---------|
| `mk.h` | 140 | Master header: all struct typedefs, flags, externs, macros, function declarations |
| `fns.h` | 64 | Additional function declarations (split from mk.h for readability) |
| `sys.h` | 4 | Plan 9 system include shim (`<u.h>`, `<libc.h>`, `<bio.h>`, `<regexp.h>`) |
| `sys.std.h` | 24 | Standard C (POSIX) system includes, used when `NOPLAN9DEFINES` is set |
| `main.c` | 287 | Entry point: argument parsing, assignment handling, target dispatch, memory wrappers |
| `mk.c` | 234 | Core build loop: `mk()`, `work()`, `update()`, `outofdate()`, `clrmade()` |
| `lex.c` | 146 | Line assembly: `assline()` — escapes, backquotes, comment elision, continuation |
| `parse.c` | 318 | Rule/assignment/include parser: `parse()`, `rhead()`, `rbody()`, `addrules()`, `ipush/pop()` |
| `graph.c` | 279 | DAG construction: `graph()`, `applyrules()`, `newnode()`, `cyclechk()`, `ambiguous()`, `vacuous()`, `attribute()` |
| `rule.c` | 112 | Rule storage: `addrule()`, `rcmp()`, `dumpr()`, `rulecnt()` |
| `run.c` | 296 | Parallel execution: `run()`, `sched()`, `waitup()`, `nproc()`, `nextslot()`, `killchildren()`, `prusage()` |
| `job.c` | 33 | Job construction: `newjob()`, `dumpj()` |
| `recipe.c` | 122 | Recipe dispatch: `dorecipe()` — builds node list, Job parameters, calls `run()` |
| `var.c` | 41 | Variable setting: `setvar()`, `dumpv()`, `shname()` |
| `varsub.c` | 252 | Variable expansion: `varsub()`, `expandvar()`, `varmatch()`, `subsub()`, `submatch()` |
| `symtab.c` | 93 | Symbol table: `symlook()`, `symdel()`, `syminit()`, `symtraverse()`, `symstat()` |
| `env.c` | 150 | Environment construction: `buildenv()`, `initenv()`, `execinit()`, `envupd()`, `ecopy()` |
| `shell.c` | 82 | Shell abstraction management: `setshell()`, `initshell()`, `pushshell()`, `popshell()` |
| `sh.c` | 205 | Bourne shell implementation: `shcharin()`, `shexpandquote()`, `shescapetoken()`, `shcopyq()`, `shmatchname()` |
| `rc.c` | 194 | rc shell implementation: `rccharin()`, `rcexpandquote()`, `rcescapetoken()`, `rccopyq()`, `rcmatchname()` |
| `shprint.c` | 125 | Recipe printing with variable expansion: `shprint()`, `vexpand()`, `mygetenv()`, `front()` |
| `word.c` | 189 | Word list operations: `stow()`, `newword()`, `wtos()`, `wdup()`, `delword()`, `nextword()` |
| `bufblock.c` | 88 | Dynamic buffer (with free-list pool): `newbuf()`, `freebuf()`, `growbuf()`, `insert()`, `rinsert()`, `bufcpy()` |
| `arc.c` | 52 | Arc construction: `newarc()`, `dumpa()`, `nrep()` |
| `archive.c` | 272 | Archive (`.a`) member handling: `atimeof()`, `atouch()`, `atimes()`, `split()` |
| `file.c` | 90 | File time operations: `mtime()`, `timeof()`, `touch()`, `delete()`, `timeinit()` |
| `match.c` | 49 | Pattern matching: `match()` (% and & pattern matching), `subst()` |
| `unix.c` | 347 | OS-specific: `execsh()`, `pipecmd()`, `waitfor()`, `exportenv()`, `readenv()`, `catchnotes()`, `maketmp()`, `chgtime()`, `rcopy()`, `mkmtime()` |

---

## Data structure map

### `Bufblock` (bufblock.c, declared mk.h)
Dynamic growing buffer with free-list pool and `current` insertion point.

```c
typedef struct Bufblock {
    struct Bufblock *next;   // free-list link
    char             *start;  // buffer base
    char             *end;    // end of allocated memory
    char             *current; // insertion point (append here)
} Bufblock;
```

- **Mechanism**: Append-only. `insert()` writes one byte at `current++`, calls `growbuf()` if full.
- **Pool**: `freebuf()` returns the block to a singly-linked freelist. `growbuf()` first tries to swap with a freelist buffer of sufficient size before calling `realloc()`.
- **Used everywhere**: lexing, string building, variable expansion, recipe print.
- **Worth porting**: The freelist pool and grow-by-swapping pattern.

### `Word` (word.c, declared mk.h)
Singly-linked list of strings — mk's universal collection type.

```c
typedef struct Word {
    char         *s;      // the string value
    struct Word  *next;   // next in list
} Word;
```

- **Used for**: rule prerequisites, targets, variable values, environment entries.
- **Key operations**: `stow()` splits a string by whitespace into a Word list; `wtos()` joins back; `wdup()` deep copies; `delword()` frees the chain.
- **Edge case**: `stow("")` returns a Word with `s=""` (non-NULL empty string).
- **Bug/oddity**: `nextword()` in word.c handles `$` variable expansion inline — mixing tokenization with expansion.

### `Symtab` (symtab.c, declared mk.h)
Single hash table with namespaced entries.

```c
typedef struct Symtab {
    short            space;    // namespace enum (S_VAR, S_TARGET, S_TIME, etc.)
    char             *name;    // key string
    union {
        void         *ptr;     // general pointer (Word*, Rule*, Node*)
        uintptr      value;    // integer timestamp
    } u;
    struct Symtab    *next;    // hash chain
} Symtab;
```

- **Hash**: 4099 buckets, multiplicative hash (`h = space; for each char: h += c; h *= 79`).
- **Namespaces** (from mk.h):
  - `S_VAR` — variable name → Word* value
  - `S_TARGET` — target name → Rule* chain
  - `S_TIME` — file name → timestamp (long)
  - `S_PID` — pid → products
  - `S_NODE` — target name → Node*
  - `S_AGG` — archive name → timestamp
  - `S_BITCH` — bitched-about archive name (suppress warnings)
  - `S_NOEXPORT` — variable name → "" (don't export)
  - `S_OVERRIDE` — variable name → "" (can't override via mkfile)
  - `S_OUTOFDATE` — `"target\377prereq"` → 1 or 2 (cached out-of-date)
  - `S_MAKEFILE` — target → Node
  - `S_MAKEVAR` — dumpable mk variable
  - `S_EXPORTED` — variable → current exported value
  - `S_WESET` — variable; we set in the mkfile
  - `S_INTERNAL` — internal mk variable (stem, target, etc.)
- **`symlook(name, space, install)`**: returns existing Symtab or creates one if `install != NULL`.
- **Reentrancy**: None — single global `hash[NHASH]`. Fine for mk's single-threaded model.
- **Worth porting**: Namespaced hash table. The union approach can be `enum` + generic value in Rust.

### `Rule` (rule.c, declared mk.h)
A single mk rule (target: prereq1 prereq2 ... recipe).

```c
typedef struct Rule {
    char         *target;      // one target name
    Word         *tail;        // prerequisite list
    char         *recipe;      // recipe body (string)
    short        attr;         // attributes bitmap
    short        line;         // source line number
    char         *file;        // source file name
    Word         *alltargets;  // all targets (for multi-target rules)
    int          rule;         // rule number (unique index)
    Reprog       *pat;         // compiled regexp (for % or REGEXP rules)
    char         *prog;        // out-of-date comparison program
    struct Rule  *chain;       // hashed per target (next in symtab chain)
    struct Rule  *next;        // global rule list link
    Shell        *shellt;      // shell to use with this rule
    Word         *shellcmd;    // shell command (argv[0])
} Rule;
```

- **Attributes** (`attr`): META(meta-rule), UNUSED, UPD, QUIET, VIR(virtual), REGEXP, NOREC(no recipe), DEL(delete on error), NOVIRT.
- **Storage model**: All rules in two global linked lists: `rules` (non-meta) and `metarules`. Each rule also in a per-target hash chain via `symlook(target, S_TARGET, ...)`.
- **`addrule()`**: Deduplicates by target+tail match (reuses same Rule struct). Adds to both global list and target chain.
- **Meta-rules**: Rules with `%`, `&`, or `REGEXP` attribute go into `metarules` list, not `rules`.
- **Multi-target**: When `T1 T2: P`, all targets stored in `alltargets`. Each gets its own Rule with same tail/recipe.

### `Arc` (arc.c, declared mk.h)
A directed edge from a Node (target) to either another Node (prerequisite) or null.

```c
typedef struct Arc {
    short      flag;               // TOGO (marked for pruning)
    struct Node *n;                // prerequisite node (NULL for recipe-less edges)
    Rule       *r;                 // rule that generated this edge
    char       *stem;              // matched stem (for %/& rules)
    char       *prog;              // out-of-date comparison program
    char       *match[NREGEXP];    // regexp submatches (10 slots)
    struct Arc *next;              // linked list of prereqs for a node
} Arc;
```

- **`flag`**: `TOGO` — marked for deletion during pruning.
- **`prog`**: Custom out-of-date program via `:P` attribute.
- **`stem`**: The %-matched stem for pattern rules.
- **`match[NREGEXP]`**: Captured regex substrings for `REGEXP` rules.
- **Lifecycle**: Created during `applyrules()`, sometimes removed by `togo()` after vacuous/ambiguous pruning.

### `Node` (graph.c, declared mk.h)
One node in the DAG — a target file or virtual target.

```c
typedef struct Node {
    char        *name;          // target name (file path)
    long        time;           // modification time (0 = doesn't exist)
    unsigned short flags;       // state flags
    Arc         *prereqs;       // linked list of prerequisite arcs
    struct Node *next;          // linked list for multi-target rule grouping
} Node;
```

- **Flags**: VIRTUAL, CYCLE, READY, CANPRETEND, PRETENDING, NOTMADE, BEINGMADE, MADE, PROBABLE, VACUOUS, NORECIPE, DELETE, NOMINUSE, ONLIST.
- **State machine**: NOTMADE → BEINGMADE → MADE (via `MADESET` macro). Pretending state allows skipping rebuild.
- **`time`**: 0 means "doesn't exist" or "never been checked." After recipe runs, gets current time.
- **`next`**: Used only in `dorecipe()` to link co-targets of a multi-target rule.

### `Job` (job.c, declared mk.h)
One unit of parallel work — a rule with its instantiation context.

```c
typedef struct Job {
    Rule        *r;        // master rule
    Node        *n;        // list of target nodes
    char        *stem;     // pattern stem
    char        **match;   // regexp match array
    Word        *p;        // all prerequisites
    Word        *np;       // new/out-of-date prerequisites
    Word        *t;        // targets (those that need building)
    Word        *at;       // all targets (full list)
    int         nproc;     // slot number
    struct Job  *next;     // linked list of pending jobs
} Job;
```

- **Created by**: `dorecipe()` → `newjob()` → `run()`.
- **`stem`/`match`**: Passed through from the Arc that triggered the recipe, used for `$stem` and `$stem0-9` variable expansion.
- **`t` vs `at`**: `t` is the subset of targets that actually need rebuilding; `at` is all targets of the rule.
- **Scheduling**: Jobs are linked into `jobs` global list. `sched()` pops from head, `waitup()` may push more via `sched()`.

### `Envy` (env.c, declared mk.h)
Environment variable entry for recipe execution.

```c
typedef struct Envy {
    char      *name;     // variable name
    Word      *values;   // multi-value list (Word list)
} Envy;
```

- Stored as a flat array (`envy[]`) terminated by `{NULL, NULL}`.
- Built per-job by `buildenv()`: includes all `S_VAR` entries (except noexport/internal), plus job-specific: target, stem, prereq, pid, nproc, newprereq, alltarget, newmember, stem0-9.

### `Shell` (shell.c, declared mk.h)
Function-pointer table abstracting shell syntax.

```c
typedef struct Shell {
    char *name;                                  // "sh" or "rc"
    char *termchars;                             // assignment attribute terminators
    int   iws;                                   // inter-word separator in env
    char *(*charin)(char*, char*);               // search for unescaped chars
    char *(*expandquote)(char*, Rune, Bufblock*); // extract escaped token (string)
    int   (*escapetoken)(Biobuf*, Bufblock*, int, int); // input escaped token
    char *(*copyq)(char*, Rune, Bufblock*);      // check/copy quoted strings
    int   (*matchname)(char*);                   // does shell name match?
} Shell;
```

- Two instances: `shshell` and `rcshell`. Default is `shshell` (unless `FORCERCFORMK` env var).
- Managed via `setshell()`, `pushshell()`, `popshell()` (stack-based scope).

---

## Feature → code trace

### Parser (lex.c + parse.c)

#### `lex.c`
- **`assline()`** — Line assembler. Reads runes from `Biobuf`, handles:
  - `\r` → skip (Windows compat)
  - `\n` → end of line
  - `\`, `'`, `"` → delegates to `shellt->escapetoken()` for shell-specific escape handling
  - `` ` `` → `bquote()` for backquote shell execution
  - `#` → comment (skip to `\n`). **Bug/edge case**: if the character before `\n` was `\`, comment is treated as continuation (the `lastc == '\\'` check)
- **`bquote()`** — Backquote execution. Reads `` `{...} `` (rc style) or `` `...` `` (sh style), executes via `execsh()`, captures stdout into line buffer.
- **`nextrune()`** — Get next rune, handling `\` + `\n` line continuation (elide or replace with space). Also tracks `mkinline` for error messages.

#### `parse.c`
- **`parse()`** — Top-level parse loop. For each assembled line:
  - Calls `rhead()` to classify as `:`, `=`, `<`, or `|`
  - `:` → rule: `rbody()` reads recipe body, then `addrules()` 
  - `=` → assignment: `setvar()`, handles `MKSHELL` specially, tracks overrides
  - `<` → include: recursive `parse()` on included file
  - `|` → pipe include: `pipecmd()` then `parse()` on pipe fd
- **`rhead()`** — Rule head parser. Uses `shellt->charin()` to find separator (`:=<`). Parses attributes (D, E, n, N, P, Q, R, U, V) between `:` pairs. Assignment attributes (`U`) after `=` via `shellt->termchars`.
- **`rbody()`** — Reads recipe body: indented lines after rule head. Stops at non-indented, non-comment line.
- **`pushshell()`/`popshell()`** — Save/restore shell context for file scoping.

### Graph construction (graph.c + arc.c + rule.c)

#### `graph.c`
- **`graph(target)`** — Entry point. Calls `applyrules()`, then `cyclechk()`, marks as PROBABLE, runs `vacuous()`, `ambiguous()`, `attribute()`.
- **`applyrules(target, cnt)`** — Recursive DAG builder. Uses `S_NODE` symtab as memoization (returns existing Node if already built). For each matching rule:
  1. Direct rules from `symlook(target, S_TARGET, ...)`
  2. Pattern rules from `metarules` list (tries `regexec()` for REGEXP or `match()` for %/&)
  3. Respects `nreps` limit via `cnt[]` array
  4. Creates arcs via `newarc(applyrules(prereq_name), rule, stem, rmatch)`
  5. **Bug/Edge case**: The `cnt[r->rule]++/--` around recursion prevents infinite loops from self-referential pattern rules
- **`newnode(name)`** — Allocates Node, installs in `S_NODE`, queries `timeof()` for mtime.
- **`cyclechk(n)`** — DFS cycle detection using `CYCLE` flag.
- **`ambiguous(n)`** — Detects conflicting recipes from different rules for same target. Prefers non-meta rules over meta. Exit on ambiguity.
- **`vacuous(n)`** — Marks arcs as `TOGO` if they come from meta-rules that contributed no real (non-vacuous) prerequisites.
- **`attribute(n)`** — Propagates VIRTUAL, NORECIPE, DELETE flags from arcs to node.

#### `arc.c`
- **`newarc()`** — Allocates Arc, copies stem and regexp match array. `prog` is taken from `r->prog`.
- **`nrep()`** — Reads `NREP` variable, updates global `nreps`.

#### `rule.c`
- **`addrule()`** — Adds rule to both global list (`rules`/`metarules`) and target chain. Deduplicates by target+tail comparison.
- **`rulecnt()`** — Allocates zeroed array of length `nrules` for recursion counting.

### Pruning (graph.c: vacuous/ambiguous/togo)

- **`vacuous()`** — Bottom-up: a node is vacuous if it has PROBABLE, all its prerequisites are vacuous, AND all its active arcs come from META rules. Non-vacuous prerequisites cause sibling META-generated arcs to also be kept (the "all-or-nothing" loop at end).
- **`togo()`** — Physically removes arcs with `TOGO` flag from the prereq linked list.
- **`ambiguous()`** — Compares recipes across arcs. If two different rules provide different recipes, it's an error. Meta-rule recipes lose to non-meta recipes.

### Parallel execution (run.c + job.c + recipe.c)

#### `run.c`
- **`run(j)`** — Appends job to `jobs` global list. Calls `sched()` if slots available.
- **`sched()`** — Pops job from `jobs` head. Calls `buildenv()` + `shprint()` (to print recipe). Forks `execsh()`. Tracks in `events[slot]`. If `nflag`/`tflag`, simulates without forking.
- **`waitup(echildok, retstatus)`** — Waits for child via `waitfor()`. Processes status: on error, prints recipe (via `front()` truncation), optionally deletes targets (`DELETE` attr), exits or continues based on `kflag`. Calls `update()` for completed nodes, then `sched()` if slots available.
- **`nproc()`** — Reads `NPROC` variable, resizes `events[]` array. Defaults to 1.
- **`nextslot()`** — Linear scan for free slot (pid <= 0).
- **`killchildren()`** — Kills all tracked child processes on signal.
- **No work stealing**: Simple FIFO job queue with static slot allocation.

#### `job.c`
- **`newjob()`** — Simple allocator and field setter.

#### `recipe.c`
- **`dorecipe()`** — The bridge between graph/pruning and execution:
  1. Finds the first arc with a non-empty recipe
  2. If no recipe: handles virtual targets, archive members, touch
  3. Builds co-target list (`node->next` linked list) from `alltargets`
  4. Collects prerequisites (`lp`) and out-of-date prereqs (`ln`) via `addw()` (deduplicated)
  5. Marks targets as `BEINGMADE`
  6. Calls `run(newjob(...))`

### Variable expansion (var.c + varsub.c + shprint.c)

#### `var.c`
- **`setvar()`** — Sets a variable: `symlook(S_VAR)` + `symlook(S_MAKEVAR)`.
- **`shname()`** — Scans a string to find end of a shell variable name.

#### `varsub.c`
- **`varsub()`** — Entry for `$var` expansion. Handles `{...}` or bare name.
- **`varmatch()`** — Looks up `S_VAR`, returns first non-empty value as Word list.
- **`expandvar()`** — Handles `${var: A%B==C%D}` substitution patterns. If no `:`, simple lookup. If `:`, parses pattern up to `}` and calls `subsub()`.
- **`subsub()`** — Applies `subst`-style pattern to each Word in variable value. For each value word:
  - Match A (prefix) and B (suffix)
  - If match: emit C + (mid between A and B) + D
  - If no match: emit original word unchanged
- **`extractpat()`** — Extracts pattern segments separated by `=`, `%`, `&`.
- **`submatch()`** — Checks if value word matches A (prefix) and optionally B (suffix).

#### `shprint.c`
- **`shprint()`** — Expands `$var` in recipe string before printing/execution. Calls `vexpand()` for `$`, otherwise copies runes through `shellt->copyq()` for quote handling.
- **`vexpand()`** — Resolves `${var}` or `$var` against the per-job `Envy` array. Only resolves variables marked `S_WESET` or `S_INTERNAL`.
- **`front()`** — Truncates recipe to first 5 fields for error display. Used to keep error messages short.

### Shell abstraction (shell.c, sh.c, rc.c)

#### `shell.c`
- **`setshell()`** — Matches shell name via `matchname()` callback against `shshell`/`rcshell`. Updates global `shellt` and `shellcmd`.
- **`initshell()`** — Sets default shell (sh, unless `FORCERCFORMK`). Calls `setvar("MKSHELL", ...)`.
- **`pushshell()`/`popshell()`** — Stack-based save/restore for file-scoped `MKSHELL`.

#### `sh.c` — Bourne shell backend
- **`shcharin()`** — Scan for unescaped chars. Respects `\'"` quoting, `$` variable gen (`${...}`).
- **`shexpandquote()`** — Extract escaped token from string. `\` escapes next char; `'` and `"` copy until matching close.
- **`shescapetoken()`** — Read escaped token from `Biobuf`. Same quote rules.
- **`shcopyq()`** — Copy quoted string, handling `` ` `` backquote scoping.
- **`shmatchname()`** — Always returns 1 (any name matches "sh").

#### `rc.c` — rc shell backend
- **`rccharin()`** — Scan for unescaped chars. Only `'` is a quote (not `"`). `'` quotes are doubled: `''` = literal `'`.
- **`rcexpandquote()`** — Only `'` is special; `\` and `"` pass through literally.
- **`rcescapetoken()`** — `'` toggles quote mode; doubled `''` is literal `'`.
- **`rccopyq()`** — Only `'` is a quote; `` ` `` ends at `}` instead of `` ` ``.
- **`rcmatchname()`** — Returns 1 if basename starts with "rc".
- **Inter-word separator**: rc uses `\1` instead of `' '` for joining multi-word values in environment.

### Environment (env.c + unix.c: exportenv)

#### `env.c`
- **`initenv()`** — Registers internal variables (target, stem, prereq, pid, nproc, etc.), calls `readenv()`.
- **`execinit()`** — Builds initial `envy[]` array: internal vars (empty), then all `S_VAR` entries via `ecopy()`, excluding `S_NOEXPORT` and internal names.
- **`buildenv()`** — Per-job environment: updates `target`, `stem`, `prereq`, `pid`, `nproc`, `newprereq`, `alltarget`, `newmember`, `stem0`-`stem9`.

#### `unix.c`
- **`exportenv()`** — Converts `Envy[]` to POSIX `environ` array. For rc, skips empty values and uses `\1` as separator.
- **`readenv()`** — Parses `environ` into `S_VAR` entries, skipping `S_INTERNAL` names.

### Recipe execution (recipe.c + unix.c: execsh)

#### `unix.c: execsh()`
- Forks twice (double-fork pattern). First child:
  - Pipes stdin from second child (which writes the recipe)
  - Sets up stdout to pipe back if `buf != NULL` (backquote)
  - Exports environment via `exportenv()`, execs shell
- Second child: writes recipe to pipe stdin, exits.
- Parent: optionally reads recipe output into buf (backquote).
- **Bug/edge case**: No explicit `execvp()` error handling in second child if write fails early.

#### `unix.c: pipecmd()`
- Forks, execs shell with `-c cmd`, optionally captures stdout to fd.
- Used for `|include` directive.

### Archives (archive.c)

- **`atimeof()`** — Gets archive member timestamp. Caches per-archive in `S_AGG`. Rereads member times if archive mtime changed.
- **`atimes()`** — Reads archive format (BSD `#1/`, GNU `//`, POSIX). Creates `S_TIME` entries for each member.
- **`split()`** — Parses `archive(member)` syntax. Checks archive type via magic bytes.
- **`atouch()`** — Updates member timestamp in archive by seeking and overwriting date field.

### Main (main.c)

- **`main()`** — Argument parsing: `-a` (all), `-d[egp]` (debug), `-e` (explain), `-f` (mkfile), `-i` (ignore errors), `-k` (keep going), `-n` (dry run), `-s` (sequential targets), `-t` (touch), `-u` (usage stats), `-w` (pretend time).
- **Assignment args**: Command-line `VAR=value` args are written to temp file and parsed as assignments.
- **`MKFLAGS`** and `**MKARGS**` are set.
- **Default target**: First non-meta rule encountered during parsing.

### Symbol table (symtab.c)

- **Hash function**: `h = space; for each char: h += c; h *= 79; h %= 4099`
- **`symlook()`**: Linear search in bucket, insert at head if `install != NULL`.
- **`symdel()`**: Removes from hash chain (but leaks the name string).
- **`symtraverse()`**: Iterates all buckets, calling callback for matching space.

### Utilities (word.c, bufblock.c)

#### `word.c`
- **`stow(s)`** — Tokenizes string by whitespace, handling shell quoting and `$var` expansion via `nextword()`.
- **`nextword()`** — Stateful tokenizer. Handles `\'"` quoting (via `shellt->expandquote`), `$var` expansion (via `varsub()`), and whitespace separation. **This is where tokenization meets variable expansion** — a key design decision.
- **`wtos()`** — Joins Word list with separator.
- **`wdup()`** — Deep copy of Word chain.
- **`delword()`** — Frees entire chain.

#### `bufblock.c`
- **Pool-based allocator** with grow-by-swapping strategy. `QUANTA=4096`.
- **`growbuf()`**: First tries to find a freelist buffer of sufficient size and swap; only falls back to `realloc()` if none found.

---

## What's worth porting directly

### Algorithms
1. **Symbol table (symtab.c)** — Namespace-partitioned hash table. The `(name, space)` key pair maps cleanly to `HashMap<(Space, String), Entry>`. Space enum is easy to define.
2. **Bufblock free-list pool** — The grow-by-swapping freelist is a nice zero-fragmentation pattern. Can become `Vec<u8>` with a custom allocator pool.
3. **`match()` + `subst()`** — Pattern matching for `%`/`&` in 49 lines. Trivial to port.
4. **`outofdate()` caching** — The `S_OUTOFDATE` cache for custom `:P` program results.
5. **`addw()` dedup** — Simple set-before-add for prerequisite lists.
6. **Word list operations** — `wdup()`, `wtos()`, `delword()` all straightforward.

### Constants and enums
7. **Decision table**: `Shell` function pointer struct → Rust trait (see below).
8. **Rule attributes** → `bitflags!` crate.
9. **Node state flags** → `bitflags!` crate.
10. **Symtab spaces** → `enum Space`.

### Small standalone functions
11. **`rcopy()`** — Regexp match array copy.
12. **`rulecnt()`** — Allocation + zeroing.
13. **`front()`** — String truncation for error display.

---

## What should be redesigned

### Global state (high priority)
| Current | Problem |
|---------|---------|
| `rules`, `metarules`, `patrule` globals | Thread-unsafe, untestable |
| `jobs` global list | Should be owned by Scheduler |
| `events[]` global array | Should be part of Scheduler |
| `hash[NHASH]` static | Should be `struct Symtab` |
| `envy[]` global with `nextv` | Stateful builder pattern needed |
| `infile`, `mkinline` globals | Pass through ParseContext |
| `shellt`, `shellcmd` globals | Part of MkContext |

**Fix**: Everything should live in a `struct Mk { ... }` or a few owned contexts (`ParseContext`, `Graph`, `Scheduler`, `Symtab`).

### Recursive DAG building (graph.c: applyrules)
The `applyrules()` function is recursive, uses a mutable `cnt[]` array for cycle/repetition counting, and relies on global `S_NODE` symtab for memoization. In Rust:
- Use `HashMap<String, NodeId>` + iterative worklist
- Or use recursive function with `RefCell` for memo cache
- The `cnt[]` increment/decrement pattern is error-prone — better to use a `HashMap<usize, usize>` or pass accumulated state

### Manual memory management
- Word lists, Arcs, Nodes are all individually `malloc`'d with no ownership tracking.
- Rust solution: `Rc<Rule>` for shared rule references, `Vec<Node>`/`Vec<Arc>` with indices, `Box<Word>` for linked lists.
- The freelist pool for Bufblock can become a `Vec<Vec<u8>>` reuse pool.

### Shell function pointers → trait
The C `Shell` struct with function pointers maps directly to a Rust trait (see below).

### Linked lists everywhere
C uses singly-linked lists for everything (Word, Arc, Rule chain, jobs). In Rust:
- Prefer `Vec` or `LinkedList` where applicable
- Word lists: `Vec<String>` is simpler and cache-friendly
- Arc lists: `Vec<Arc>` is cleaner than manual linked list with `togo()` removal
- Job queue: `VecDeque<Job>`

### The `nextword()` mixing of tokenization and expansion
`word.c:nextword()` simultaneously tokenizes by whitespace AND expands `$var` references. This makes it complex and error-prone. In Rust, separate:
1. Lex into tokens (words, quotes, variable references)
2. Expand variables
3. Join/split as needed

### Event/slot system (run.c)
The `events[]` array with linear slot search is O(n) for both allocation and lookup. In Rust:
- Use `HashMap<Pid, JobSlot>` or slab allocator
- Replace Process linked list with `HashMap`

### Double-fork in execsh
The `execsh()` double-fork (one for shell, one for stdin writer) is a Unix-ism. In Rust, use `std::process::Command` with piped stdin.

---

## Shell trait design

Based on the C `Shell` struct, here's a proposed Rust trait:

```rust
/// Shell syntax abstraction — controls how mk parses, expands, and quotes
/// text for a particular shell (sh, rc, fish, etc.).
pub trait Shell: fmt::Debug {
    /// Display name (e.g. "sh", "rc").
    fn name(&self) -> &str;

    /// Characters that terminate an assignment attribute section in `=U=` syntax.
    /// For sh: `"\"'= \t"`, for rc: `"'= \t"`.
    fn termchars(&self) -> &str;

    /// Inter-word separator used when joining multi-valued variables for the
    /// environment. For sh: `' '`, for rc: `'\x01'`.
    fn inter_word_separator(&self) -> char;

    /// Find the first unescaped character in `s` that belongs to `pattern`.
    /// Must respect shell-specific quoting rules.
    fn find_unescaped<'a>(&self, s: &'a str, pattern: &str) -> Option<&'a str>;

    /// Extract an escaped token from a string `s` where the opening escape
    /// character `esc` has already been consumed. Returns the rest of the string,
    /// or None on error.
    fn expand_quote<'a>(&self, s: &'a str, esc: char, buf: &mut String) -> Option<&'a str>;

    /// Read an escaped token from a reader `r` after the opening escape character
    /// `esc` has been read. Returns whether the token was closed successfully.
    fn escape_token<R: BufRead>(&self, r: &mut R, esc: char, buf: &mut String) -> bool;

    /// Copy a quoted string starting at `s` (past the opening quote `quote_char`).
    /// Handles nested quoting. Returns the position after the closing quote.
    fn copy_quoted<'a>(&self, s: &'a str, quote_char: char, buf: &mut String) -> &'a str;

    /// Returns true if `shell_name` matches this shell (e.g., basename starts with "rc").
    fn matches_name(&self, shell_name: &str) -> bool;
}
```

If you want to keep function pointers for dynamic dispatch without trait objects boxed everywhere:
```rust
pub struct ShellVTable {
    pub name: &'static str,
    pub termchars: &'static str,
    pub iws: char,
    pub find_unescaped: fn(&str, &str) -> Option<&str>,
    pub expand_quote: fn(&str, char, &mut String) -> Option<&str>,
    pub escape_token: fn(&mut dyn BufRead, char, &mut String) -> bool,
    pub copy_quoted: fn(&str, char, &mut String) -> &str,
    pub matches_name: fn(&str) -> bool,
}
```

But trait objects are more idiomatic:

```rust
// Concrete implementations
pub struct ShShell;
pub struct RcShell;

impl Shell for ShShell { ... }
impl Shell for RcShell { ... }
```

### Shell stack (scoped MKSHELL)

```rust
pub struct ShellStack {
    stack: Vec<(Box<dyn Shell>, Vec<String>)>, // shell + shell command words
}

impl ShellStack {
    pub fn push(&mut self, shell: Box<dyn Shell>, cmd: Vec<String>);
    pub fn pop(&mut self) -> Option<(Box<dyn Shell>, Vec<String>)>;
}
```

---

## Key edge cases and bugs found

1. **Comment continuation** (lex.c:57-60): If a `#` line ends with `\` before `\n`, the next line is treated as a continuation — this probably unintentional for comments.

2. **cnt[] recursion gating** (graph.c: `applyrules`): The `cnt[r->rule]++` is done before iterating prerequisites and `--` after. This means it gates both direct and indirect recursion of the same rule, but due to `++/--` it can undercount if exceptions occur.

3. **symdel() memory leak** (symtab.c: `symdel`): The comment says "multiple memory leaks" — the name string and the entry itself are leaked. The function exists but is only called from `syminit()` which clears everything.

4. **Archive split() modifies input** (archive.c `split()`): Mutates the original string by writing `\0` at `(` and `)` positions. Callers must pass a mutable copy.

5. **execsh() double-fork**: The second child (recipe writer) uses `_exit(0)` regardless of write errors. If write fails, the shell child's stdin just closes.

6. **rc charin vargen scoping** (rc.c: `rccharin`): The `$` detection uses `*(cp+1)` which is a raw pointer dereference past current rune — could read past string end if `$` is at the last byte before `\0` (unlikely with proper UTF-8 but fragile).

7. **`stow("")` returns a non-empty Word**: `newword("")` is used as fallback, which means empty strings get a Word struct with `s=""` (valid pointer, empty content). Callers must check `*s` not just `s`.

8. **`addrule()` dedup logic**: If a rule with same target+tail is encountered, it reuses the Rule struct but doesn't update `alltargets`. This means multiple rules with identical target+tail but different recipe silently drop later recipes.

9. **NREP/NPROC dynamic update**: `nproc()` and `nrep()` are called at the start of each `mk()` call, allowing these variables to be changed mid-build by other rules' execution.
