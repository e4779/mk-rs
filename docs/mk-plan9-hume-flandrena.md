# Maintaining Files on Plan 9 with Mk

*Andrew G. Hume & Bob Flandrena, Bell Labs*

> Source: https://9p.io/sys/doc/mk.html

---

## ABSTRACT

Mk is a tool for describing and maintaining dependencies between files. It is similar to the UNIX program make, but provides several extensions. Mk's flexible rule specifications, implied dependency derivation, and parallel execution of maintenance actions are well-suited to the Plan 9 environment. Almost all Plan 9 maintenance procedures are automated using mk.

---

## 1. Introduction

This document describes how mk, a program functionally similar to make, is used to maintain dependencies between files in Plan 9. Mk provides several extensions to the capabilities of its predecessor that work well in Plan 9's distributed, multi-architecture environment. It exploits the power of multiprocessors by executing maintenance actions in parallel and interacts with the Plan 9 command interpreter rc to provide a powerful set of maintenance tools.

An earlier paper [Hume87] provides a detailed discussion of mk's design.

---

## 2. The Mkfile

Mk reads a file describing relationships among files and executes commands to bring the files up to date. The specification file, called a _mkfile_, contains three types of statements: **assignments**, **includes**, and **rules**.

A rule has four elements: targets, prerequisites, attributes, and a recipe:

```
targets:attributes: prerequisites
	recipe
```

### Simple example (POSIX):

```makefile
CC=pcc
f1: f1.c
	$CC -o f1 f1.c
```

### Plan 9 multi-architecture example:

```makefile
</$objtype/mkfile

f1: f1.$O
	$LD $LDFLAGS -o f1 f1.$O
f1.$O: f1.c
	$CC $CFLAGS f1.c
```

The first line includes the prototype mkfile for the target architecture (`$objtype` is inherited from the environment). Variables `CC`, `LD`, `O` are architecture-specific.

### Building multiple programs:

```makefile
</$objtype/mkfile
ALL=f1 f2
all:V: $ALL

f1: f1.$O
	$LD $LDFLAGS -o f1 f1.$O
f1.$O: f1.c
	$CC $CFLAGS f1.c

f2: f2.$O
	$LD $LDFLAGS -o f2 f2.$O
f2.$O: f2.c
	$CC $CFLAGS f2.c
```

The target `all`, modified by the attribute `V`, builds both programs. The attribute identifies `all` as a dummy target not related to a file of the same name.

---

## 3. Variables and the Environment

Mk does not distinguish between its internal variables and rc variables in the environment. When mk starts, it imports each environment variable into a mk variable of the same name.

**Precedence** (decreasing):
1. Command line assignment
2. Assignment statement
3. Imported from the environment
4. Implicitly set by mk

### Namelists

A _namelist_ is a list produced by transforming the members of an existing list using pattern matching:

```
${var:A%B=C%D}
```

The pattern `A%B` matches a member beginning with `A` and ending with `B` with any string in between (like `A.*B`). `C` replaces `A`, `D` replaces `B`, and the matched string replaces itself.

Example:
```makefile
SRC=a.c b.c c.c
OBJ=${SRC:%.c=%.v}   # → (a.v b.v c.v)
```

### Command output

```
var=`{rc command}
```

The command executes in an environment populated with previously assigned variables.

---

## 4. The Include Statement

```
<filename
```

The contents of the file are evaluated as they are read. An include statement may be used anywhere except in a recipe. **Unlike make, mk has no built-in rules.** Instead, the include statement allows generic rules to be imported from a prototype mkfile.

---

## 5. Rules

- **Rule header**: evaluated when the rule is read. Variables are replaced by their values at this time.
- **Recipe**: an rc script. Optional. When missing, the rule is handled specially.
- Mk executes recipes **without interpretation** — after stripping the first white space character from each line, it passes the entire recipe to rc on standard input.
- Mk invokes rc with the `-e` flag (stop on error); the `E` attribute overrides this.
- Variable substitution in a rule is done when the rule is **read**; variable substitution in the recipe is done when the recipe is **executed**.

```
STRING=all
all:VQ:
	echo $STRING
STRING=none      # → produces "none", because $STRING is evaluated at execution time
```

---

## 6. Metarules

A _metarule_ is a rule based on a pattern.

### Intrinsic patterns:
- **`%`** — matches one or more of anything (like `.+`)
- **`&`** — matches one or more of any characters except `/` and `.` (like `[^./]+`)

```makefile
%.$O: %.c
	$CC $CFLAGS $stem.c
