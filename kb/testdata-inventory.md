# Testdata Inventory: Real-World mkfiles for mk-rust Testing

Collected 2026-06-07 from 4 sources (mksite, plan9port, 9legacy, ctSkennerton).

## Summary Table

| Source | Files | Interesting Features |
|--------|-------|---------------------|
| **mksite** | 1 | Regex rules (`:R:`), rc shell recipes, directory targets, attribute `:V:`, command substitution `` `{} `` |
| **plan9port** | 28 (incl. 8 build framework includes) | Archive aggregates `$(%)`, metarules `%.o`, virtual targets `:V:`, includes `<$file`, command piping `<|cmd`, `$O.$TARG` pattern, variable lists, `$target`/`$prereq`/`$stem`, multi-directory recursion |
| **9legacy** | 15 (incl. 2 include files) | Regex rules `^()\.$O:R:`, archive aggregates `$BOOTLIB(%):N:`, kernel build (203-line mkfile), `:D:` double-colon, `:Q:` quiet, complex variable expansion `${VAR:%=$DIR/%}`, rc for loops, `$newprereq`/`$newmember`, generated headers |
| **ctSkennerton** | 18 tests + 1 mkfile | Focused unit tests: includes, variable expansion, attribute `:V:`, backquote `${}`, regex `:R:`, `:S:` suffix attributes, `<|cmd` piping, multi-line variables |
| **dcjones/mk** | 0 | Go mk port with no test mkfiles — skipped |

**Total: 63 files** (incl. framework includes)

## Detailed Breakdown

### 1. mksite — Static Site Generator using mk + rc

**File:** `mksite/mkfile` (52 lines)

**Features tested:**
- Regex metarules: `html/(.*)/([^/]*)\.html:R: md/\1/\2.md`
- Variable substitution with bash-like backticks: `` `{du -a md | awk '{print $2}'}` ``
- Plan 9 rc shell recipes
- Virtual targets: `all:V:`, `/bin/date:V:`, `clean:V:`
- Directory targets: `html/%/ gend/%/:`
- Attribute `:V:` for phony targets
- Nested variable expansion `${md_dirs:md/%/=html/%/}`

**For:** Integration test — complex regex rules and rc recipes.

---

### 2. plan9port — Build Framework + Libraries + Commands

#### Build Framework Includes (8 files)

| File | Purpose | Features |
|------|---------|----------|
| `mkenv` | Environment setup | Shell/mk dual-syntax file, `uname`, `INSTALL=` |
| `mkhdr` | Header for all mkfiles | Variable defaults, `<|cat $PLAN9/config`, `O=o`, `CC=9c` |
| `mkcommon` | Common rules | Metarules `%.$O: %.c`, `nuke:V:`, `clean:V:` |
| `mkone` | Single binary build | `$PROG = $O.$TARG`, `install:V:`, `%.install:V:` |
| `mkmany` | Multi-binary build | `$O.%: %.$O $OFILES`, `many-install:V:`, for loop install |
| `mkdirs` | Directory recursion | `dir-%:V: for i in $DIRS (cd $i; mk ...)` |
| `mksyslib` | System library with archive | **Archive aggregate**: `$PLAN9/lib/$LIB(%):N:%`, `&:n: &.$O` |
| `mklib` | Simple library | Simpler archive: `$LIB(%):N:%`, `&:n: &.$O` |

**For:** **Simplify into unit tests** — archive aggregates, metarules, virtual targets, multi-target patterns.

#### Representative per-directory mkfiles (18 files)

| Path | Lines | Key Features |
|------|-------|-------------|
| `src/lib9/mkfile` | 199 | Huge variable list `OFILES=`, metarules `%.$O: fmt/%.c`, `%.$O: utf/%.c`, multi-variable XREFs `ctime.$O tm2sec.$O zoneinfo.$O: zoneinfo.h` |
| `src/libavl/mkfile` | 14 | Simple library include `<$PLAN9/src/mksyslib`, `${LIB:/$objtype/%=/386/%}` |
| `src/libthread/mkfile` | ~60 | Metarules with `%.$O: arch/%.c`, variable overrides |
| `src/libdraw/mkfile` | ~40 | Library, variable lists |
| `src/libmp/mkfile` | ~40 | Library with assembly files |
| `src/libsec/mkfile` | ~50 | Library with assembly |
| `src/libmach/mkfile` | ~30 | Simple library |
| `src/cmd/mkfile` | 36 | **Multi-binary** `TARG=`ls`, metarules `%.tab.h %.tab.c: %.y`, `%.o: %.tab.c`, yacc/lex rules, `$PLAN9/bin/yacc: $O.yacc` |
| `src/cmd/devdraw/mkfile` | 47 | **Complex build** with `<|osxvers`, `<|sh ./mkwsysrules.sh`, Objective-C metarule `%.$O: %.m`, `install:V:` with conditional, `:Q:` on install |
| `src/cmd/rc/mkfile` | ~20 | Simple single binary |
| `src/cmd/samterm/mkfile` | ~30 | Multi-target with yacc |
| `src/cmd/acid/mkfile` | ~30 | Debugger build |
| `src/cmd/fossil/mkfile` | ~40 | Fossil filesystem |
| `src/cmd/venti/srv/mkfile` | ~50 | Venti server |
| `src/cmd/upas/mkfile` | ~30 | Mail system directory recursion |
| `src/cmd/upas/fs/mkfile` | ~20 | Mail fs |
| `src/cmd/vbackup/mkfile` | ~30 | Vbackup |
| `src/cmd/auth/mkfile` | ~30 | Auth with subdirs |
| `src/cmd/mk/mkfile` | 37 | **The mk command itself** — prototypical mkfile |
| `src/cmd/mk/mkfile.test` | 150 | **mk's own test suite** — goldmine of edge cases |

