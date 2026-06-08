# mk(1) — Plan 9 from User Space

> Source: https://9fans.github.io/plan9port/man/man1/mk.html

---

## NAME

**mk** – maintain (make) related files

## SYNOPSIS

`mk [ -f mkfile ] ... [ option ... ] [ target ... ]`

## DESCRIPTION

Mk uses the dependency rules specified in _mkfile_ to control the update (usually by compilation) of _targets_ (usually files) from the source files upon which they depend. The _mkfile_ (default `mkfile`) contains a _rule_ for each target that identifies the files and other targets upon which it depends and an _sh_(1) script, a _recipe_, to update the target. The script is run if the target does not exist or if it is older than any of the files it depends on. _Mkfile_ may also contain _meta-rules_ that define actions for updating implicit targets. If no _target_ is specified, the target of the first rule (not meta-rule) in _mkfile_ is updated.

The environment variable `$NPROC` determines how many targets may be updated simultaneously; Some operating systems, e.g., Plan 9, set `$NPROC` automatically to the number of CPUs on the current machine.

## Options

| Option | Description |
|--------|-------------|
| `-a` | Assume all targets to be out of date. Everything is updated. |
| `-d[egp]` | Produce debugging output (p=parsing, g=graph building, e=execution). |
| `-e` | Explain why each target is made. |
| `--graph` | Output dependency graph in DOT format and exit. Pipe to `dot -Tsvg > graph.svg`. |
| `--graph-of TARGET` | Output subgraph reachable from TARGET in DOT format and exit. |
| `-i` | Force any missing intermediate targets to be made. |
| `-k` | Do as much work as possible in the face of errors. |
| `-n` | Print, but do not execute, the commands needed to update the targets. |
| `-s` | Make the command line arguments sequentially rather than in parallel. |
| `-t` | Touch (update the modified date of) file targets, without executing any recipes. |
| `-w target1,target2,...` | Pretend the modify time for each target is the current time; useful with `-n`. |

## The Mkfile

A _mkfile_ consists of _assignments_ and _rules_. A rule contains _targets_ and a _tail_. A target is a literal string and is normally a file name. The tail contains zero or more _prerequisites_ and an optional _recipe_, which is an shell script. Each line of the recipe must begin with white space.

A rule takes the form:

```
target: prereq1 prereq2
	recipe using prereq1, prereq2 to build target
```

When the recipe is executed, the first character on every line is elided.

After the colon on the target line, a rule may specify _attributes_ (see below).

### Meta-rules

A _meta-rule_ has a target of the form `A%B` where `A` and `B` are (possibly empty) strings. A meta-rule acts as a rule for any potential target whose name matches `A%B` with `%` replaced by an arbitrary string, called the _stem_. In interpreting a meta-rule, the stem is substituted for all occurrences of `%` in the prerequisite names. In the recipe, the environment variable `$stem` contains the string matched by the `%`.

Meta-rules may contain an ampersand `&` rather than a percent sign `%`. A `%` matches a maximal length string of any characters; an `&` matches a maximal length string of any characters except period or slash.

The text of the mkfile is processed as follows:
- Lines beginning with `<` followed by a file name are replaced by the contents of the named file
- Lines beginning with `<|` followed by a file name are replaced by the output of the execution of the named file
- Blank lines and comments (`#` to newline) are deleted
- Backslash-newline is deleted, so long lines may be folded
- Non-recipe lines: `` `{command} `` — output of the command when run by sh
- Variable references are replaced by the variables' values
- Special characters may be quoted using single quotes `''` as in sh(1)

### Rule modification

A later rule may modify or override an existing rule:
- If the targets of the rules exactly match and one rule contains only a prerequisite clause and no recipe, the clause is added to the prerequisites of the other rule.
- If the targets of the rules match exactly and the prerequisites do not match and both rules contain recipes, mk reports an "ambiguous recipe" error.
- If the target and prerequisites of both rules match exactly, the second rule overrides the first.

## Environment

References: `$OBJ` or `${name}` (expanded as in sh). A reference of the form `${name:A%B=C%D}` has the value formed by expanding `$name` and substituting `C` for `A` and `D` for `B` in each word that matches pattern `A%B`.

Variables can be set by assignments: `var=[attr=]value`. Blanks in the value break it into words. Variables are exported to the environment of recipes unless `U` attribute is present.

**Precedence** (increasing):
1. Default values
2. Mk's environment
3. The mkfiles
4. Command line assignment

Special variables:
- **`MKFLAGS`** — all option arguments
- **`MKARGS`** — all targets
- **`MKSHELL`** — shell command line mk uses to run recipes. If the first word ends in `rc` or `rcsh`, mk uses rc(1)'s quoting rules; otherwise sh(1)'s.

## Execution

A target is considered up to date if:
- It has no prerequisites, OR
- All prerequisites are up to date and it is newer than all prerequisites

Date stamps:
- **Virtual targets** (V attribute): initially zero; set to most recent prerequisite's date stamp when updated
- **Non-existent file targets**: set to most recent prerequisite's date stamp, or zero if no prerequisites
- **Existing file targets**: always the file's modification date

**Missing intermediates**: Non-existent targets with prerequisites that are themselves prerequisites are treated specially. If their most recent prerequisite date stamp would make dependents up-to-date, they are skipped. The `-i` flag overrides this.

Recipes are executed by supplying the recipe as standard input to `/bin/sh`. **Unlike make, mk feeds the entire recipe to the shell** rather than running each line separately.

### Recipe environment variables

| Variable | Description |
|----------|-------------|
| `$alltarget` | All the targets of this rule |
| `$newprereq` | Prerequisites that caused this rule to execute |
| `$newmember` | Prerequisites that are members of an aggregate that caused this rule to execute |
| `$nproc` | Process slot for this recipe (0 ≤ $nproc < $NPROC) |
| `$pid` | Process ID of the mk executing the recipe |
| `$prereq` | All prerequisites for this rule |
| `$stem` | String that matched `%` or `&` in meta-rule. For regex rules: `stem0...stem9` |
| `$target` | Targets for this rule that need to be remade |

## Aggregates

Names of the form `a(b)` refer to member `b` of the aggregate `a`. Currently, only `9ar` archives are supported.

## Attributes

The colon separating the target from the prerequisites may be immediately followed by attributes and another colon:

| Attribute | Description |
|-----------|-------------|
| **`D`** | If the recipe exits with non-null status, the target is deleted |
| **`E`** | Continue execution if the recipe draws errors |
| **`N`** | If there is no recipe, the target has its time updated |
| **`n`** | The rule is a meta-rule that cannot be a target of a virtual rule. Only files match |
| **`P`** | Custom comparison program. Program invoked as `sh -c prog 'arg1' 'arg2'` |
| **`Q`** | The recipe is not printed prior to execution |
| **`R`** | The rule is a meta-rule using regular expressions. `%` has no special meaning |
| **`U`** | The targets are considered to have been updated even if the recipe did not do so |
| **`V`** | The targets are marked as virtual — distinct from files of the same name |
