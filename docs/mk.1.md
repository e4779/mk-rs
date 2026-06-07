% MK(1) mk-rust 0.1.0
% mk-rust contributors
% June 2025

# NAME

**mk** — maintain (make) related files

# SYNOPSIS

`mk` \[**-f** _mkfile_\]... \[_option_...\] \[_target_...\]

# DESCRIPTION

**Mk** reads dependency rules from a _mkfile_ (default `mkfile`) and executes
shell recipes to bring _targets_ up to date. A target is rebuilt when it does
not exist or when any of its prerequisites is newer.

Unlike **make**(1), mk builds the entire dependency graph before executing any
recipe, supports pattern-based metarules with transitive closure, and runs
recipes in parallel (controlled by `$NPROC`).

If no _target_ is given, mk builds the first non-metarule target in the
_mkfile_.

# OPTIONS

**-a**
: Assume all targets are out of date. Every recipe is executed.

**-C** _dir_
: Change to _dir_ before reading the mkfile or building targets.

**-d**\[_egp_\]
: Print debugging output. Each character selects a category:
  **e** — execution,
  **g** — graph building,
  **p** — parsing.

**-e**
: Explain why each target is (or is not) being remade.

**-f** _mkfile_
: Use _mkfile_ instead of the default `mkfile`. Multiple **-f** flags
  concatenate the named files.

**-k**
: Continue building unrelated targets when a recipe fails. Targets that
  depend on a failed target are skipped.

**-n**
: Print recipes that would be executed, but do not run them.

**-s**
: Silent mode. Do not print recipes before executing them (equivalent
  to the **Q** attribute on every rule).

**-t**
: Touch targets — update their modification times to the current time
  without running any recipes.

# MKFILE SYNTAX

A _mkfile_ consists of _assignments_ and _rules_. Blank lines and comments
(`#` to end of line) are ignored. A backslash at the end of a line joins it
with the following line (before any other processing).

## Assignments

```
VAR = value
```

Sets the variable `VAR` to the given value. The value is split into words on
whitespace, respecting sh(1)-style quoting. Variable references (`$VAR` or
`${VAR}`) in the value are expanded at parse time.

A value may also be obtained from a shell command:

```
SRCS = `{ls *.c}
```

## Rules

```
target [target ...] [:attributes:] [prereq ...]
	recipe line one
	recipe line two
```

A rule declares that each _target_ depends on the _prerequisites_. If any
target is out of date, the _recipe_ is fed as a single block to **$MKSHELL**
(default `/bin/sh`). The first character of each recipe line is stripped
before execution.

If a target appears in multiple rules, their prerequisites are merged. Only
one of those rules may provide a recipe, otherwise mk reports an "ambiguous
recipe" error.

## Meta-rules

A meta-rule uses a pattern in place of a literal target name. Two pattern
characters are supported:

**%**
: Matches the longest possible string of any characters (greedy).
  Example: `%.o: %.c` matches `foo.o` with stem `foo`.

**&**
: Matches the longest possible string of characters except `.` and `/`.
  Example: `lib&.a: &.o` matches `libfoo.a` with stem `foo`, but not
  `lib.foo.a`.

In prerequisites of a meta-rule, `%` (or `&`) is replaced by the stem. In the
recipe, the environment variable `$stem` holds the matched string.

**Regex meta-rules** use the **R** attribute. The target is an egrep-style
regular expression with capture groups. Matched groups are available in the
recipe as `$stem1` through `$stem9`.

```
(.*)\.o:R: \1.c
	cc -c $stem1.c
