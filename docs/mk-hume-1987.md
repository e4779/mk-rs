# Mk: a successor to make

*Andrew Hume, AT&T Bell Laboratories, 1987*

---

## ABSTRACT

Mk is an efficient general tool for describing and maintaining dependencies
between files or programs. Mk is styled on, and largely compatible with, the UNIX
tool make. The major advantages of mk over make are executing recipes in parallel,
using pattern-matching metarules rather than suffix transformation rules, and deriving
dependencies by transitive closure on all rules. Mk runs anywhere from 2 to 30 times faster
than make.

This report describes mk by means of an evolving example. Other sections summarize
the differences between mk and make and discuss the principles underlying mk's design.

---

## 1. Introduction

A large fraction of computer activity consists of repeated application of tools (special or general purpose
programs) to input files to produce output files. The most obvious example is programming, but other
no less important examples range from simple document-processing pipelines to the generation of a circuit
board or integrated circuit involving hundreds of files. Common to all these activities are file dependencies,
where changing a file requires that other files be remade. Mk reads a dependency description (called a
mkfile) and does the minimal work necessary to bring a target file up to date.

Mk owes much to make, written by Stu Feldman, which has been doing a similar job on UNIX systems
since 1976. The version of make referred to throughout this report is Feldman's research version distributed
with Research UNIX, Eighth Edition and is substantially more advanced than the versions found
in System V or Berkeley UNIX systems.

---

## 2. An Extended Example

This section describes mk in the context of building C programs. This is for the reader's comfort;
mk knows nothing special about C programs.

Initially, our program is called prog and is made from a.o and b.o, which are made by compiling a.c
and b.c respectively. In addition, b.c includes a header file prog.h.

The mkfile is a sequence of rules. Each rule defines a target (say prog) that depends on some
prerequisites (a.o and b.o) and the commands (a shell script called the recipe) to bring the target up to date.

```makefile
prog:  a.o b.o
	cc -o prog a.o b.o
a.o:   a.c
	cc -c a.c
b.o:   b.c prog.h
	cc -c b.c
```

```
$ mk
cc -c a.c
cc -c b.c
cc -o prog a.o b.o
$

$ mk
mk: 'prog' is up to date
$

modify a.c
$ mk
cc -c a.c
cc -o prog a.o b.o
$
```

Mk will explain why it is rebuilding a file if we use the -e option. For example,

```
modify prog.h
$ mk -e
b.o(540869437) < prog.h(540869535)
cc -c b.c
prog(540869493) < b.o(540869546)
cc -o prog a.o b.o
$
```

### Variables

```makefile
CFLAGS=-g
prog:  a.o b.o
	cc $CFLAGS -o prog a.o b.o
a.o:   a.c
	cc $CFLAGS -c a.c
b.o:   b.c prog.h
	cc $CFLAGS -c b.c
```

Some variables are supplied by mk for use by the recipe. One is `prereq` whose value is all the
prerequisites for this rule. We can rewrite the first rule:

```makefile
prog:  a.o b.o
	cc $CFLAGS -o prog $prereq
```

This guarantees that the lists of object files (the prerequisite line and the cc line) are the same.

### Metarules

Mk supports metarules, that is, rules that apply to a class of targets, rather than just one specific target.
The class of targets is defined by pattern matching, with the symbol `%` (called the stem) equivalent to the
regular expression `.*`.

```makefile
%.o:  %.c
	$CC $CFLAGS -c $stem.c
```

Using this metarule, our mkfile becomes shorter:

```makefile
CC=cc
CFLAGS=-g -p
prog:  a.o b.o c.o
	$CC $CFLAGS -o prog $prereq
b.o:  prog.h
c.o:  prog.h
%.o:  %.c
	$CC $CFLAGS -c $stem.c
```

The prerequisites for a target can spread across many rules. Only one of the rules should have a recipe.

Mk has some predefined variables and rules listed in Appendix 1.

Any non-metarule takes precedence over a metarule.

### Rules with no prerequisites

