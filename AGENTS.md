# AGENTS.md — mk-rs

> The mk-rust agent's working memory. Thin by design — details live in their
> canonical homes (PLAN, TODO, docs/, cargo doc). This file only tells you
> *where to look before you act*.

---

## Before any work, read

* **PLAN.md** — constraints (what must hold), decisions (what NOT to
  re-propose — tokio, Rc/RefCell, rc-shell, daemon mode, serde feature-gate),
  and next milestones. *Семь раз отмерь, один раз отрежь* (measure
  seven times, cut once).
* **TODO.md** — open work. First unchecked item = next thing to work on.
* **docs/gotchas.md** — operational traps (recipe opacity, `:V:` attributes,
  `MKSHELL` splitting, glob timing, virtual-staleness). Read before touching
  parser, graph, or shell dispatch.
* **CONTRIBUTING.md** — conventions: commit message format, hook gates,
  coverage ratchet.
* **README.md** — what mk-rust is (and is not).

## Before architectural work, check

*Do not re-propose rejected alternatives.* PLAN §Decisions carries the
why-not for each. Regressing to a rejected design is the most common agent
failure mode on this project.

## Before proposing a new file or section

*Look for the canonical home first.* cargo doc owns types and the pipeline
diagram (`//!` in each crate's lib.rs, rendered at docs.rs). Tests own
verifiable claims. PLAN owns constraints/decisions/milestones. TODO owns
open tasks. Do not duplicate what another file already carries — extend
the right one instead.

## Build / test / lint

*Enforced by `.githooks/pre-commit`* (cargo fmt → clippy `-D warnings` →
`cargo test --workspace`) and `.githooks/pre-push` (coverage ratchet at
89.90%, ±0.10% tolerance). See CONTRIBUTING.md §Tier 1 / Tier 2.

## Key references

- `TRACEABILITY.md` — feature matrix (F-xxx → module → phase → status)
- `docs/mk-spec.md` — the spec
- `docs/design-f-045.md` — largest fix to date, documents the parse-time
  variable expansion semantics that underpin current behavior
