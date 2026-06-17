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

*Scars:* `eb44489` — `=` and `:` were tokenized inside recipes, breaking
NodeShell mkfiles that use `var=expr` and `key:value` syntax. `ac0931b` —
backticks in recipes were being processed as mk command substitution
instead of passed through to the shell.

### `$target` / `$prereq` are env vars, not expanded

Same as above, called out because it's the most common surprise. Inside a
recipe, `$target` resolves via the shell reading the environment mk injected —
not via `var::Scope::expand`. If you change how recipe vars are populated
(`recipe.rs`), do not route them through `Scope`; they bypass it by design.

*Why:* inherited from plan9port mk — recipes see target/prereq via the
environment. No known scar in mk-rust; called out because agents new to the
project reach for `Scope::expand` first.

---

## Rule attributes

### `:V:` is an ATTRIBUTE, not part of the target name

`:V:` attaches a virtual attribute to a *rule's target*, written in the
`target:VQ:` position between the target name and its prerequisites. It is
never written on a prereq reference. Writing `run: build:V:` does NOT make
`build` a virtual prereq — after the first `:` ends `run`'s target header,
mk-rust's parser sees `build` followed by `:` and tries to parse it as an
attribute block (same `target:VQ:` syntax), then fails with
`unknown attribute b`. Use just the name: `run: build`. Declare `:V:` on the
rule that *defines* the virtual: `build:V:`.

*Why:* the `target:<attrs>: prereq` and `target: <prereq-name>` grammars
share the first `:`; mk-rust resolves the ambiguity positionally. Attributes
belong on the rule declaring the target, not on references to it.

*Divergence note:* plan9port mk reports the same `run: build:V:` differently
(`don't know how to make 'build:V:'` — treats it as a literal prereq name).
Both reject the input; only the error differs.

*Scar:* none — preventive guard. The parser's attribute grammar would reject
this input even if no one had ever hit it; the gotcha exists to spare agents
the debugging time of decoding the `unknown attribute b` error.

### Virtual targets must be explicit (`:V:`)

The graph builder does NOT auto-mark orphan prereqs as virtual. If a target
has no file and no rule, you get an error, not a virtual. Declare `:V:`
explicitly.

*Why:* inherited from plan9port mk — auto-marking would hide missing-rule
bugs. No known scar in mk-rust; the rule is preventive.

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
splitting happens in the scheduler before shell dispatch. Architectural rule;
no known scar.

---

## Graph

### Glob expansion (`*.toon`) happens at graph-build time

Not at recipe execution time. The graph builder expands globs when wiring
prereqs into arcs; by the time the recipe runs, prereqs are already concrete
file lists.

*Why:* the DAG must be concrete before staleness check and parallel dispatch.
Architectural rule; no known scar.

---

## Release

### `cargo publish` creates a tarball without `.git`

`build.rs` reads the `GIT_HASH` file first, falls back to `git rev-parse`. CI
creates `crates/mk-cli/GIT_HASH` before publish so the version string is
correct in the published binary.

*Why:* without this, `cargo install mk-rs` would show `(unknown)` for the
git hash in `mk --version`. Preventive CI design; no known scar — caught
during the first release prep, not after a bad publish.

---

## SEE ALSO

- `PLAN.md` §Decisions — strategic why-not (do not re-propose these alternatives)
- `CONTRIBUTING.md` — commit conventions, hook gates, coverage ratchet
- `TRACEABILITY.md` — feature matrix (which spec row each module implements)