**For:** **Keep as integration tests** — large file lists, archive aggregates, nested includes. **Simplify** into unit tests for: archive `(%)` syntax, `&` metarule, virtual target attributes.

---

### 3. 9legacy — Native Plan 9 Operating System Build

| File | Lines | Key Features |
|------|-------|-------------|
| `mkfile.386` | 6 | Architecture config: `CC=8c`, `LD=8l`, `O=8` |
| `mkfile.amd64` | 6 | Architecture config: `CC=6c`, `LD=6l`, `O=6` |
| `mkfile.arm` | 6 | Architecture config: `CC=5c` |
| `mkfile.proto` | 18 | Base config, `OS=568ijqv`, `CPUS=`, `CFLAGS=-FTVw`, clears TARG/OFILES/HFILES |
| `9.mkfile` | 36 | **Multi-arch kernel recursion**: `for(i in $ARCH)@{ cd $i; mk }`, `installall:V:` |
| `9pc.mkfile` | 203 | **Most complex mkfile**: archive aggregates, `<|cmd`, `:D:` double-colon, `${}` variable transforms, `$CONFLIST`, computed OFILES via `$DEVS`, `$ETHER`, `$VGA`, `$SDEV`, gen'd headers, `:Q:` quiet, `for` loops |
| `portmkfile` | 97 | **Regex rules**: `^($PORTFILES)\.$O:R: '../port/\1.c'`, `^($IPFILES)\.$O:R:` — captures with `$stem1`, `%.$O: %.c`, `%.acid:` |
| `bootmkfile` | 28 | **Archive aggregate**: `$BOOTLIB(%.$O):N:`, `$BOOTLIB: ${BOOTFILES:%=$BOOTLIB(%)}`, `$newprereq` |
| `git.mkfile` | 57 | Multi-target with rc scripts, overridden `install:V:`, `%.rcinstall:V:` pattern |
| `cmd.mkfile` | ~30 | Command directory build |
| `libc.mkfile` | ~40 | Large library CFILES list |
| `libavl.mkfile` | 15 | Simple library |
| `acme_source.mkfile` | 27 | Subdirectory recursion with `%.dirs:VQ:`, `@{}` |
| `acme.mkfile` | ~30 | Acme editor build |
| `9k_k10.mkfile` | ~40 | K10 kernel variant |

