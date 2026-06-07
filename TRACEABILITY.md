# Traceability Matrix: Spec → Modules → Phases

> Each feature F-xxx from `kb/mk-spec.md` is mapped to its implementing module and phase.
>
> Status: `—` = not started, `✓` = done, `◐` = partially done (attr parsing done, behavior pending)

## Legend

| Column | Meaning |
|--------|---------|
| **F-xxx** | Feature ID from mk-spec.md |
| **Module** | Primary Rust module implementing it |
| **Phase** | 1a (core MVP), 1b (essentials), 2 (metarules+parallel), 3 (polish) |
| **Status** | `—` not started, `◐` partial, `✓` done |

## Core Syntax & Parsing

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-001 | Rule definition (target: prereqs + recipe) | `parse` | 1a | ✓ |
| F-011 | Comments: `#` to newline | `lex` | 1a | ✓ |
| F-012 | Line continuation: `\<newline>` | `lex` | 1a | ✓ |
| F-013 | Includes: `< file` | `include`, `parse` | 1b | ◐ (include done) |
| F-014 | First target as default | `graph`, `cli` | 1a | ✓ |
| F-045 | Rule header evaluated at parse time | `parse`, `var` | 1a | ✓ |
| F-063 | Backquote command substitution in mkfile | `lex`, `var` | 1b | ◐ (lex done) |
| F-066 | Glob expansion in assignments | `var` | 2 | — |
| F-067 | `-f mkfile` flag | `cli` | 1a | — |

## Variables

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-002 | Variables: `$VAR` / `${VAR}` | `var` | 1a | ✓ |
| F-003 | Assignment: `VAR=value` | `parse`, `var` | 1a | ✓ |
| F-039 | Namelist transform: `${VAR:A%B=C%D}` | `var` | 2 | — |
| F-040 | Environment variable import | `var` | 1a | ✓ |
| F-041 | Variable precedence (cmdline > file > env > builtin) | `var` | 1a | ✓ |
| F-042 | Command-line assignment `mk VAR=value` | `cli`, `var` | 1a | — |
| F-046 | Short-circuit variable eval (recipe execution time) | `var` | 1b | — |
| F-064 | Variables exported to recipe environment | `var`, `recipe` | 1a | — |

## Meta-rules

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-004 | `%` metarules | `parse`, `graph` | 2 | — |
| F-005 | Transitive closure | `graph` | 2 | — |
| F-019 | Regular rule overrides metarule | `graph` | 2 | — |
| F-029 | `R:` regex metarules | `parse`, `graph` | 2 | — |
| F-044 | `&` metarule (limited match) | `parse`, `graph` | 2 | — |
| F-056 | `$NREP` variable | `var`, `graph` | 2 | — |

## Graph & DAG

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-006 | Whole-DAG construction before execution | `graph` | 1a | ✓ |
| F-008 | Timestamp-based staleness | `graph` | 1a | ✓ |
| F-017 | Missing intermediate targets | `graph` | 1b | ✓ |
| F-018 | Multiple rules for same target (prereq merging) | `parse`, `graph` | 1a | ◐ (parse done) |
| F-059 | Cycle detection and rejection | `graph` | 1a | ✓ |
| F-060 | Pruning irrelevant subgraphs | `graph` | 2 | — |
| F-061 | Uniqueness of derivation | `graph` | 2 | — |
| F-062 | Longest-path-first execution order | `graph`, `sched` | 1b | ◐ (sched done) |
| F-065 | Identical rule headers override | `parse` | 1a | ✓ |
| F-069 | Non-existent file targets get pretend timestamp | `graph` | 1b | ✓ |

## Parallel Execution

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-007 | Parallel execution (`$NPROC`) | `sched` | 2 | — |
| F-037 | `$nproc` variable | `var`, `sched` | 2 | — |
| F-052 | `-s` flag (sequential) | `cli`, `sched` | 2 | — |