Rules need not actually build their targets. Some rules are simply shell scripts embedded in the mkfile.

```makefile
clean:V:
	rm -f *.o prog core
```

Mk allows a label to have an attribute of **virtual** (`V:`), which means that it is distinct from a file
of the same name.

### Rules with multiple targets

```makefile
b.o c.o:  prog.h
clean:V:
	rm -f *.o prog core
```

### Yacc example (conditional updates)

The grammar is kept in gram.y. The -d option to yacc produces y.tab.h. The mkfile uses a conditional
shell construct to avoid unnecessary recompilation:

```makefile
prog:    a.o b.o c.o y.tab.o lex.o
	$CC $CFLAGS -o prog $prereq
b.o c.o: prog.h
lex.o:   x.tab.h
x.tab.h: y.tab.h
	cmp -s x.tab.h y.tab.h || cp y.tab.h x.tab.h
y.tab.c y.tab.h: gram.y
	yacc -d gram.y
```

If y.tab.h doesn't change, lex.o is not recompiled.

### Aggregates

Mk supports aggregates such as UNIX object libraries (archives maintained by ar). The notation `a(m)`
refers to member m of aggregate a.

```makefile
lib.a:N: lib.a(a.o) lib.a(b.o) lib.a(c.o)
lib.a(%.o): %o
	ar r lib.a $stem.o
```

A better way collects all .o files first, then does the ar:

```makefile
lib.a: lib.a(a.o) lib.a(b.o) lib.a(c.o)
	ar r lib.a `membername $newprereq`
lib.a(%.o):N: %o
```

The `N` attribute stops mk from complaining that there is no recipe. The variable `newprereq` contains
only the prerequisites that have changed.

### Parallel processing

Mk executes recipes by continually traversing the dependency graph looking for targets that can be
made. When mk finds a recipe it can execute, it puts the recipe on a queue. The number of recipes
executing simultaneously is the value of the variable `NPROC`, which is initially one. On multi-processor
machines, mk goes faster with higher values; most mkfiles on their 12 processor machine have NPROC
between 6 and 10.

The `-u` (utilization) option measures how many seconds are spent with so many recipes executing.

### Missing intermediates

Any non-existent intermediate target is treated specially. If pretending it existed with the time stamp of
its most recent prerequisite would make all targets that depended on it be up to date, then it is not made.

```
$ mk -e
mk: 'prog' is up to date
remove a.o
$ mk -e
pretending a.o has time 540869454
mk: 'prog' is up to date
```

### Administrative options

- **`-t`** — touch (update timestamps without rebuilding)
- **`-n`** — print recipes without executing
- **`-w files,...`** — "what if" — set internal timestamps to current time
- **`mk -n -wprog.h`** — ask "what would we rebuild if prog.h changed?"
- **`mk -a`** — always make every target

### Quoting

Quoting rules for assignment lines and rule header lines are intended to be the same as for sh(1).
Backquotes execute shell commands. Text between single quotes is quoted. Text between double quotes
is quoted after variable expansion. The text is then broken into parts separated by unquoted white space,
and parts containing `[*?` as unquoted characters are expanded as filenames.

---

## More on metarules

There are two kinds of metarules:
1. **`%` metarules** — pattern matching with `%` (equivalent to `.*`)
2. **Regular expression metarules** — full egrep(1) regular expressions with sub-expressions `\(\)`

Example: making object files in sub-directories:

```makefile
'(.*)/([/]*).o':\R: '\1/\2.c'
	cd $stem1; $CC $CFLAGS -c $stem2.c
```

The `R` attribute means interpret the target(s) as regular expression(s).

The variable `NREP` limits the number of times a metarule is used in generating prerequisites (normally 1).

---

## 3. Getting Fancy

(Section noted but details were not fully expanded in the original paper — see original PDF.)

---

## 4. Differences between make and mk

