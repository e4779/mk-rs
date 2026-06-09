# Mk Feature Specification — for `mk-rust` implementation

Sources:
- [Hume87] "Mk: a successor to make" — A.G. Hume, AT&T Bell Labs, 1987
- [H+F95] "Maintaining Files on Plan 9 with Mk" — Hume & Flandrena
- [man] mk(1) man page — Plan 9 from User Space (plan9port)

---

## 1. Syntax (BNF-ish Grammar)

### 1.1 Lexical elements

```
line          = assignment | rule | include | comment | blank
comment       = '#' <any chars to newline>
blank         = (whitespace) | (whitespace '\n')
continuation  = <backslash> <newline>  # deleted before further processing
```

### 1.2 Assignments

```
assignment    = var-name '=' var-value
var-name      = <identifier>  # alphabetic + underscore + digits
var-value     = <word-list> | quoted-string | backquote-expr

# Attribute-value assignment (valid for variable to affect rule semantics)
var-assignment = var-name '[' attr-name ']' '=' var-value
```

**Quoting rules** (same as sh(1) — [Hume87]):
- Single quotes `'...'`: fully quoted, no variable expansion
- Double quotes `"..."`: variable expansion occurs, result is single word
- Backquotes: `` `{command} `` (plan9 style) or `` `command `` (sh style) — shell stdout becomes value
- Unquoted whitespace splits into words
- Parts containing `[*?` as unquoted characters are glob-expanded as filenames [Hume87]

### 1.3 Rules

```
rule            = header recipe?
header          = target-list ':' attr-list? ':' prereq-list
target-list     = target (whitespace target)*
prereq-list     = prereq (whitespace prereq)*  # may be empty
attr-list       = <attr-char>+                  # no spaces between attrs
recipe          = (whitespace-line)+            # each line begins with whitespace
```

**Recipe processing**:
- First character of every recipe line is stripped (elided) [man]
- The remaining text, as a block, is passed to the shell via stdin [Hume87]
- Mk does NOT interpret the recipe text — it is pure shell script

### 1.4 Meta-rules

```
# % metarule — % matches any string (equivalent to .+ or .* depending on context)
meta-pct        = prefix '%' suffix
meta-amp        = prefix '&' suffix   # & matches [^./]+ only

# Regex metarule — requires R attribute
meta-regex      = 'regex-pattern'  ':' 'R' ':' prereq-list
                  # pattern uses egrep-style regex with \(\) sub-expressions
```

**Semantics of `%` vs `&`** [H+F95, man]:
- `%` matches maximal length string of any characters
- `&` matches maximal length string of any characters except `.` and `/`

**Stem binding**:
- For `%`/`&` metarules: `$stem` = string matched by `%`/`&`
- For regex metarules: `$stem0`..`$stem9` = matched sub-expressions from \(\) [Hume87]

**NREP**: Limits how many times a metarule is tried when generating prerequisites (default 1) [Hume87]

### 1.5 Includes

```
include-file    = '<' filename
include-cmd     = '<|' executable  # stdout becomes mkfile content
```

Includes are processed as they are read (eager textual inclusion) [man, H+F95].
Unlike make, mk has **no built-in rules** — they come from includes [H+F95].

### 1.6 Variable references

```
var-ref         = '$' var-name
                | '$' '{' var-name '}'
                | '$' '{' var-name ':' pattern '=' replacement '}'
pattern         = A '%' B    # where A and B are literal strings
replacement     = C '%' D    # C replaces A, D replaces B
```

The third form is a **namelist transform** (see §4).

### 1.7 Attributes (post-colon)

```
attr-char       = 'V' | 'Q' | 'N' | 'U' | 'D' | 'E' | 'P' | 'R' | 'n'
```

See §2 for details.

---

## 2. Feature Catalog

### Legend
- **P0**: Core syntax / essential behavior
- **P1**: Important / commonly used
- **P2**: Nice-to-have / less common
- **P3**: Plan 9 specific / deferred

