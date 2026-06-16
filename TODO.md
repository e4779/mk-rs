# TODO.md — mk-rust

> Execution companion to [PLAN.md](PLAN.md).
> PLAN.md says *where we are going*; TODO.md tracks *the concrete steps*.
> Cross-referenced to [TRACEABILITY.md](TRACEABILITY.md) (feature matrix) and
> [docs/](docs/) (specs). Update at every session boundary.

Status legend: `[ ]` pending · `[~]` in progress · `[x]` done · `[!]` blocked

---

## Current focus

Land **F-045** (rule-header variable evaluation) — the largest correctness fix
since v0.2.0. It restores incrementality in the only aggressive production user
(`invest-research`) and unblocks three spec rows that TRACEABILITY was marking
`✓` dishonestly (F-045, F-002, F-039, F-042). Full spec: `docs/design-f-045.md`.

---

## F-045 — Rule-header variable evaluation  `[x]` **DONE**

Spec & contract: [`docs/design-f-045.md`](docs/design-f-045.md). Implemented by
rust-coder. **Verified by architect** (not just trusted from the report):

- Gates: **335 tests** (was 309, +26), `cargo clippy --all-features -D warnings`
  clean, `cargo build` green.
- Empirically against the reference `mk`: S1 (read-time `early`), S2/S3
  (assign+backtick), S4 (word-list → 2 prereqs), S7 (recipe-var empties),
  S10 (CLI sticky-override), R-REC (cycle→empty, 1000-deep→leaf, no error/hang).
- sh-style backtick in assignment → expands into prereqs correctly.
- TRACEABILITY F-045/F-002/F-039/F-042 now honest; BUGS.md entry added.

**Known gaps left (out of scope, tracked separately):**
- S11b/c (literal-glue / quoted-space) → **F-003a** (lexer quoting divergence).
- **invest-research dictionary NOT fixed** — see F-063 below. F-045 is correct;
  the production mkfile hits a *different* pre-existing gap (rc-style backtick).

## F-063 — rc-style backtick `` `{cmd} `` not parsed  `[ ]` **DIAGNOSED**

Uncovered by F-045 verification on `invest-research`. The production mkfile
uses rc-style backtick `` DATA = `{fd -e toon ...} ``. mk-rust leaves it
**literal** — so `$DATA_TOONS` stays `` `{fd ...} `` and the dictionary
target's prereqs never wire up (incrementality broken in invest-research).

### Root cause (verified vs `/usr/local/plan9/src/cmd/mk/`)

**This is NOT a shell-dialect issue.** `` `{cmd} `` vs `` `cmd` `` is a
**mkfile-level lexer feature, shell-independent**. `lex.c::bquote` (lines
75-81) reads the backtick then branches on the NEXT char: `{` → rc-style
(term = `}`), else sh-style (term = `` ` ``). The command inside is then
exec'd via the active shell regardless of style.

Empirical proof: plan9port mk expands `` `{echo one} `` correctly **even
under `MKSHELL=sh`**. So the form is parsed before any shell sees it.

### Fix (small, isolated)

`crates/mk-core/src/lex.rs::read_backtick` (lex.rs ~149) currently reads
only sh-style (until closing `` ` ``). Add the rc-style branch mirroring
plan9port: after consuming the opening `` ` ``, peek; if `{`, consume it and
read until `}`; else read until `` ` ``. Shell-independent — do NOT gate on
`ShellMode`.

### Status

- mk-rs v0.2.1 (released) does NOT have this — invest-research must use
  sh-style `` `cmd` `` meanwhile (see `docs/mk-rs-v0.2.1-note.md` in
  invest-research).
- Target for v0.2.2 (small patch). Low risk — isolated lexer function.

## F-003a — Quoting in values (lexer strip in non-recipe mode)  `[ ]`

Follow-up to F-045, **orthogonal layer** (lexer, not parse/var). Currently
hidden under F-003 (`✓`) in TRACEABILITY — same dishonesty pattern as F-045.

- mk-rust lexer keeps quote chars in `Word` (`read_double_quoted` does
  `word.push('"')`); reference plan9port strips them in non-recipe mode and
  treats a quoted span as one word.
- Exposed by F-045 in rule headers (S11b literal-glue, S11c quoted-space) but
  does **not** block F-045's common case (whole-word `$VAR` → split, S11a).
- Must respect `in_recipe` flag — recipes must keep quotes (sh expects them).
- **Low urgency:** no mkfile in the corpus (invest-research, Harvey, Inferno,
  plan9port) uses quoted-spaces-in-values.

Tracked as TRACEABILITY row F-003a (`◐`). Details: `docs/design-f-045.md` §7
(quoting divergence) + §10 (QC-1/QC-4/QC-7).

---

## Documentation — structural gaps (post-F-045)

Surfaced by audit against `~/wiki/pages/plan-md-best-practices.txt` and
`Scar_Driven_Agent_Docs.txt` / `Two_Beat_Agent_Docs.txt`.

- [ ] **Restructure PLAN.md** (1380 → ~150 lines). Fails the 30-second reboot
  test: no "Current Focus", §3 Module design (600 lines) is ARCHITECTURE.md
  material, mixes spec+architecture+design-log. Move §3 → new `ARCHITECTURE.md`;
  keep Current Focus / Goal / Constraints / Decisions / Next Milestones per
  best-practices template. **Separate session** — large edit.
- [ ] **Update AGENTS.md gotcha** on F-045. Current line ("`$target`/`$prereq`
  are injected as env vars, not mk-variable expanded") becomes **partially
  false** after F-045 (assignment RHS + rule headers WILL expand; only
  recipe-time injection stays true). Rewrite as a scar per two-beat pattern
  (aphorism + procedure). Wait until F-045 lands.
- [ ] **Keep BUGS.md scar-driven** — it already follows the format (Found /
  Expected / Actual / Root cause / Fix / Commit). Add the F-045 resolved entry
  as part of F-045 P6.

---

## TODO file hygiene

- Move items here when they survive a single session (sprint-spanning work).
- Check the box when done; do **not** delete history — archive to a
  `CHANGELOG.md` if this file grows past ~80 lines.
- Every item links to its spec (`docs/...`) or TRACEABILITY row so context is
  one click away (progressive disclosure).