**For:** **Keep regex rules and archive aggregate files as integration tests** — these are the most advanced mk features. **Simplify** the pattern into unit tests.

---

### 4. ctSkennerton — Focused mk Test Harness (Go port)

| File | Lines | Feature Under Test |
|------|-------|--------------------|
| `mkfile` | 32 | Real-world mkfile: `$PROG`, `$GOTOOL`, metarule `%.1: %.1.md`, `:V:` attributes |
| `test1.mk` | 3 | Basic target + recipe |
| `test2.mk` | 9 | Environment variable expansion `$TEST_MAIN`, `$prereq`/`$stem`/`$target` |
| `test3.mk` | 8 | Variable as dependency list `deps = one` |
| `test4.mk` | 12 | Multi-dependency variable `deps = one two` |
| `test5.mk` | 12 | Multi-line variable with `\` continuation |
| `test6.mk` | 12 | Include + variable deps `<$file` |
| `test7.mk` | 14 | Variable-expanded include path `<$depsfile` |
| `test8.mk` | 14 | Include + variable in recipe |
| `test9.mk` | 14 | Include + variable in recipe (different content) |
| `test10.mk` | 6 | Command execution `<|go env`, variable `$GCCGO` |
| `test11.mk` | 8 | Backquote expansion in variables `deps = \`echo $hello\`` |
| `test12.mk` | 5 | `:V:` attribute on target |
| `test13.mk` | 21 | **Rich expansion**: `${targets:%=$targetpath/%}`, `${targets:%=%.$suffix}`, `ab$targetpath` ambiguity, `$targetpath/foo` vs `${targetpath}/foo`, `$prefix.$suffix` |
| `test14.mk` | 5 | Command-defined rules via `<| echo "target:..."` |
| `test15.mk` | 2 | **`:S:` suffix attribute** — `awktarget:Sawk -f /dev/stdin:` (reciped-from-stdin) |
| `test16.mk` | 10 | Include + `$extracmdarg` in recipes |
| `test17.mk` | 6 | **Regex rule `:R:`**: `prereq.(\w+):R:`, `$stem1`, `$(echo ${stem1})` |

**For:** **Directly usable as unit tests.** These are small, focused, and purpose-built. Each tests one feature. 17 tests, ~8 lines average.

---

## Features Tested (Feature Matrix)

| Feature | mksite | plan9port | 9legacy | ctSkennerton |
|---------|--------|-----------|---------|--------------|
| `$target`, `$prereq`, `$stem` | ✓ | ✓ | ✓ | ✓ |
| `$newprereq`, `$newmember` | | | ✓ | |
| Metarules `%.o: %.c` | | ✓ | ✓ | ✓ |
| Regex rules `:R:` | ✓ | | ✓ | ✓ |
| Archive aggregates `$(%):N:` | | ✓ | ✓ | |
| `&:n: &.$O` (unnamed archive) | | ✓ | | |
| Virtual targets `:V:` | ✓ | ✓ | ✓ | ✓ |
| Double-colon `:D:` | ✓ | ✓ | ✓ | |
| Quiet attribute `:Q:` | | ✓ | ✓ | |
| Suffix attribute `:S:` | | | | ✓ |
| Null attribute `:N:` | | ✓ | ✓ | |
| Includes `<$file` | | ✓ | ✓ | ✓ |
| Piped command `<|cmd` | | ✓ | ✓ | ✓ |
| Backquote `${}` | | ✓ | ✓ | ✓ |
| Variable transforms `${X:%=$Y/%}` | | ✓ | ✓ | ✓ |
| Multi-line vars `\` | ✓ | ✓ | | ✓ |
| Multi-target rules | ✓ | ✓ | ✓ | ✓ |
| Directory recursion | | ✓ | ✓ | |
| Shell recipe (not rc) | | ✓ | ✓ | ✓ |
| rc recipe | ✓ | | ✓ | |
| Comments `#` | ✓ | ✓ | ✓ | ✓ |
| Attribute `\%` force | | | | |
| Nested attribute parsing | | | | |
| `:XXX` 3-letter attributes | | | | |

