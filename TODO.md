# TODO.md — mk-rust

> Execution companion to [PLAN.md](PLAN.md). Cross-references
> [TRACEABILITY.md](TRACEABILITY.md) and [docs/](docs/).
> Order = priority: the first open item is the next thing to work on.

---

## Current focus

**v0.2.2 shipped** (F-045 rule-header variables, F-063 rc-style backtick, Bug 4
virtual-staleness). Republished to gitverse + github; invest-research unblocked.
Now restructuring project documentation: `cargo doc` migration of PLAN §3 is in
flight, PLAN.md rewrite (1380 → ~130 lines) follows. See task #22 / #31.

---

## Open work

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

### AGENTS.md slimming — deferred

Project AGENTS.md should become shortest ("read README, key docs at X, build
commands Y"). Most current content (architecture decisions, gotchas) migrates
to a global `APPEND_SYSTEM.md`. Two-beat audit on gotchas after migration.
Blocked on the APPEND_SYSTEM.md work (separate effort). Task #30.

### CHANGELOG via git-cliff — deferred, not urgent

Auto-generate CHANGELOG.md from conventional commits (git-cliff + cliff.toml).
Replaces hand-writing; git remains source of truth. Pair with APPEND_SYSTEM.md
conventional-commit conventions. Current release flow (CI publishes on tag,
GitHub Releases show commits between tags) works fine meanwhile.

---

## Hygiene

- An item earns a place here only when it survives a single session.
  Session-scoped steps live in the task system (`TaskCreate`), not here.
- **Delete done items** — history lives in `git log` + release tags, not here.
  Done items break the 30-second scan.
- Every item links to its spec (`docs/...`) or TRACEABILITY row (progressive
  disclosure; TODO.md is the index, `docs/` is the detail).
- Keep under ~80 lines of open items. If it grows, the problem is too many
  open tasks, not file organization — close some.
- See `~/wiki/pages/Companion_Files_Best_Practices.txt` for the rationale.
