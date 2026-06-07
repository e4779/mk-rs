# 9base Research — Focus on mk

> Source: https://git.suckless.org/9base/ — "revived minimalist port of Plan 9 userland to Unix"

## 1. What is 9base?

A minimal port of Plan 9 userland tools to Unix, created by **Anselm R Garbe** (suckless.org founder). Version 7, last commit 2019-09-11.

**Included tools** (from `Makefile` SUBDIRS):

| Tool | Purpose |
|------|---------|
| `lib9` | Plan 9 libc compatibility layer |
| `yacc` | Parser generator |
| `rc` | The Plan 9 shell |
| **`mk`** | **Plan 9's build tool (main focus)** |
| `sam`, `ssam` | Text editors |
| `sed`, `awk`, `grep` | Text processing |
| `bc`, `dc`, `hoc` | Calculators |
| `diff` | File comparison |
| `ed` | Line editor |
| `troff` | Typesetter |
| `ls`, `cat`, `echo`, `rm`, `cp?` | File utilities |
| `fortune`, `freq`, `getflags` | Misc utilities |
| ~60 subdirectories total | Many small Plan 9 tools |

**Philosophy**: Take the original Plan 9 source (via plan9port), strip it to essentials, make it compile with a simple `config.mk` + `make`. No autotools. No complex build. Static linking by default.

## 2. How is mk implemented in 9base?

### Source structure (27 .c files + 3 headers)

```
mk/
├── sys.h          # Just 5 includes: <u.h>, <libc.h>, <bio.h>, <regexp.h>
├── mk.h           # Core types: Bufblock, Word, Rule, Node, Arc, Job, Symtab, Shell
├── fns.h          # Function declarations (~70 functions)
├── main.c         # main() — flag parsing, env init, file preread, parse, mk()
├── mk.c           # Core loop: graph(target), clrmade(), work(), waitup()
├── parse.c        # mkfile parser — reads dependency rules
├── lex.c          # Lexical analyzer for mkfile syntax
├── graph.c        # Build dependency graph from rules
├── run.c          # Execute recipes (out-of-date checking, job launching)
├── rule.c         # Rule management (addrule, addrules)
├── unix.c         # Unix-specific: readenv(), exportenv(), waitfor(), execsh()
├── sh.c           # Bourne shell syntax functions (quote handling)
├── rc.c           # rc shell syntax functions
├── shell.c        # Shell dispatch: setshell(), initshell(), push/pop
├── job.c          # Job creation/management
├── arc.c          # Arc (dependency edge) creation
├── archive.c      # Archive (.a) member handling
├── env.c          # Environment variable management
├── file.c         # File I/O helpers
├── match.c        # Pattern matching for meta-rules
├── recipe.c       # Recipe string building
├── varsub.c       # Variable substitution ($VARIABLE, ${VARIABLE})
├── var.c          # Variable operations
├── word.c         # Word list operations
├── bufblock.c     # Dynamic buffer management
├── symtab.c       # Symbol table (hash table)
├── shprint.c      # Shell-escaped printing
└── mk.1           # Man page
```

**Total**: ~3,000 lines of C (roughly 200-300 lines per file on average).

### Key architectural decisions:

1. **Shell abstraction** — `Shell` struct with function pointers for shell-specific syntax:
   - `rcshell` — rc-specific: single-quote only, `$variable` with `$var`/`${var}` syntax
   - `shshell` — Bourne: `\` escape, `'` and `"` quotes, `$var` syntax
   - Default: **`shshell`** (the Plan 9 default is rc; 9base changes it to sh)

2. **Dependency graph** — `graph()` builds a directed graph of `Node` → `Arc` → `Rule` relationships. `mk()` loops calling `work()` and `waitup()`.

3. **Parallel builds** — Controlled by `$NPROC` environment variable. Uses `fork()/waitpid()` on Unix (unix.c).

4. **Portability layer** — Via `lib9`: includes Plan 9's `Biobuf` buffered I/O, `Rune` Unicode support, `fmt` printing, and regexp. Very thin layer compared to plan9port.

### Build system (self-hosting via GNU Make):

```makefile
# std.mk (included by all subdirectory Makefiles)
.c.o:
    @${CC} ${CFLAGS} -I../lib9 -I../lib9/sec $*.c

${TARG}: ${OFILES}
    @${CC} ${LDFLAGS} -o ${TARG} ${OFILES} -L../lib9 -l9 -lm
```

Flags: `-DPLAN9PORT`, links with `-l9` (lib9) and `-lm`.

## 3. Key differences: 9base mk vs plan9port mk