```

## Attributes

Attributes are single-character flags placed between colons after the target
list:

```
target:VQ: prereq
```

| Attribute | Description |
|-----------|-------------|
| **D** | Delete the target file if the recipe fails. |
| **E** | Continue building even if this recipe exits with a non-zero status. |
| **N** | No-exec — treat the target as updated without running a recipe. |
| **P** | Custom comparison — use an external program to decide staleness. |
| **Q** | Quiet — do not print the recipe before executing it. |
| **R** | Regex — the target is a regular expression (metarule). |
| **U** | The target is considered updated even if the recipe did not change it. |
| **V** | Virtual target — not a real file. Always stale if prerequisites change. |
| **n** | Only match real files (metarule). Never match virtual targets. |

## Includes

```
< path/to/file
```

Replaces the line with the contents of the named file, processed immediately.
Variables in the filename are expanded at parse time.

```
<| command
```

Runs _command_ and includes its standard output as mkfile content.

# ENVIRONMENT

## Variables set by mk for recipes

**`$alltarget`**
: All targets declared by the current rule.

**`$MKARGS`**
: All target arguments passed to mk on the command line.

**`$MKFLAGS`**
: All option arguments passed to mk on the command line.

**`$newprereq`**
: The prerequisites that are newer than the target (triggered the rebuild).

**`$nproc`**
: Process slot number for this recipe (0 ≤ `$nproc` < `$NPROC`).

**`$pid`**
: Process ID of the mk process.

**`$prereq`**
: All prerequisites of the current rule.

**`$stem`**
: The string matched by `%` or `&` in a meta-rule.

**`$target`**
: The targets being built by this rule invocation.

## Variables that control mk

**`$MKSHELL`**
: Shell used to execute recipes. Default: `/bin/sh`. If the first word of
  `$MKSHELL` ends in `rc` or `rcsh`, mk uses rc quoting conventions.

**`$MKFLAGS`**
: (Read-only in recipes.) Option arguments passed to mk.

**`$NPROC`**
: Maximum number of recipes to run in parallel. Default: `1`.

## Variable precedence

Variables are resolved in order of increasing priority:

1. Built-in defaults (`CC=cc`, `NPROC=1`, etc.)
2. Environment variables imported at startup
3. Assignments in the mkfile
4. Command-line assignments (`mk VAR=value`)

## Namelist transforms

```
${VAR:A%B=C%D}
```

For each word in `$VAR` matching the pattern `A%B`, replaces the prefix `A`
with `C` and the suffix `B` with `D`. Words not matching the pattern are
dropped.

Example: `OBJ = ${SRC:%.c=%.o}` converts `a.c b.c` to `a.o b.o`.

# EXIT STATUS

**0**
: All requested targets were brought up to date successfully.

**1**
: An error occurred (missing mkfile, parse error, recipe failure without
  **-k**, cycle detected, or ambiguous recipe).

# EXAMPLES

**Simple C compilation:**

```
# mkfile
prog: main.o util.o
	cc -o prog main.o util.o

main.o: main.c
	cc -c main.c

util.o: util.c
	cc -c util.c
```

**Using metarules and variables:**

```
# mkfile
CC = cc
CFLAGS = -O2 -Wall
OBJ = main.o util.o

prog: $OBJ
	$CC -o $target $prereq

%.o: %.c
	$CC $CFLAGS -c $stem.c

clean:V:
	rm -f prog $OBJ
```

**Virtual target as default, Q for quiet:**

```
all:VQ: prog test

prog: main.o
	cc -o $target $prereq

test:V:
	./prog < input.txt | diff - expected.txt
```

**Regex metarule:**

```
doc/(.*)\.html:R: src/\1.md
	pandoc -o $target src/$stem1.md
```

# SEE ALSO

**make**(1), **rc**(1), **sh**(1)

Plan 9 from User Space: _/usr/local/plan9/man/man1/mk.html_

Andrew Hume, "Mk: a Successor to Make" (USENIX Summer Conference, 1987)

# NOTES

mk-rust is a faithful Rust port of Plan 9 mk. It aims for full compatibility
with mkfiles written for plan9port mk. Some Plan 9-specific features (the
`$O` / `$OBJ` architecture variables, rc-only recipe semantics) are not yet
supported.
