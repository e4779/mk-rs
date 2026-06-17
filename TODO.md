# TODO.md — mk-rust

> Execution companion to [PLAN.md](PLAN.md). Cross-references
> [TRACEABILITY.md](TRACEABILITY.md) and [docs/](docs/).
> Order = priority: the first open item is the next thing to work on.

---

## Current focus

**Documentation restructuring in progress** (#22). v0.2.2 shipped (F-045, F-063,
Bug 4). Infrastructure wave landed (pre-commit hooks, coverage ratchet,
git-cliff CHANGELOG, conventional commits). PLAN.md being slimmed from 777 →
~130 lines. Bug A/B (`bugfix/pattern-rule-stem-extraction` branch) held for
merge after docs (#39).

---

## Open work

### `-s` flag resolution (plan9port compat)

**Conflict:** plan9port mk uses `-s` for "sequential" (force `NPROC=1`).
mk-rust uses `-s` for "silent" (suppress recipe printing). This breaks
compatibility with mkfiles that pass `-s` expecting sequential behavior.

**Decision (PLAN §6.7, tentative):** rename silent → `-q` (quiet), reserve
`-s` for sequential. The `Q` attribute already handles per-rule silence.

**Steps:**
- [ ] Rename `-s`/`--silent` to `-q`/`--quiet` in clap args (`crates/mk-cli/src/main.rs`)
- [ ] Add `-s`/`--sequential` → forces `NPROC=1` (same as current `-s` in plan9port)
- [ ] Update tests, TRACEABILITY F-052 (`-s` sequential)
- [ ] Update README flag reference, `docs/mk.1.md` man page
- [ ] Regenerate CHANGELOG via `git-cliff`

### F-003a — Quoting in values (lexer strips quotes in non-recipe mode)

Orthogonal layer to F-045 (lexer, not parse/var). Currently hidden under F-003
(`✓`) in TRACEABILITY — same dishonesty pattern F-045 had.

- mk-rust lexer keeps quote chars in `Word` (`read_double_quoted` pushes `"`);
  reference plan9port strips them in non-recipe mode and treats a quoted span
  as one word.
- Exposed by F-045 in rule headers (S11b literal-glue, S11c quoted-space) but
  does **not** block F-045's common case (whole-word `$VAR` → split, S11a).
- Must respect the `in_recipe` flag — recipes keep quotes (sh expects them).
- **Low urgency:** no mkfile in the corpus (invest-research, Harvey, Inferno,
  plan9port) uses quoted-spaces-in-values.

Details: `docs/design-f-045.md` §7 (quoting divergence) + §10 (QC-1/QC-4/QC-7).
Tracked as TRACEABILITY row F-003a (`◐`).

### AGENTS.md slimming — deferred (#30)

Project AGENTS.md should become shortest ("read README, key docs at X, build
commands Y"). Why-not entries (currently transitional in AGENTS) + gotchas
migrate to `APPEND_SYSTEM.md` (global, already extended with wiki-first +
conventional-commits sections in #33). Two-beat audit on gotchas after
migration. Blocked on completing #22 (PLAN restructure) first.

---

## Hygiene

- An item earns a place here only when it survives a single session.
  Session-scoped steps live in the task system (`TaskCreate`), not here.
- **Delete done items** — history lives in `git log` + release tags + CHANGELOG,
  not here. Done items break the 30-second scan.
- Every item links to its spec (`docs/...`) or TRACEABILITY row (progressive
  disclosure; TODO.md is the index, `docs/` is the detail).
- Keep under ~80 lines of open items. If it grows, the problem is too many
  open tasks, not file organization — close some.
- See `~/wiki/pages/Companion_Files_Best_Practices.txt` for the rationale.