| Aspect | 9base mk | plan9port mk |
|--------|----------|-------------|
| **Author** | Anselm R Garbe (suckless) | Russ Cox (from Bell Labs Plan 9) |
| **Default shell** | `/bin/sh` (shshell) | `rc` (rcshell) |
| **Build system** | GNU Make + config.mk | mkfiles (self-hosting with mk) |
| **Library** | `lib9` — slim Plan 9 compat layer | Full lib9 (much larger) |
| **Source size** | ~3,000 lines, 27 files | ~4,000+ lines, ~30 files |
| **Install prefix** | `/usr/local/plan9` (isolated) | `/usr/local/plan9` (same) |
| **Compile flags** | `-DPLAN9PORT`, static link | More complex configure |
| **Maintenance** | Last commit 2019 | Active (commit 2025+) |
| **Portability targets** | Linux, BSD, musl, Solaris | Linux, BSD, macOS, Solaris |
| **Archive support** | Yes (ar archive member deps) | Yes (more extensive) |
| **#ifdef complexity** | Minimal | Higher (more platforms) |
| **Style** | Suckless — minimal changes, flat Makefile | Plan 9 — self-hosting mk, full compat |

### Shell handling comparison:

**9base** (`shell.c`):
```c
static Shell *shells[] = {
    &rcshell,
    &shshell
};
Shell *shelldefault = &shshell;  // sh is default!
```

**plan9port** — defaults to rc. 9base explicitly changed the default to sh because the suckless philosophy prefers sh (POSIX) over rc (Plan 9-specific).

### What 9base stripped from mk:

- No Plan 9 `note` handling (signals done differently on Unix)
- Simplified `readenv()` — replaced Plan 9's `shname()` parsing with simple `strchr('=')`
- Removed Plan 9-specific `havefork` complexity from rc
- Simpler archive format handling
- Static linking by default (no shared lib concerns)

## 4. Portability approach: Suckless vs Plan9port

### 9base (suckless style):

```
config.mk — edit by hand, one per system
├── PREFIX = /usr/local/plan9
├── OBJTYPE = x86_64
├── CFLAGS += -c -I. -DPLAN9PORT -DPREFIX=...
└── LDFLAGS += -static
```

- **One config.mk** to rule them all
- Simple `make install` (no configure step)
- Musl/libc patches applied as needed (#ifdef __MUSL__)
- Minimal #ifdefs — mostly Linux/BSD with a few Solaris workarounds
- Uses lib9 to fill Plan 9 API gaps (Biobuf, Runes, etc.) but keeps it thin

### plan9port style:

- `configure` script detects system capabilities
- More comprehensive lib9 — emulates Plan 9 /proc, /net, etc.
- Handles macOS, Solaris, Linux, BSD more thoroughly
- Much more `#ifdef` infrastructure

## 5. Maintenance status

| Metric | Status |
|--------|--------|
| **Last commit** | 2019-09-11 (hoc bugfix) — ~7 years ago |
| **Last mk-related commit** | 2016 (musl fixes, bc lib) |
| **Active maintainer** | None (Anselm R Garbe stopped) |
| **Community** | Suckless community — no active 9base development |
| **Alternatives** | plan9port (active), 9front's npe (newer) |
| **Verdict** | Effectively **abandoned** for mk purposes |

### Implications for mk-rs:

- **9base mk is a reference but not a living project** — good for understanding the core mk engine without plan9port's complexity
- The `Shell` abstraction pattern (function pointers for shell-specific parsing) is worth replicating
- The `graph.c`/`mk.c`/`run.c` architecture — graph building, work dispatch, wait/execute — is the core algorithm
- **plan9port mk is a better living reference** (active, more complete)
- 9base's stripped-down approach makes it the best *readable* reference for someone new to mk internals
- The suckless `config.mk` + `Makefile` approach inspired the current `mk-rust` Makefile pattern

## 6. Full tool listing (from Makefile SUBDIRS)

```
lib9   yacc   ascii   awk   basename   bc   cal   cat
cleanname  cmp   date   dc   du   dd   diff   echo   ed
factor  fortune  fmt  freq  getflags  grep  hoc  join
listen1  look  ls  md5sum  mk  mkdir  mtime  pbd
primes  rc  read  rm  sam  sha1sum  sed  seq  sleep
sort  split  ssam  strings  tail  tee  test  touch  tr
troff  unicode  uniq  unutf  urlencode  wc
```

---

*Researched 2026-06-08 by the researcher subagent.*
*Sources: 9base cgit (git.suckless.org/9base), plan9port GitHub (9fans/plan9port)*