## Recipe Execution

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-015 | Recipe as shell script block | `recipe`, `shell` | 1a | ✓ |
| F-016 | First-char elision in recipes | `recipe` | 1a | ✓ |
| F-033 | `$target` variable | `var`, `recipe` | 1b | ✓ |
| F-034 | `$prereq` variable | `var`, `recipe` | 1b | ✓ |
| F-035 | `$stem` variable | `var`, `recipe` | 2 | — |
| F-036 | `$alltarget` variable | `var`, `recipe` | 2 | — |
| F-031 | `$newprereq` variable | `var`, `recipe` | 2 | — |
| F-032 | `$newmember` variable | `var`, `recipe` | 3 | — |
| F-038 | `$pid` variable | `var`, `recipe` | 1b | ✓ |
| F-053 | `$MKSHELL` variable | `shell`, `cli` | 2 | — |
| F-054 | `$MKFLAGS` variable | `var` | 2 | — |
| F-055 | `$MKARGS` variable | `var` | 2 | — |

## Attributes

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-009 | Virtual targets (V attribute) | `attr`, `graph` | 1a | ◐ (attr done) |
| F-010 | No-recipe rule (N attribute) | `attr`, `graph` | 1a | ◐ (attr done) |
| F-023 | Error handling: E attribute | `attr`, `sched` | 1b | ◐ (attr done) |
| F-024 | Error handling: D attribute | `attr`, `sched` | 2 | ◐ (attr done) |
| F-025 | Q attribute (quiet) | `attr`, `recipe` | 1a | ◐ (attr done) |
| F-026 | U attribute (unconditionally updated) | `attr`, `graph` | 1b | ◐ (attr done) |
| F-027 | n attribute (non-virtual-only metarule) | `attr`, `graph` | 2 | ◐ (attr done) |
| F-028 | P attribute (custom comparison) | `attr`, `graph` | 3 | ◐ (attr done) |
| F-068 | Virtual target timestamp initialization | `attr`, `graph` | 1a | — |

## CLI

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-020 | `-n` flag (dry-run) | `cli`, `sched` | 1a | ✓ |
| F-021 | `-e` flag (explain) | `cli`, `sched` | 1a | ✓ |
| F-022 | `-k` flag (keep going) | `cli`, `sched` | 1b | ✓ |
| F-047 | `-t` flag (touch) | `cli`, `sched` | 1b | ✓ |
| F-048 | `-w` flag (what-if) | `cli`, `graph` | 2 | — |
| F-049 | `-a` flag (always make) | `cli`, `graph` | 1b | ✓ |
| F-050 | `-d[egp]` debugging | `cli` | 3 | — |
| F-051 | `-i` flag (force intermediates) | `cli`, `graph` | 1b | ✓ |

## Aggregates

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-030 | Aggregate syntax: `lib(member)` | `archive`, `parse` | 3 | — |

## Special / Plan 9 Specific

| ID | Feature | Module | Phase | Status |
|----|---------|--------|-------|--------|
| F-043 | Recipe stdout as mkfile (dynamic generation) | `recipe`, `parse` | 3 | — |
| F-058 | `<| command` include | `include` | 2 | — |
| F-057 | `$OBJ` / `$O` (Plan 9 arch-dependent objects) | — | P3 | — |
| F-070 | `membername` utility | — | P3 | — |

## Summary by phase

| Phase | Progress | Feature count | What it covers |
|-------|:---:|:---:|---|
| **1a** | ████████ 22/22 ✅ | 22 | Parser, DAG, serial exec, core variables, attrs, scheduling |
| **1b** | ███████░ 10/12 | 12 | Include `< file`, prereq/target vars, missing intermediates, flags |
| **2** | ░░░░░░░░ 0/22 | 22 | %/&/R metarules, transitive closure, pruning, NPROC parallel |
| **3** | ░░░░░░░░ 0/10 | 10 | Aggregates, `<| cmd`, P attribute, dynamic mkfile, -d debug |
| **P3** | ░░░░░░░░ 0/4 | 4 | Plan 9 specifics ($O, membername) |

*Completed: lex (F-011, F-012, F-063 backtick tokens), attr (F-009-010, F-023-028 parsing).*