## Features Possibly Not Supported by mk-rust (Check)

1. **Regex rules (`:R:`)** — used in mksite (2 regex rules), 9legacy portmkfile (computed regex), ctSkennerton test17. These are documented in Plan 9 mk but may not be implemented in mk-rust yet.
2. **Archive aggregates (`$(%):N:`)** — used heavily in plan9port mksyslib/mklib, 9legacy bootmkfile. Parsing `target(%):N: prereq` is nontrivial.
3. **Suffix attribute (`:S:`)** — ctSkennerton test15 only. `awktarget:Sawk -f /dev/stdin:` — recipe specified in the attribute.
4. **Null attribute (`:N:`)** — used in archive aggregates. Means "no recipe, this is a hollow rule".
5. **Nested attribute parsing** — `target:V:`, `target:VQ:`, `target:D:`, `(#)`, etc. Ensure attribute stack parsing is correct.
6. **`&` metarule** — `&:n: &.$O` — the `&` matches any target path (Plan 9 mk feature).
7. **`$newprereq` / `$newmember`** — 9legacy bootmkfile uses `$newprereq` in archive rules.
8. **Dual-format include file** — `mkenv` is both valid mk and valid shell (used by both mk and `buildmk`).
9. **`@{}` block syntax** — `for(i in $ARCH) @{ cd $i; mk }` — Plan 9 rc-ish blocks.
10. **Namelists** — Plan 9 mk has namelist syntax for archive members. Not tested in these files.

## Recommendations

### Keep as Integration Tests (complex, multi-file)

| File | Reason |
|------|--------|
| `9legacy/9pc.mkfile` | Most complex — includes, regex rules, archives, double-colon, computed vars, rc blocks |
| `9legacy/portmkfile` | Regex rules + standard metarules |
| `9legacy/bootmkfile` | Archive aggregate pattern |
| `plan9port/src/lib9/mkfile` | Huge file list, cross-references, multi-directory metarules |
| `plan9port/src/cmd/devdraw/mkfile` | Piped includes, Objective-C, conditional install |
| `mksite/mkfile` | Regex rules, rc recipes, directory targets |
| `plan9port/src/cmd/mk/mkfile.test` | mk's own test suite |

### Simplify into Unit Tests (extract patterns)

| Pattern to Extract | Source |
|--------------------|--------|
| Archive aggregate `$(%):N: prereq` | `plan9port/src/mksyslib` |
| `&:n: &.$O` metarule | `plan9port/src/mksyslib` |
| Regex rule `:R:` with `$stem1` | `9legacy/portmkfile` line 2 |
| Variable transform `${OFILES:%=$PLAN9/lib/$LIB(%)}` | `plan9port/src/mksyslib` line 7 |
| Double-colon `:D:` target | `9legacy/9pc.mkfile` lines 92-101 |
| Quiet attribute `:Q:` | `plan9port/src/cmd/devdraw/mkfile` line 48 |
| Piped include `<|cmd` | `plan9port/src/mkhdr` line 25 |
| Metarule with file-wildcard target pattern | `plan9port/src/mkcommon` lines 1-10 |
| Subdirectory recursion pattern | `plan9port/src/mkdirs` |
| Multi-target install pattern | `plan9port/src/mkone`/`mkmany` |
| `:S:` suffix attribute | `ctSkennerton/test15.mk` |
| Nested metarules `%.$O: subdir/%.c` | `plan9port/src/lib9/mkfile` lines 172-176 |

### Already Good as Unit Tests

All 17 `ctSkennerton/test*.mk` files are small, focused, single-feature tests. Use as-is.

## Clean Up

Temporary clones at `/tmp/mksite`, `/tmp/plan9port`, `/tmp/9legacy`, `/tmp/dcjones-mk`, `/tmp/ctsken-mk` can be removed.
