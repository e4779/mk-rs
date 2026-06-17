# mk-rust Gotchas

> Operational traps that bite when editing lex/parse/graph/sched or writing
> mkfiles. Each gotcha is a rule plus its reason — what the trap is and why
> it exists. Where the trap came from a known bug, the reason carries the
> failure story (a *scar*, per `Scar_Driven_Agent_Docs`). This is rule+reason
> format, distinct from the aphorism+procedure two-beat used in AGENTS.md —
> don't mix the terms. Read before touching parser internals or authoring
> mkfiles.

---

## Parser / lexer

### Recipe lines after TAB are opaque

Recipe lines are passed verbatim to the shell. `$target`/`$prereq` are
injected as **env vars**, not mk-variable expanded. `=` and `:` inside recipe
text are NOT split by the lexer (`in_recipe` flag).

*Why:* recipes are shell scripts, not mkfile syntax. The lexer must not
re-interpret them.

### `$target` / `$prereq` are env vars, not expanded

Same as above, called out because it's the most common surprise. Inside a
recipe, `$target` resolves via the shell reading the environment mk injected —
not via `var::Scope::expand`. If you change how recipe vars are populated
(`recipe.rs`), do not route them through `Scope`; they bypass it by design.

---

## Rule attributes

### `:V:` is an ATTRIBUTE, not part of the target name

When referencing a virtual target as a prereq, use just the name:
`run: build` (NOT `run: build:V:`). The parser greedily interprets `Word:`
after a target header as attributes — `build:V:` as a prereq gets parsed as
target `build` with bogus attribute chars `V`.

*Why:* the attribute-position grammar is ambiguous with the target-name
grammar; the parser resolves it greedily at the first `Word:`.

### Virtual targets must be explicit (`:V:`)

The graph builder does NOT auto-mark orphan prereqs as virtual. If a target
has no file and no rule, you get an error, not a virtual. Declare `:V:`
explicitly.

*Why:* plan9port mk requires explicit `V` too; auto-marking would hide
missing-rule bugs.

### Virtual targets are unconditionally stale

`:V:` targets have no file to stat, so they fire their recipe on every
invocation regardless of prereq freshness (Bug 4 — `graph.rs::check_stale`).
Downstream targets depending on a virtual also rebuild every time (via
`effective_mtime=None`).

*Why:* this matches plan9port mk semantics. The previous code
(`prereq_stale || arcs_in.is_empty()`) skipped recipes when prereqs were
fresh — that was the bug.

---

## Shell

### `MKSHELL` splits on whitespace

First token is the binary, rest are flags. `MKSHELL=node -e` → the recipe is
invoked as `node -e "recipe"`. `MKSHELL=python3 -c` works the same way.

*Why:* allows any interpreter without a dedicated shell implementation. The
splitting happens in the scheduler before shell dispatch.

---

## Graph

### Glob expansion (`*.toon`) happens at graph-build time

Not at recipe execution time. The graph builder expands globs when wiring
prereqs into arcs; by the time the recipe runs, prereqs are already concrete
file lists.

*Why:* the DAG must be concrete before staleness check and parallel dispatch.

---

## Release

### `cargo publish` creates a tarball without `.git`

`build.rs` reads the `GIT_HASH` file first, falls back to `git rev-parse`. CI
creates `crates/mk-cli/GIT_HASH` before publish so the version string is
correct in the published binary.

*Why:* without this, `cargo install mk-rs` would show `(unknown)` for the
git hash in `mk --version`.

---

## SEE ALSO

- `PLAN.md` §Decisions — strategic why-not (do not re-propose these alternatives)
- `CONTRIBUTING.md` — commit conventions, hook gates, coverage ratchet
- `TRACEABILITY.md` — feature matrix (which spec row each module implements)