```

The string matched by `%` in the target is supplied to the recipe in `$stem`.

### Metarule example (multi-program build):

```makefile
</$objtype/mkfile
ALL=f1 f2
all:V: $ALL

%: %. $O
	$LD -o $target $prereq
%.$O: %.c
	$CC $CFLAGS $stem.c

clean:V:
	rm -f $ALL *.[$OS]
```

### Regular expression metarules:

Must have an `R` attribute. Prerequisites may reference matching substrings using `\n`. In a recipe, `$stemn` is the equivalent reference.

```makefile
(.+)\.$O: R: \1.c
	$CC $CFLAGS $stem1.c
```

---

## 7. Archives

Mk provides a special mechanism for maintaining an archive. An archive member is referenced using the form `lib(file)`.

```makefile
$LIB(foo.8):N: foo.8
$LIB: $LIB(foo.8)
	ar rv $LIB foo.8
```

The `N` attribute prevents mk from complaining about a target with no recipe (the subsequent rule updates the member).

### Efficient archive update:

```makefile
$LIB: ${OBJS:%=$LIB(%)}
	ar rv $LIB `{membername $newprereq}
```

The internal variable `$newprereq` contains only the out-of-date prerequisites. The rc script `membername` translates archive member specifications into file names.

---

## 8. Evaluation Algorithm

1. Build a **dependency graph** (nodes = targets/prerequisites, arcs = dependencies)
2. Compute **transitive closure** — extend the graph to include all potentially derivable targets
3. **Check for cycles** — mk does not allow cyclic dependencies (unlike make)
4. **Prune** subgraphs irrelevant for producing the desired target
5. **Verify** there is only one way to build each target
6. **Execute** recipes on the longest path between target and out-of-date prerequisite, in reverse order

Mk avoids infinite cycles by evaluating each metarule once.

---

## 9. Conventions for Evaluating Rules

There must be only one way to build each target. When metarule patterns select potential targets that conflict with other rules:

1. **Regular rule over metarule** — explicit targets always win
2. **Recipe-less rules add prerequisites** — prerequisites are merged into other rules with the same target
3. **Virtual target with no other rule** — evaluates each prerequisite (acts as an alias)
4. **Identical rule headers with recipes** — the later rule replaces the former

---

## 14. Unspecified Dependencies

The `-w` command line flag forces files to be treated as just modified. Combined with grep:

```
$ mk -w `{grep -l _var_ *.[cyl]}
```

Rebuilds all source files that reference a global variable `_var_` changed in a header file.

---

## 16. Conclusion

There are many programs related to make, each choosing a different balance between specialization and generality. **Mk emphasizes generality** but allows customization through its pattern specifications and include facilities.

Plan 9 presents a difficult maintenance environment with its heterogeneous architectures and languages. Mk's flexible specification language and simple interaction with rc work well in this environment. As a result, Plan 9 relies on mk to automate almost all maintenance.

---

## 17. Appendix: Differences between make and mk

| make | mk |
|------|-----|
| Builds targets when it needs them, allowing side effects | Constructs the **entire dependency graph** before building any target |
| Supports suffix rules and % metarules | Supports % and **regular expression** metarules |
| Does NOT perform transitive closure on metarules | **Performs transitive closure** on metarules |
| Supports cyclic dependencies | Does **NOT** support cyclic dependencies |
| Evaluates recipes **one line at a time**, replacing variables | Passes **entire recipe** to the shell without interpretation |
| Parallel execution of single-line recipes for specified targets | Parallel execution of **all** recipes |
| Special targets beginning with `.` for special processing | Uses **attributes** to modify rule evaluation |
| No virtual targets | Supports **virtual targets** independent of the file system |
| Standard out-of-date determination only | Allows **non-standard** out-of-date determination |

It is usually easy to convert a makefile to or from an equivalent mkfile.

---

## References

- [Feld79] S. I. Feldman, "Make — a program for maintaining computer programs", _Software Practice & Experience_, 1979
- [Flan95] Bob Flandrena, "Plan 9 Mkfiles"
- [Hume87] A. G. Hume, "Mk: A Successor to Make", _USENIX Summer Conf. Proc._, Phoenix, Az.