- **Make** builds targets when it needs them, allowing systematic use of side effects. **Mk** constructs the entire dependency graph before building any target.
- **Make** supports suffix rules and % metarules. **Mk** supports % and regular expression metarules.
- **Mk** performs transitive closure on metarules, **make** does not.
- **Make** supports cyclic dependencies, **mk** does not.
- **Make's** recipes are collections of one-line shell commands, executed a line at a time. Variable values are passed by editing the recipe text. **Mk's** recipes are simply shell scripts executed as one unit. Variable values are passed through environment variables.
- **Make** supports parallel execution of single line recipes when building prerequisites for specified targets. **Mk** supports parallel execution of all recipes.
- **Mk** allows the standard output of a recipe to be read as an additional mkfile while mk is running.
- **Mk** supports virtual targets which exist only within an execution of mk.
- **Mk** supports a general mechanism for deciding whether a file is out of date.

### Performance comparison

For mkfiles with no metarules, mk is always faster because of better accessing algorithms.

| Scenario | make | mk | Speedup |
|----------|------|-----|---------|
| OS compile (83 object files) | 19.8u+3.6s | 6.6u+3.6s | 2.3x |
| Program (61 object files, all metarules) | 12.0u+9.7s | 5.1u+4.0s | 2.4x |
| Program (61 object files, one metarule) | 12.0u+9.7s | 3.9u+2.9s | 3.2x |
| C library (242 members) | 47.7u+10.9s | 6.3u+12.5s | 3.1x |
| Workstation (238 .c, 59 .h, 7 .y, 7 .l) | 278.8u+16.2s | 8.4u+10.5s | **15.6x** |

### Conversion from make to mk

A sed(1) script called `mkconv` handles mechanical syntax conversion. Manual changes are needed for
side-effects used by make (like yacc grammar handling). The general rule: tell the truth about dependencies
and let dynamic time measuring prevent unnecessary work.

### Availability

AT&T Bell Laboratories employees: TOAD. Commercial UNIX licensees: AT&T Toolchest. Educational
licensees: contact Judith L. Macor, AT&T Bell Laboratories, Murray Hill, NJ.

---

## 5. The Principles

1. **Use existing syntax and notions.** Mkfile syntax is almost exactly same as makefile. Variables are
   exactly same as shell variables. Recipes are written in sh(1). Regular expression syntax adopted from
   egrep and ed.

2. **Generalize features.** Metarules extended to full regular expressions. Transitive closure on
   target-prerequisite relations. Parallel execution of any recipe. Full dependency graph before any
   recipes execute.

3. **Removing special cases.** Recipes are shell scripts — mk doesn't parse or process them. Special
   dot-name targets dropped in favor of explicit target attributes.

4. **Mk is a general purpose tool.** Not just for C programming — for maintaining file dependencies
   whether they be programs or circuit board descriptions.

---

## 6. Appendix — Built-in Variables and Rules

### Default variables

```
AS=as          CC=cc          CFLAGS=
FC=f77         FFLAGS=        LDFLAGS=
LEX=lex        LFLAGS=        NPROC=1
NREP=1         YACC=yacc      YFLAGS=
```

### Built-in rules

```makefile
%.o:  %.c  $CC $CFLAGS -c $stem.c
%.o:  %.s  $AS -o $stem.o $stem.s
%.o:  %.f  $FC $FFLAGS -c $stem.c
%.o:  %.y  $YACC $YFLAGS -o $stem.c $stem.y && $CC $CFLAGS -c $stem.c && rm $stem.c
%.o:  %.l  $LEX $LFLAGS -t $stem.l > $stem.c && $CC $CFLAGS -c $stem.c && rm $stem.c
```

### Environment variables for recipes

| Variable | Description |
|----------|-------------|
| `alltarget` | All the targets for this rule |
| `newprereq` | Prerequisites more recent than the target |
| `nproc` | Process slot number (0 to NPROC-1) |
| `pid` | Process ID of the mk invocation |
| `prereq` | All prerequisites for this target |
| `stem` | Value of % in a metarule (null for non-metarule) |
| `stemn` | Value of nth sub-expression in regex metarule |
| `target` | Targets being built for this rule |