| ID | Feature | Source | Semantics | Priority |
|----|---------|--------|-----------|----------|
| **F-001** | Rule definition (target: prereqs + recipe) | [Hume87], [H+F95], [man] | Basic unit: a target depends on prereqs; recipe updates target. Recipe is optional (for prerequisite-only rules). | **P0** |
| **F-002** | Variables: `$VAR` / `${VAR}` | [Hume87], [man] | Simple variable reference, expanded at parse time for rule headers, at execution time for recipes. | **P0** |
| **F-003** | Assignment: `VAR=value` | [Hume87], [H+F95], [man] | Sets variable value. Value is split into words by whitespace (with quoting). | **P0** |
| **F-004** | % metarules | [Hume87], [H+F95], [man] | Pattern-based rules. `%` matches any substring in target. `$stem` captures match. | **P0** |
| **F-005** | Transitive closure | [Hume87], [H+F95] | After building initial DAG, mk computes closure: if target X can be derived via metarule, it's added as a node. | **P0** |
| **F-006** | Whole-DAG construction before execution | [Hume87], [H+F95] | Mk builds the complete dependency graph before executing any recipe. Unlike make which builds on-demand. | **P0** |
| **F-007** | Parallel execution | [Hume87], [H+F95], [man] | Controlled by `$NPROC`. Recipes are queued; up to NPROC execute simultaneously. | **P0** |
| **F-008** | Timestamp-based staleness | [Hume87], [man] | Target is stale if: it doesn't exist, OR any prerequisite is newer than it, OR a prerequisite is itself stale. | **P0** |
| **F-009** | Virtual targets (V attribute) | [Hume87], [H+F95], [man] | `target:V: prereqs` — target is not a file. Always "built" if prereqs change. Used for `all`, `clean`, etc. | **P0** |
| **F-010** | No-recipe rule (N attribute) | [Hume87], [man] | `target:N: prereqs` — suppresses "no recipe" error. Target is treated as having its timestamp updated if all prereqs up to date. | **P0** |
| **F-011** | Comments: `#` to newline | [man], [Hume87] | Full-line, possibly trailing on non-recipe lines. | **P0** |
| **F-012** | Line continuation: `\<newline>` | [man] | Backslash-newline deleted, joining lines. Can be used in recipes too? [man] says yes for non-recipe lines; recipe lines have first-char elision which interacts. | **P0** |
| **F-013** | Includes: `< file` | [H+F95], [man] | Textual inclusion. Contents evaluated immediately. | **P0** |
| **F-014** | First target as default | [man] | If no target given on command line, mk builds the first non-metarule target in mkfile. | **P0** |
| **F-015** | Recipe as shell script block | [Hume87], [man] | Entire recipe (all indented lines) is fed to shell as single stdin. Not line-by-line like make. | **P0** |
| **F-016** | First-char elision in recipes | [man] | The first character of each recipe line is stripped before passing to shell (accounts for tab/space). | **P0** |
| **F-017** | Missing intermediate targets | [Hume87], [man] | Non-existent file that only serves as intermediate gets a "pretend" timestamp = most recent prereq's. If that makes all dependents up to date, it's skipped. | **P0** |
| **F-018** | Multiple rules for same target (prerequisite merging) | [Hume87], [H+F95], [man] | If a target appears in multiple rules, prerequisites are unioned. Only one rule should have a recipe (else "ambiguous recipe" error). | **P0** |
| **F-019** | Regular rule overrides metarule | [Hume87], [H+F95] | Explicit target rule always wins over a pattern-matching metarule. | **P0** |
| **F-020** | -n flag (dry-run) | [Hume87], [man] | Print recipes without executing. | **P0** |
| **F-021** | -e flag (explain) | [Hume87], [man] | Print why each target is rebuilt (target_timestamp < prereq_timestamp). | **P0** |
| **F-022** | -k flag (keep going) | [man] | Continue as much as possible when errors occur. | **P0** |
| **F-023** | Error handling: E attribute | [H+F95], [man] | `target:E: prereqs` — continue even if recipe exits with non-zero status. Opposite of -e for rc. | **P1** |
| **F-024** | Error handling: D attribute | [man] | `target:D: prereqs` — if recipe fails, delete the target (prevents corrupted files). | **P1** |
| **F-025** | Q attribute (quiet) | [man] | `target:Q: prereqs` — don't print the recipe before executing. | **P1** |
| **F-026** | U attribute (unconditionally updated) | [man] | `target:U: prereqs` — target is considered updated even if recipe didn't change it. | **P1** |
| **F-027** | n attribute (non-virtual-only metarule) | [man] | On metarules: the rule can only match actual files, not virtual targets. | **P1** |
| **F-028** | P attribute (custom comparison) | [man] | `target:P: prereqs` — the `P:` is followed by a program name. Program invoked as `sh -c prog 'target' 'prereq'` to determine staleness. Exit 0 = up-to-date, non-zero = stale. | **P1** |
| **F-029** | R attribute (regex metarules) | [Hume87], [man] | Targets are regular expressions (egrep style). Prerequisites can reference `\1`, `\2` etc. Recipe gets `$stem1`..`$stem9`. | **P1** |
| **F-030** | Aggregate syntax: `lib(member)` | [Hume87], [H+F95], [man] | `archive(member)` references member of aggregate. Used for ar-style archives. | **P1** |
| **F-031** | $newprereq variable | [Hume87], [H+F95], [man] | In recipe: only the prerequisites that triggered the rebuild (newer than target). | **P1** |
| **F-032** | $newmember variable | [man] | In recipe: aggregate members that triggered the rebuild. | **P1** |
| **F-033** | $target variable | [Hume87], [man] | In recipe: list of targets being built by this rule. | **P1** |
| **F-034** | $prereq variable | [Hume87], [man] | In recipe: all prerequisites for this rule. | **P1** |
| **F-035** | $stem variable | [Hume87], [H+F95], [man] | In recipe: string matched by `%` or `&` in metarule. | **P1** |
| **F-036** | $alltarget variable | [Hume87], [man] | In recipe: all targets of this rule (including ones not being rebuilt). | **P2** |
| **F-037** | $nproc variable | [Hume87], [man] | In recipe: slot number (0 to NPROC-1) for this recipe's process. | **P2** |
| **F-038** | $pid variable | [Hume87], [man] | In recipe: PID of the mk process (useful for temp files). | **P2** |
| **F-039** | Namelist transform: `${VAR:A%B=C%D}` | [H+F95], [man] | Transforms each word matching `A%B` by replacing `A`→`C`, `B`→`D`, middle preserved. | **P1** |
| **F-040** | Environment variable import | [H+F95], [man] | On startup, mk imports all environment variables as mk variables (with same name). | **P0** |
| **F-041** | Variable precedence | [H+F95], [man] | Command-line > mkfile assignment > environment > built-in defaults. | **P0** |
| **F-042** | Command-line assignment | [H+F95], [man] | `mk VAR=value` overrides any file/environment assignment. | **P0** |
| **F-043** | Recipe stdout as mkfile (dynamic generation) | [Hume87] | If enabled, the stdout of a recipe can be read as an additional mkfile while mk runs. (Mentioned but not detailed in papers — may be a specific mode or deferred.) | **P3** |
| **F-044** | `&` metarule (limited match) | [H+F95], [man] | Like `%` but matches only `[^./]+` — useful for filenames without extensions. | **P1** |
| **F-045** | Rule header evaluated at parse time | [H+F95] | Variables in target/prereq/attribute sections are expanded when rule is read. Recipe variables are expanded at execution time. | **P0** |
| **F-046** | Short-circuit variable eval (recipe) | [H+F95] | `STRING=all` then `all:VQ: echo $STRING` then `STRING=none` — recipe outputs "none" because $STRING expanded when recipe runs. | **P1** |
| **F-047** | -t flag (touch) | [Hume87], [man] | Update target timestamps without running recipes (like touch). | **P1** |
| **F-048** | -w flag (what-if) | [Hume87], [man] | Pretend listed files have current time. Useful with -n to preview rebuilds. | **P1** |
| **F-049** | -a flag (always make) | [Hume87], [man] | Assume all targets are out of date. | **P1** |
| **F-050** | -d[egp] debugging | [man] | Debug output: p=parsing, g=graph building, e=execution. | **P2** |
| **F-051** | -i flag (force intermediates) | [man] | Override missing-intermediate optimization; always build intermediate targets. | **P1** |
| **F-052** | -s flag (sequential) | [man] | Run recipes sequentially regardless of NPROC. | **P1** |
| **F-053** | `$MKSHELL` variable | [man] | Shell command used to run recipes. If first word ends in `rc` or `rcsh`, uses rc quoting rules; otherwise sh quoting. Default: `/bin/sh`. | **P2** |
| **F-054** | `$MKFLAGS` variable | [man] | In recipe: all option arguments passed to mk. | **P2** |
| **F-055** | `$MKARGS` variable | [man] | In recipe: all target arguments passed to mk. | **P2** |
| **F-056** | `$NREP` variable | [Hume87] | Limits how many times a metarule is used when generating prerequisites (default 1). | **P2** |
| **F-057** | `$OBJ` / `$O` pattern — arch-dependent objects (Plan 9) | [H+F95] | Plan 9 uses `$O` for object suffix (e.g., `8` for 68020, `v` for PowerPC). Not relevant outside Plan 9 but shows design pattern. | **P3** |
| **F-058** | `<| command` — include from command | [man] | Execute command, include its stdout as mkfile input. | **P2** |
| **F-059** | Cycle detection and rejection | [Hume87], [H+F95] | Mk detects cycles in the dependency graph and reports error. Unlike make, cycles are not allowed. | **P0** |
| **F-060** | Pruning irrelevant subgraphs | [H+F95] | After closure, mk prunes parts of DAG not needed for desired target. | **P0** |
| **F-061** | Uniqueness of derivation | [H+F95] | Mk verifies there is exactly one way to build each target (no ambiguous rules). | **P0** |
| **F-062** | Longest-path-first execution order | [H+F95] | Recipes are issued starting from the longest path between target and out-of-date prerequisite, in reverse order (deepest dependencies first). | **P0** |
| **F-063** | Backquote command substitution in mkfile | [man], [Hume87] | `` `{command} `` in non-recipe lines: command runs, stdout becomes part of the line. | **P1** |
| **F-064** | Variables exported to recipe environment | [man], [Hume87] | All variables (except those with U attribute) become environment variables for the recipe's shell process. | **P0** |
| **F-065** | Identical rule headers override | [man] | If target + prerequisites + attributes exactly match an existing rule, the second rule's recipe overrides the first. | **P1** |
| **F-066** | Glob expansion in assignments | [Hume87] | Unquoted `[*?` chars cause filename glob expansion in assignment values. | **P1** |
| **F-067** | `-f mkfile` flag | [man] | Specify alternative mkfile. Multiple `-f` flags allowed (concatenated). | **P0** |
| **F-068** | Virtual target timestamp initialization | [man] | Virtual targets get timestamp = most recent prerequisite's timestamp after being updated. Initially zero. | **P1** |
| **F-069** | Non-existent file targets get pretend timestamp | [man] | Non-existent file targets (not intermediates) get timestamp = most recent prerequisite's timestamp. | **P1** |
| **F-070** | membername utility (Plan 9 rc) | [Hume87], [H+F95] | The `membername` command converts aggregate specs `lib(member)` to just `member`. External rc script, not built into mk. | **P3** |

---

## 3. Execution Model

### 3.1 Phases

Mk operates in distinct phases, executed sequentially:

```
                 +-----------+
                 |   Parse   |  ← Also includes includes, backquote commands
                 +-----+-----+
                       |
                       v
                 +-----------+
                 |   Build   |  ← Construct initial DAG from mkfile rules
                 |  DAG      |
                 +-----+-----+
                       |
                       v
                 +-----------+
                 | Transitive|  ← Apply metarules; add derived nodes & arcs
                 | Closure   |
                 +-----+-----+
                       |
                       v
                 +-----------+
                 |  Prune    |  ← Remove targets not needed for desired goal
                 +-----+-----+
                       |
                       v
                 +-----------+
                 |  Verify   |  ← Check for cycles, ambiguous derivations
                 +-----+-----+
                       |
                       v
                 +-----------+
                 | Execute   |  ← Parallel recipe execution, longest-path-first
                 | Recipes   |
                 +-----------+
```

### 3.2 DAG Construction

- **Nodes**: Every target and prerequisite that appears in a rule, plus any derived via transitive closure.
- **Arcs**: `prerequisite → target` direction (prereq must be built first).
- **Transitive closure**: For each target that matches a metarule pattern, add the metarule-derived prerequisites as nodes and create new arcs. Apply metarules recursively until closure or until NREP limit is reached.
- **Pruning**: Remove all nodes and arcs that are not necessary to build the requested target(s). If no target given, the first non-metarule target in mkfile is used.
- **Verification**: 
  - Detect cycles → error (unlike make, mk rejects cycles)
  - Detect multiple derivations for the same target → "ambiguous recipe" error
  - But: multiple rules for same target are OK if only one has a recipe (prerequisite merging)

### 3.3 Staleness Determination

**Standard model** (timestamp-based):
```
stale(target) = !exists(target) 
                || ∃prereq: stale(prereq) 
                || ∃prereq: mtime(prereq) > mtime(target)
```

**Missing intermediate optimization** [Hume87, man]:
- If a target does not exist, AND all of its dependents are up to date when the target is assigned a "pretend" timestamp = `max(mtime(its_prereqs), 0)`:
  - Then the target is considered up to date and NOT built.
- This avoids building intermediate files that would be immediately deleted (e.g., `.o` that gets linked away).
- `-i` flag: force build all missing intermediates, don't optimize.

**Virtual target timestamps** [man]:
- Virtual targets have no file timestamp.
- Initially: timestamp = 0 (always stale when first needed).
- After being updated (recipe run): timestamp = max(mtime(prereqs)).
- If all prereqs are up to date, virtual target is up to date.

**Custom comparison (P attribute)** [man]:
- `target:P:prog prereqs` — for each prerequisite, mk invokes:
  ```sh
  sh -c prog 'target' 'prereq'
  ```
- Exit code 0 → target is up to date (no rebuild needed for this prereq).
- Non-zero exit → target is stale, rebuild.
- This overrides timestamp-based comparison for that target.

**What-if mode** (-w flag) [Hume87, man]:
- Pretend listed files have current modification time.
- Combined with -n: preview what would be rebuilt.

### 3.4 Parallel Execution

**NPROC control** [Hume87, man]:
- `$NPROC` determines max simultaneous recipes (default 1).
- On Plan 9, `$NPROC` is set automatically to CPU count.
- -s flag forces sequential execution regardless of NPROC.

**Scheduling** [H+F95]:
- Mk processes the DAG from longest path first.
- A recipe is eligible when all its prerequisites are up to date.
- Eligible recipes are placed on a work queue.
- Up to NPROC recipes execute simultaneously from the queue.
- Process slots: each concurrent recipe gets a slot 0..NPROC-1, exposed as `$nproc` (useful for temp file naming).

**Waitup** [Hume87]:
- As each recipe completes, mk checks if new recipes are now eligible.
- If a recipe fails and -k (keep going) is in effect, targets depending on it are marked as failed but other branches continue.

### 3.5 Recipe Execution

**Shell invocation** [man]:
```
MKSHELL=/bin/sh  # default
```
Recipe is fed to shell via stdin as a single block.

**Shell selection** [man]:
- If `$MKSHELL` first word ends in `rc` or `rcsh`, mk uses rc quoting rules (single vs double quotes reversed).
- Otherwise, sh quoting rules apply.
- On Unix/plan9port: typically `/bin/sh`.
- On Plan 9: typically `rc` (default shell).

**Recipe environment** [Hume87, H+F95, man]:
All variables (except those with attribute U) are exported as environment variables to the recipe's shell. See variable system in §4 for the full list.

**Error behavior**:
- By default: if recipe exits non-zero, mk stops building that target chain.
- `E` attribute: continue even on error (equivalent to rc `-e` override).
- `D` attribute: if recipe fails, delete the target file to avoid corrupted state.
- `-k` flag: continue other branches even when one target fails.

### 3.6 Multiple Targets

When a rule has multiple targets [Hume87]:
```
b.o c.o: prog.h
```
The recipe runs once if any target is out of date. Variables `$target` and `$alltarget` distinguish which targets are being rebuilt vs. all targets.

---

## 4. Variable System

### 4.1 Classification

| Type | Source | Example | Priority |
|------|--------|---------|----------|
| Built-in defaults | Mk itself | `CC=cc`, `CFLAGS=`, `NPROC=1` | 0 (lowest) |
| Environment import | OS env on startup | `$PATH`, `$HOME`, `$objtype` | 1 |
| Mkfile assignments | Assignment in file | `CFLAGS=-g` | 2 |
| Command-line assignments | `mk VAR=value` | — | 3 (highest) |

### 4.2 Evaluation Timing

**Critical distinction** [H+F95]:

| Context | When variables are expanded |
|---------|----------------------------|
| Rule header (targets, attrs, prereqs) | Parse time (when rule is read) |
| Recipe body | Execution time (when recipe runs) |
| Assignment right-hand side | Parse time |
| Include filename | Parse time |

This means:
```makefile
TARGET=foo
$(TARGET):V: prereq    # expands to: foo:V: prereq   (at parse time)

$(TARGET):V:
	echo $TARGET       # $TARGET expanded when recipe runs
TARGET=bar             # → outputs "bar", not "foo"
```

### 4.3 Built-in Variables (for recipes)

| Variable | Contents | Source |
|----------|----------|--------|
| `$target` | Targets being built (for this rule invocation) | [Hume87], [man] |
| `$alltarget` | All targets of this rule (including ones not currently stale) | [Hume87], [man] |
| `$prereq` | All prerequisites of this rule | [Hume87], [man] |
| `$newprereq` | Prerequisites that are newer than the target (triggered rebuild) | [Hume87], [H+F95], [man] |
| `$newmember` | Aggregate members that triggered the rebuild | [man] |
| `$stem` | String matched by `%` or `&` in metarule | [Hume87], [H+F95], [man] |
| `$stem0`..`$stem9` | Sub-expressions matched by regex metarule | [Hume87] |
| `$nproc` | Process slot number (0..NPROC-1) | [Hume87], [man] |
| `$pid` | PID of the mk process | [Hume87], [man] |
| `$MKFLAGS` | All option arguments passed to mk | [man] |
| `$MKARGS` | All target arguments passed to mk | [man] |

### 4.4 User Variables

Set via:
- Mkfile: `CFLAGS=-g` (space-separated list of words after expansion)
- Environment: automatically imported at startup
- Command-line: `mk CFLAGS=-O2 target`

### 4.5 Namelists (Variable Transforms)

**Syntax** [H+F95, man]:
```
${var:A%B=C%D}
```

**Semantics**:
1. Expand `$var` to get a list of words.
2. For each word, if it matches pattern `A%B` (where `A` is prefix, `B` is suffix, and `%` matches any middle — like `A.*B`):
   - Replace `A` with `C` (prefix substitution)
   - Replace `B` with `D` (suffix substitution)
   - The middle portion matched by `%` is preserved
3. Words that do NOT match `A%B` are dropped from the result.

**Example**:
```makefile
SRC=a.c b.c c.c
OBJ=${SRC:%.c=%.o}    # → (a.o b.o c.o)
```

Edge case: if `A` and `B` are empty, `%` matches the entire word, and C and D become prefix/suffix wrappers:
```
${VAR:%=prefix%suffix}   # → each word becomes "prefix{word}suffix"
```

### 4.6 Backquote Assignment

**Syntax**: `` var=`{command} `` (Plan 9 style) or `` var=`command `` (sh style) [H+F95, man]

**Semantics**: The command runs in an environment populated with previously assigned variables. Its stdout becomes the value of `var`. The command is run by the shell.

### 4.7 Variable Attributes (less documented)

Hume87 briefly mentions `var=[attr=]value` syntax in the man page description:
```
var=[attr=]value
```
This is used for special variable attributes. The only known use is via `NPROC` which controls parallelism. This syntax is obscure and may be treated as P3 unless further detail emerges.

---

## 5. Special Mechanisms

### 5.1 Aggregates (Archive Support)

**Syntax**: `archive(member)` — references `member` within `archive` [Hume87, H+F95, man].

**Purpose**: Maintain UNIX static libraries (`.a` files managed by `ar`).

**Rules with aggregate notation**:
```makefile
lib.a:N: lib.a(a.o) lib.a(b.o)
lib.a(%.o): %o
	ar r lib.a $stem.o
```

**Efficient invocation** [Hume87, H+F95]:
```makefile
lib.a: lib.a(a.o) lib.a(b.o)
	ar r lib.a `membername $newprereq`
lib.a(%.o):N: %o
```

Here `$newprereq` contains only the archive members that changed. The external `membername` command strips the `lib(` prefix and `)` suffix.

**In man page**: "Currently, only `9ar` archives are supported" (plan9port). For a Rust implementation, this means the aggregate mechanism is abstract: mk provides the `a(b)` notation, and the actual archive tool is external.

**$newmember** [man]: Specifically for aggregate prerequisites — contains only the member names (without the aggregate wrapper) that triggered the rebuild.

### 5.2 Virtual Targets

**Marked by** V attribute: `target:V: prereqs` [Hume87, H+F95, man].

**Semantics**:
- The target has no associated file.
- If no prereqs, the recipe ALWAYS runs (like `clean:V:`).
- If prereqs exist: target is stale when any prereq is stale.
- Timestamp: initially 0; after building, timestamp = max(mtime(prereqs)).
- A virtual target cannot be the prerequisite of a non-virtual rule for staleness purposes (it has no file to compare).

**Common patterns**:
```makefile
all:V: program1 program2    # "build everything" alias
clean:V:                    # always runs
	rm -f *.o program
```

### 5.3 Custom Comparison (P attribute)

**Syntax**: `target:P:prog prereqs` where `prog` is a program name [man].

**Execution**: For each prerequisite, mk invokes:
```sh
sh -c prog 'target' 'prereq'
```
- Exit 0: up to date
- Non-zero: stale (rebuild)

**Use cases**: Binary comparison (`cmp -s`), checksum comparison, semantic equivalence checking.

This makes mk's out-of-date determination pluggable — one of its key extensions over make.

### 5.4 Dynamic Mkfile Generation

**From recipe stdout** [Hume87]: "Mk allows the standard output of a recipe to be read as an additional mkfile while mk is running."

**Semantics**: This is briefly mentioned. It means that during execution, when a recipe completes, its stdout can be interpreted as additional mkfile content (assignments/rules). This allows self-modifying build descriptions.

**Implementation note**: This is a P3 feature. It's mentioned in the "Differences between make and mk" list in Hume87 but no detailed syntax or mechanism is given. It may refer to a specific operational mode rather than a general feature.

### 5.5 Include Mechanism

**File include** [H+F95, man]:
```makefile
<$objtype/mkfile
```
Replaces the line with contents of the named file. Variables in the filename are expanded at parse time. The included content is evaluated immediately (like C `#include`).

**Command include** [man]:
```makefile
<| mkdep *.c
```
Executes the command and uses its stdout as mkfile input. This is processed at parse time.

**Usage pattern**: Plan 9 splits mkfiles into architecture-specific prototypes:
```makefile
# In local mkfile:
</$objtype/mkfile

# The prototype ($objtype/mkfile sets CC, LD, O, etc.)
```
This is how mk achieves multi-architecture builds without built-in rules.

### 5.6 Metarule Application (Transitive Closure)

**Algorithm** [H+F95]:
1. For each target T that is requested (or depends on a requested target):
2. Check if T matches any metarule pattern.
3. If yes, derive new prerequisites by applying the pattern transformation:
   - `%` metarule: replace `%` in prereq pattern with the stem from target
   - Regex metarule: substitute `\1`, `\2` etc. with matched groups
4. Add the target to the DAG if not already present (with its derived prerequisites).
5. Add derived prerequisites as new nodes and recurse (within NREP limit).

**Example**:
```makefile
%.o: %.c
	$(CC) -c $stem.c

prog: a.o b.o
```
When mk encounters `a.o` as a prerequisite of `prog`, it matches `%.o` → stem = "a". Derived prerequisite: `a.c`. Adds `a.o` as a node with prerequisite `a.c`.

### 5.7 Recipe-less Rules

A rule without a recipe [H+F95, man]:
```makefile
b.o: prog.h
```
- Prerequisites are merged with other rules for the same target.
- If no other rule exists for this target, and the N attribute is set, the target is treated as having timestamp updated.
- If no other rule exists and no N attribute, mk issues a "no recipe" warning.

### 5.8 Environment and Recipes

**What is exported** [man]:
- All variables are exported as environment variables to the recipe's shell.
- Exception: variables set with attribute U (unconditionally updated — `U` probably stands for something else here? Actually, the man page says variables are exported "unless `U` attribute is present". This is confusing because U is a target attribute, not a variable attribute. This may be an error in the man page or refer to a different context.)

**Recipe environment** includes at minimum: `$PATH`, `$target`, `$prereq`, `$newprereq`, `$stem`, `$nproc`, `$pid`, and all user-defined variables.

### 5.9 Meta-rule Non-file Target (n attribute)

From [man]:
> `n` — The rule is a meta-rule that cannot be a target of a virtual rule. Only files match.

This means a metarule with the `n` attribute will only match actual file names, not virtual targets or non-existent targets that could satisfy the pattern. Prevents metarules from accidentally matching against virtual/phoney targets.

### 5.10 Option Summary

| Short | Long | Effect | Source |
|-------|------|--------|--------|
| `-a` | — | Assume all targets out of date | [Hume87], [man] |
| `-d[egp]` | — | Debug (p=parse, g=graph, e=execution) | [man] |
| `-e` | — | Explain why each target is made | [Hume87], [man] |
| `-f file` | — | Use specified mkfile (can be multiple) | [man] |
| `-i` | — | Force missing intermediates | [man] |
| `-k` | — | Keep going on error | [man] |
| `-n` | — | Dry-run (print, don't execute) | [Hume87], [man] |
| `-s` | — | Sequential execution | [man] |
| `-t` | — | Touch targets (update timestamps) | [Hume87], [man] |
| `-w list` | — | What-if (pretend files modified now) | [Hume87], [man] |

---

## 6. Edge Cases and Ambiguities

### 6.1 Recipe-first-char elision + empty lines

The first character of every recipe line is stripped [man]. For blank lines in recipes, stripping a newline character would break the line. Implementation should handle:
- Lines containing only whitespace → elide first whitespace char, remainder is empty string fed to shell
- Empty lines (zero length after elision) → pass as empty line

### 6.2 Overlapping metarules

If two metarules match the same target, and there's no explicit rule, mk reports "ambiguous recipe" error after transitive closure [H+F95].

### 6.3 Cyclic dependencies

Mk rejects cycles at verification phase. Example:
```makefile
a: b
b: c
c: a
```
→ "cycle detected" error.

### 6.4 Metarule infinite recursion guard

NREP (default 1) prevents infinite expansion during transitive closure [Hume87].

### 6.5 Recipe stdout as mkfile

This is mentioned in Hume87's difference list but no mechanism is specified. Our implementation should provide this as P3 unless we find more documentation.

### 6.6 Empty prerequisite lists

A rule with no prerequisites and no recipe:
```makefile
target::
```
If file exists: up to date. If file doesn't exist: always stale with "no recipe" warning (unless N attribute).

### 6.7 Glob expansion interaction with assignments

From [Hume87]: unquoted `[*?` characters trigger filename glob expansion in assignment values. This is surprising and interacts with variable evaluation. Our implementation should:
- Glob after variable expansion
- Only in non-quoted contexts
- Only for assignment values, not rule targets/prereqs (or is it for rule headers too? Hume87 says "The text is then broken into parts separated by unquoted white space, and parts containing `[*?` as unquoted characters are expanded as filenames" — this applies to assignment lines AND rule header lines.)

### 6.8 Backslash-newline in recipes

From [man]: "Backslash-newline is deleted, so long lines may be folded" — this applies to the entire mkfile processing, but recipe lines also have first-char elision. So `\<newline>` in a recipe:
- Before elision: backslash-newline removed
- After elision: first char of each line removed

The order matters: backslash-newline deletion happens before first-char elision.

---

## 7. Built-in Defaults

From [Hume87]:

| Variable | Default |
|----------|---------|
| `AS` | `as` |
| `CC` | `cc` |
| `CFLAGS` | (empty) |
| `FC` | `f77` |
| `FFLAGS` | (empty) |
| `LDFLAGS` | (empty) |
| `LEX` | `lex` |
| `LFLAGS` | (empty) |
| `NPROC` | `1` |
| `NREP` | `1` |
| `YACC` | `yacc` |
| `YFLAGS` | (empty) |
| `MKSHELL` | `/bin/sh` |

Note: Plan 9 port likely ships different defaults (rc, different compiler names). These defaults are from the original Bell Labs Research UNIX version.

---

## 8. Implementation Notes for mk-rust

### 8.1 Key Architectural Decisions

1. **Parse → Build DAG → Closure → Prune → Verify → Execute**: This is the core pipeline. Each phase is distinct.

2. **Parallelism**: Use Rust's concurrency primitives (threads or async) for recipe scheduling. NPROC maps to thread pool size or tokio semaphore.

3. **File system abstraction**: Timestamps are read from the OS. Handle precision differences (nanosecond vs. second resolution) gracefully.

4. **Regex engine**: Need a Rust regex crate for R: metarules. Must support capture groups.

5. **No built-in rules**: Unlike make, mk has zero built-in inference rules. Everything comes from the mkfile or includes.

### 8.2 Order of Implementation (suggested)

| Phase | Features | Priority |
|-------|----------|----------|
| **Phase 1** | Parser (F-001..F-014, F-041, F-042, F-045, F-067) | P0 |
| **Phase 2** | Execution (F-006, F-007, F-008, F-015..F-022, F-059..F-062) | P0 |
| **Phase 3** | Variables (F-033..F-036, F-039, F-040) | P1 |
| **Phase 4** | Metarules advanced (F-029, F-044, F-056) | P1 |
| **Phase 5** | Aggregates (F-030, F-032) | P1 |
| **Phase 6** | Custom comparison (F-028), custom comparison | P1 |
| **Phase 7** | Special attributes (F-023..F-027) | P1 |
| **Phase 8** | What-if, touch, debug (F-047..F-052) | P1 |
| **Phase 9** | Plan9-specific (F-053, F-057, F-058) | P2/P3 |
| **Phase 10** | Dynamic mkfile generation (F-043) | P3 |

### 8.3 Deviation Notes

- The `n` attribute (F-027) and `n` as lowercase need careful lexing to distinguish from uppercase `N`.
- Glob expansion in assignments (F-066) is unusual — most modern make implementations dropped this. Consider if mk-rust should retain this behavior.
- `membername` (F-070) is an external Plan 9 rc script, not part of mk itself. Our implementation should document this but not implement it inside mk.

---

*End of feature specification.*
