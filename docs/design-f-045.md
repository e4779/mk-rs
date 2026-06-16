# Design: F-045 — Rule-Header Variable Evaluation

> Status: **spec** (ready for implementation)
> Owner: architect (this doc), rust-coder (implementation)
> Blocks: F-039 (namelist), F-063 (backtick) are exercised by the same pass
> Found via: invest-research pipeline (session `019ecdd2`) + plan9port reference
> Reference corpus: `testdata/external/{harvey,inferno}/` + wiki `mkfile-corpus-extended`

## 1. Problem

mk-rust does **not** expand variables in rule headers (target / prereq /
attribute sections). The spec F-045 says they are expanded when the rule is
read. The reference plan9port mk (`/usr/local/plan9/bin/mk`) does expand them.
mk-rust stores them literally. `TRACEABILITY.md` marks F-045 as `✓` — this is
false and actively misled an agent (see session `019ecdd2`).

Two related gaps share the same root cause:

| # | Gap | mk-rust now | Reference mk |
|---|-----|-------------|--------------|
| 1 | Rule-header targets/prereqs | literal `$VAR` | expanded |
| 2 | Assignment RHS (`A = b $C`) | literal (only backtick runs in `Scope::set`) | recursively expanded |
| 3 | Namelist `${VAR:%=%}` in header | implemented in `Scope::expand`, never applied to headers | applied |

### Evidence (all verified against `/usr/local/plan9/bin/mk`, sh-independent)

```
SRCS = foo.txt bar.txt
target: $SRCS
        echo PREREQ=[$prereq]
```
- plan9port mk → `PREREQ=[foo.txt bar.txt]`
- mk-rust      → `PREREQ=[$SRCS]`  ❌

Production impact: in `~/dev/invest-research/mkfile`,
`DATA_TOONS = \`{fd -e toon ... data/}` then `data/processed/dictionary.toon: $DATA_TOONS`
— `$DATA_TOONS` is treated as a literal non-existent file, so adding a new
`.toon` does **not** rebuild the dictionary (broken incrementality).

## 2. Semantics contract (empirically verified)

These are the behavioral requirements the implementation MUST meet. Each was
confirmed by running the reference `mk`.

| ID | Behavior | Verified result |
|----|----------|-----------------|
| S1 | **Read-time expansion.** A rule header uses the variable's value *at the line where the rule is read*, not the final value. `TARG=early; t: $TARG; TARG=late` → prereq = `early` | ✓ |
| S2 | **Assignment RHS is recursively expanded** at assignment time. `A=world; B=hello $A` → B = `hello world` | ✓ |
| S3 | **Backtick runs at assignment time** (already works via `Scope::set` → `expand_backtick`) | ✓ |
| S4 | **A variable is a word list.** `SRCS=a.c b.c; t: $SRCS` produces TWO prereqs (`a.c`, `b.c`), not one string. Applies to targets too | ✓ |
| S5 | **Namelist transform** `${VAR:A%B=C%D}` works in target AND prereq position | ✓ |
| S6 | **`$$` → `$`** in headers, then re-scanned (recursive, same as recipes) | ✓ |
| S7 | **Recipe-time vars (`$target`, `$prereq`, `$stem`, `$alltarget`, `$newprereq`, `$newmember`, `$pid`, `$nproc`) are NOT defined at parse time** → expand to empty string. `$prereq` in a header silently empties (can break the target name); this matches the reference | ✓ |
| S8 | **Include path is expanded** — variables AND backtick. `< $INCL` and `` < `{echo sub.mk} `` both resolve before opening; `<| $CMD` expands vars in the command | ✓ |
| S9 | **Target/prereq variables expand at parse time; the attribute-word does NOT.** `${objtype}l.h:Q: input` → target `amd64l.h`, then `Q` parsed. A `$` inside the attribute-word is a syntax error (`unknown attribute '$'`). The `P` program string is stored LITERAL in the AST and expanded at **call-time** (when the staleness check runs the program), using the variable's final value — not the read-time value | ✓ (QB-1..QB-9) |
| S10 | **CLI assignments (`mk VAR=value`) are *sticky*: parsed FIRST, before the mkfile, and the mkfile cannot override them.** A CLI var is visible to every rule header (read-time semantics S1 still hold — the CLI value is what's in scope). CLI vars also propagate into mkfile assignment RHS (`DERIVED = $BASE/extra` with `mk BASE=/opt` → `/opt/extra`) and into include paths. There is NO mkfile syntax to force-override a CLI var | ✓ (F42-1..F42-7) |
| S11 | **Word-splitting of expanded values follows the reference's `Word*`-list model, NOT naive `split_whitespace`.** A variable holds a LIST of words whose boundaries were fixed at definition time (quotes stripped: `"a b".c` → one word `a b.c`; `a.c b.c` → two words). When `$VAR` is used as a whole word in a header, EACH stored word becomes its own target/prereq. When a literal is glued to the variable (`pre.$VAR` / `$VAR.x`), the reference's `nextword` merges the literal onto the first/last stored word and keeps middle words separate. | ✓ (QC-1..QC-8) |
| S12 | **Recursion semantics for assignment RHS.** A variable's stored value is the fully-resolved value at the moment of assignment (read-time, single-level substitution per line, recursing through already-stored values). Deep non-cyclic chains (tested to 1000 levels) resolve completely — there is **no fixed depth limit**. Cycles (`A=$B; B=$A`) terminate gracefully by treating the revisited variable as empty, yielding empty or partial-literal results — **never an error, never a hang.** | ✓ (D-1..D-15, D-8) |
| S13 | **A variable may expand to structurally-significant text in a target/prereq position** (NOT in the attribute-word — see S9). A target `$PAT` where `PAT=%.o` produces a metarule (E-2: `%` survives expansion and is matched as a metarule). A target `$NAMES` where `NAMES=alpha beta` produces **multiple rules** — one per word (E-3: both `alpha` and `beta` resolve to the recipe). A target `${objtype}l.h` with a literal attribute `:Q:` works (E-1). | ✓ (E-1..E-3) |

### Expansion order within one value
backtick (exec) → variable ref (`$VAR` / `${VAR}`) → namelist (`${VAR:%=%}`)
→ recursive re-scan. Backtick must run once at the assignment moment (it already
does in `Scope::set`); the new work is wiring `Scope::expand` into the parse
pass.

### S2-detail — how assignment-RHS recursion actually works (verified)

The reference does **NOT** store the raw RHS and expand at use; it expands the
RHS **at assignment time**, substituting each `$X` with the *currently stored*
value of X, and recursing. Concretely (D-12..D-15):

- `A=aa; B=$A; C=$B` → B is stored as `aa`, C is stored as `aa` (because B's
  stored value is already `aa`, not `$A`). Deep chains (1000 levels, D-8)
  resolve fully — there is **no depth limit**.
- **Order matters at assign time (read-time).** `GREETING=$FIRST world; FIRST=hello`
  → GREETING stored as ` world` because FIRST was empty at that line (D-12).
- **Cycles resolve to empty/literal-remainder, NOT an error.** `A=$B; B=$A` →
  both `[]` (D-1); `A=a$B; B=b$C; C=c$A` → A=`a`, B=`b`, C=`ca` (D-10). The
  reference terminates a cycle by treating the revisited variable as empty,
  it does **not** raise an error.

**Implication for the current mk-rust code:** `Scope::expand` is fixed-point
iteration with a depth-10 cap that returns `VarError::RecursiveExpansion`.
This is **wrong** on two counts after F-045:
  (a) depth-10 rejects legitimate deep chains (reference handles 1000);
  (b) cycles must yield empty/partial, not an error.
The fix: replace the depth cap with a **visited-set cycle detector** — track
variable names currently being expanded; on revisit, substitute empty string
and continue (matching reference). See §7 risk R-REC.

### What does NOT expand at parse time (verified)
- **Attribute-word** (the token between the two colons, `target:ATTR: prereq`).
  Attribute chars are parsed byte-by-byte; a `$` there is `unknown attribute`.
  Do not run `Scope::expand` on the attribute token. (QB-3/QB-4 syntax errors.)
- **P-program string** (the text captured after `P`). Stored literal; expanded
  later when the P program is invoked during staleness check. Uses the
  variable's **final** value, not the read-time value. (QB-9: CMPROG changed
  after the rule; the final value was the one invoked.)
- **Env-imported values containing `$`.** `DOLLAR_VAR='price$5' mk ...` keeps
  `$5` literal; it is NOT re-expanded as a variable ref. (QU-1.)

### Out of scope (do NOT change)
- Recipe bodies are expanded at **execution** time (F-046), not parse time.
  Leave recipe text literal in the AST; the scheduler/shell expand it. Do not
  touch `recipe.rs`, `sched.rs`, `graph.rs` semantics.
- `graph.rs::build_graph` needs **no** scope — rule headers arrive already
  expanded in `stmts`. Stem substitution (`%`/`&` → `$stem`) is unrelated and
  stays as-is.
- Metarule matching works on already-expanded patterns.

## 3. Current vs target pipeline

### Current
```
lex::tokenize(input) -> tokens
parse::parse(tokens) -> Vec<Stmt>          // LITERAL targets/prereqs/values
// main.rs:
scope = builtin_scope(); import_env(scope)
for Assign(a) in stmts: scope.set(...)      // backtick only, no $VAR expansion
build_graph_with_nrep(stmts, targets, nrep) // headers still literal
```

### Target (Variant A — faithful, mirrors plan9port `parse.c`/`word.c`)
```
lex::tokenize(input) -> tokens
// main.rs:
scope = builtin_scope(); import_env(scope)
parse_with_scope(&tokens, &mut scope) -> Vec<Stmt>   // EXPANDED inline, scope grows
build_graph_with_nrep(stmts, targets, nrep)      // headers already expanded
```

plan9port reference: `rhead()` → `stow()` → `nextword()` → `varsub()` does the
expansion **inside the parser**, against a symtab that grows as assignments are
read. We mirror that: the parser carries a `&mut Scope` and expands each word
against the scope-as-it-exists-at-this-line.

### Why a single post-parse pass is rejected (Variant B)
A pass after `parse()` would use the **final** scope, breaking S1 (read-time
semantics). `TARG=early; t:$TARG; TARG=late` would wrongly give `late`.
Variant A is required for mkfile compatibility.

## 4. Implementation changes

### 4.1 `var.rs` — `Scope` (minor)

1. **`Scope::set`**: currently calls `expand_backtick` only. Change so the
   stored value is fully expanded: backtick → variable → namelist (recursive,
   with the visited-set cycle detector from R-REC). Keep the public signature
   `set(&mut self, name, value, prec) -> bool`. Add `set_raw(name, value,
   prec)` that stores literally (no expansion) for the cases below.
   - **Who calls `set` (production, non-test — verified):**
     - `builtin_scope()` (var.rs:353-365) — builtins (CC, NPROC, MKSHELL…).
       Values are literal; use `set_raw` for consistency (harmless either way).
     - `import_env` (var.rs:373) — env values at `Environment`. **MUST use
       `set_raw`** — env values containing `$` must NOT be re-expanded (QU-1).
     - main.rs:147 — the post-parse assign loop at `Mkfile`. This loop is
       **removed** in §4.4 (parse() now does assign-time expansion itself), so
       it is not a caller after the refactor.
   - **`set_force` is NOT the recipe-time path.** Recipe-time vars (`$target`,
     `$prereq`, `$stem`, `$alltarget`, `$newprereq`, `$newmember`, `$pid`)
     bypass `Scope` entirely: they are injected into a **plain `HashMap`** in
     `recipe.rs:172-182` (`env.insert("target", …)`). `set_force` is currently
     only exercised by tests (var.rs:410). **Implication:** F-045's assign-time
     expansion in `Scope::set` can NEVER corrupt recipe-time variables — they
     are immune by construction. No `set_force` change is required for
     correctness; leave it as-is (or switch it to `set_raw` for clarity, but
     it does not affect recipe execution).

2. **`Scope::expand`** already handles `$VAR`, `${VAR}`, `${VAR:%=%}`, `$$`,
   recursive re-scan. Verify it composes with the backtick step. No structural
   change expected; possibly fold `expand_backtick` into the front of `expand`
   so there is one entry point used by both `set` and the parser.

### 4.2 `parse.rs` — parser becomes scope-aware (core change)

1. Change public API:
   ```rust
   // convenience for tests/library: builtins + env, no mkfile assigns
   pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError>
   // full control: caller provides the scope (builtins+env already in it)
   pub fn parse_with_scope(tokens: &[Token], scope: &mut Scope) -> Result<Vec<Stmt>, ParseError>
   ```
   `parse()` builds a fresh `builtin_scope()` + `import_env`, delegates to
   `parse_with_scope`. This keeps the library API usable and tests green.

2. `parse_with_includes` (renamed/updated) threads `scope` through. At each
   statement:
   - **`Assign`**: after collecting raw `value`, call `scope.set(name, value,
     Mkfile)` (which now fully expands per 4.1). Store the **expanded** value in
     the returned `Assign { value }` (so downstream sees the real value and the
     post-parse scope-build loop in main.rs becomes a no-op / is removed).
   - **`Rule`**: after collecting raw `targets` and `prereqs` word-by-word,
     expand EACH word with `scope.expand`, then split the result into
     possibly-multiple words per S11. **`split_whitespace` is a minimum-viable
     splitter but is NOT fully correct** — see §7 (quoting) and S11. For the
     common case (`$VAR` as a whole whitespace-delimited token) it is correct.
     Replace the word lists with the expanded+split vectors. Do this **before**
     attribute parsing for the target side is finalized (S9 — but see ordering
     note below).
   - **GNU-make `$(...)` rejection** (`parse.rs:295`): keep, but run on the
     RAW (pre-expansion) prereq tokens so `$(` in source is still caught.

   **S9 ordering — attribute parsing.** The current code parses attributes from
   the token between the two colons (`target:VQ: prereq`). Expand ONLY the
   target tokens and prereq tokens. Do **not** expand the attribute token —
   verified: `$` in the attribute-word is a syntax error in the reference, and
   the `P` program is expanded later at call-time (see §2 "What does NOT
   expand"). Keep the attribute token untouched and let `parse_attributes`
   run on it as today.

3. `parse_recipe` — **unchanged**. Recipe text stays literal (F-046).

### 4.3 `include.rs` — path expansion (S8)

`IncludeContext::include_file` and `include_command` receive the path/command
as joined words. Expand via `scope.expand` before resolving/opening. Thread
`&Scope` (or `&mut`) into `IncludeContext` or pass it to the include methods.
`<| command` should also expand variables in the command (reference does).

### 4.4 `main.rs` — reorder

1. Build scope (builtins + env) **before** parse.
2. Call `parse_with_scope(&tokens, &mut scope)`.
3. **Remove** the post-parse loop that re-sets assigns (it would re-expand and,
   worse, re-run backtick). The scope is already fully populated by parse.
4. CLI command-line assignments (`mk VAR=value`, F-042) — apply to scope with
   `Precedence::CommandLine` **before** parse so they win (S1 + precedence).
   This also implements F-042 (currently a TRACEABILITY gap). Parse `args`
   containing `=` before invoking parse.

### 4.5 `lib.rs` / public API
Re-export `parse_with_scope`. Update the README library example if needed.

## 5. Test plan

Tests live next to the module under test. New unit tests in `parse.rs`,
`var.rs`; regression/integration where appropriate.

### 5.1 Contract tests (from this design — must pass)
- `t_S1_read_time_expansion` — override-between-rules gives the early value.
- `t_S2_assign_rhs_recursive` — `B = hello $A`.
- `t_S4_var_is_word_list` — `SRCS=a.c b.c; t: $SRCS` → 2 prereqs; also targets.
- `t_S5_namelist_in_header` — `${FILES:%=%.ps}` in target; `${LIBOFILES:%=$LIB(%)}` in prereq.
- `t_S6_dollar_dollar_in_header` — `$$` → `$` then re-scan.
- `t_S7_recipe_var_in_header_empties` — `$prereq` in target → empty.
- `t_S8_include_path_expanded` — `< $INCL`.
- `t_S9_var_target_before_attrs` — `${objtype}l.h:Q:`.
- `t_S12_recursion_deep_and_cycles` — (a) 1000-deep chain resolves to leaf;
  (b) `A=$B; B=$A` → both empty (no error, no hang); (c) 3-cycle `A=a$B,
  B=b$C, C=c$A` → A=`a`, B=`b`, C=`ca`.
- `t_S13_var_expands_to_structural_text` — (a) `$PAT` with `PAT=%.o` →
  metarule (E-2); (b) `$NAMES` with `NAMES=alpha beta` → two rules, both
  resolvable (E-3); (c) `${objtype}l.h:Q:` builds (E-1).

### 5.2 Corpus regression (from `mkfile-corpus-extended`)
For each, assert the rule headers parse to the EXPECTED expanded form:
- Harvey `sys/src/cmd/mkfile` — `$cpuobjtype._cp` target, `$PROGS` prereq,
  `$BIN/%: $O.%` metarule (pattern 3.1).
- Harvey `sys/src/cmd/fossil/mkfile` — `${LIBOFILES:%=$LIB(%)}` (pattern 2.1).
- Harvey `sys/src/9k/k10/mkfile` — `${objtype}l.h:DQ:` (pattern 4.1),
  `$p$CONF` concatenation (pattern 6.1).
- Inferno `mkfiles/mkdis` — `$DISBIN/%.dis` metarule (pattern 3.3).

These corpus files are reference truth; if a header expands differently than
the documented expectation, that is a bug.

### 5.3 No-regression
- Existing `parse.rs` tests that asserted literal `$prereq` in prereq position
  (e.g. `assert_eq!(r.prereqs, vec!["$prereq"])` at parse.rs ~802) must be
  updated to the new (empty, per S7) expectation, with a comment citing S7.
- `cargo test` must stay green; target 309→ (309 + new).
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- End-to-end: the `/tmp/f45q` reproductions (Q1–Q4, Q-precise) produce the
  reference output.

### 5.4 Production check
- In `~/dev/invest-research/`, `mk -n data/processed/dictionary.toon` shows
  real `.toon` files as prereqs (not literal `$DATA_TOONS`), and
  `mk-graph --target data/processed/dictionary.toon` lists them as arcs.

## 6. Documentation updates

- `TRACEABILITY.md`: F-045 — change note from "done (but actually broken)".
  F-045, F-002 stay `✓` (now genuinely). Consider F-039 (namelist) and F-042
  (cmd-line assign) — update to `✓` where the pass now covers them; otherwise
  mark `◐` honestly.
- `BUGS.md`: add a resolved entry documenting F-045 (found via invest-research,
  root cause, fix pointer to this doc + commit).
- `README.md`: the library example (`parse(&tokens)`) still works; add a note
  that `parse_with_scope` is available for pre-seeded scopes.
- wiki `Mk_Rust.txt` / `mk-spec.txt`: note F-045 now implemented.

## 7. Risks & edge cases

- **Re-running backtick.** If a post-parse loop survives anywhere, backtick
  re-executes. Audit main.rs and any caller of `parse` to ensure scope is built
  once, before parse, and not re-set after.
- **Env values with `$`.** `import_env` must use a non-expanding setter (4.1)
  so an env var literally containing `$HOME` is not mangled.
- **Infinite recursion (R-REC).** `A = $A` or `A=$B; B=$A`. The reference does
  **NOT** treat these as errors and does **NOT** hang: it terminates cycles
  by treating a revisited variable as empty (D-1 → `A=[]`; D-10 3-cycle →
  partial literal results). The current `Scope::expand` is **wrong for F-045**
  on two counts: (a) its depth-10 cap rejects legitimate deep chains that the
  reference handles (verified 1000 levels, D-8); (b) it returns
  `VarError::RecursiveExpansion`, but the reference yields empty/partial.
  **Required change (blocks S12):** replace the fixed depth-10 cap with a
  **visited-set cycle detector** — carry a `HashSet<String>` of variable
  names currently mid-expansion; on revisit, substitute the empty string and
  continue. Deep chains then work (no artificial limit); cycles terminate
  gracefully matching the reference. The parser must still guard against
  pathological input but should never raise on a mere cycle.
- **Glob vs variable — KNOWN DIVERGENCE, not a bug to fix here.** The reference
  plan9port mk does **not** expand globs in prereqs at all (QU-2a: literal
  `globsub/*.c` → `don't know how to make`). mk-rust implements F-066 (glob in
  prereqs via `expand_globs` in `graph.rs`) — a feature beyond the reference.
  After this fix, `expand (parse) → glob (graph)` still applies in mk-rust: a
  variable expanding to `*.c` will then be globbed by `expand_globs`. This is
  consistent with mk-rust's own (intentional) F-066 behavior, even though it
  differs from the reference. Add a test: `PAT=*.c; t: $PAT`. Do NOT try to
  make mk-rust match the reference's no-glob behavior in this fix — F-066 is
  tracked separately in TRACEABILITY and is a deliberate divergence.
- **Attribute-word variables** (`P$PROG`) — RESOLVED by QB-9: the attribute
  token is NOT expanded at parse time; `P` program is literal and expanded at
  call-time. No parser work; leave `parse_attributes` untouched.
- **Quoting divergence (QC-4) — PRE-EXISTING, exposed by F-045, scoped out.**
  The reference strips quote characters at definition time (`"a b".c` → value
  `a b.c`, one word). mk-rust's lexer KEEPS the quote chars in the `Word`
  token (see `lex.rs::read_double_quoted`, which `word.push('"')`). This is
  independent of F-045 but F-045 makes it observable in rule headers. Two
  sub-issues, both tracked but NOT required for the F-045 MVP:
  1. **Quote preservation.** To match the reference, assignment RHS values
     should be quote-stripped before storage. This is a lexer/`Scope::set`
     change. Out of scope here — file a follow-up. mk-rust recipes rely on
     quote preservation (recipes pass verbatim to sh), so a full fix must be
     careful not to break recipe lines (where the lexer is in `in_recipe`
     mode and quotes are passed through).
  2. **Literal-glue word splitting (S11b).** `pre.$VAR` / `$VAR.x` where `$VAR`
     holds 2+ words: the reference merges the literal onto the first/last word
     (`pre.one`, `two`). A naive `split_whitespace` of the fully-merged string
     `pre.one two` actually produces the right two words for QC-5 by luck, but
     `$NAME.x` → `foo bar.x` would split as `foo` + `bar.x` only if the merge
     happens BEFORE split. The reference does merge-then-split via `Word*`
     list. Recommendation: implement whole-word `$VAR` splitting (S11a)
     correctly now; for literal-glue cases, expand the merged token then
     `split_whitespace` — document that quoted-space inside a glued literal is
     a known gap (rare in real mkfiles). Add S11a/S11b/S11c tests with the
     expected (possibly-divergent) mk-rust result clearly annotated.
- **Public API break.** `parse(tokens)` signature is unchanged (convenience
  wrapper). Only adds `parse_with_scope`. No semver break.

## 8. Acceptance criteria

1. All §5.1 contract tests pass and match reference `mk` output.
2. §5.2 corpus files expand to documented expectations.
3. `cargo test` green, clippy clean (`--all-targets --all-features`).
4. invest-research dictionary target rebuilds when a `.toon` is added
   (incrementality restored).
5. TRACEABILITY F-045/002 honest; BUGS.md entry added.
6. No regression in the existing 309 tests (beyond intentionally-updated
   `$prereq`-literal assertions, each commented).

## 9. Open questions for architect review (before handing to rust-coder)

- Q-A (RESOLVED, see S10): CLI `VAR=value` (F-042) is implemented in this same
  pass. It is sticky-override: CLI vars parsed first, mkfile cannot reassign
  them. The existing `Scope::set` precedence gate likely already enforces this
  (CommandLine > Mkfile); verify with a test.
- Q-B (RESOLVED): attribute-word is NOT expanded at parse time; `P` program
  is literal in AST and expanded at call-time. No work needed in the parser
  for attributes — leave `parse_attributes` untouched.
- Q-C (RESOLVED, see S11): quoting and word-splitting of expanded values.
  The reference uses a `Word*` linked list with quote-aware tokenization
  (quotes stripped at definition time, word boundaries preserved). A naive
  `split_whitespace` is INSUFFICIENT — see §4.2 and §7. mk-rust additionally
  has a pre-existing quoting divergence (it KEEPS quote chars in values;
  reference strips them) that F-045 exposes but does not need to fully fix.
  Recommended scope for F-045: handle the common case (whole-word `$VAR` →
  split on whitespace, S11a) correctly; document the literal-glue edge cases
  (S11b) and the quoting divergence (S11c) as known gaps, do NOT try to
  reproduce the reference's buffer-glue logic in this pass.
- Q-D (RESOLVED, see S12): mutual recursion `A=$B; B=$A` is **NOT an error and
  not a hang** in the reference — cycles terminate by treating the revisited
  variable as empty (D-1 → `[]`). More importantly, the current `Scope::expand`
  depth-10 cap is **wrong** for F-045: legitimate deep chains (1000 levels,
  D-8) resolve in the reference but would be rejected by mk-rust. Required
  change (risk R-REC): replace the depth cap with a visited-set cycle
  detector; deep chains then work, cycles yield empty/partial matching the
  reference. This is a pre-condition for S2/S12 — must land in phase 1
  (`var.rs`) before the parse pass uses `expand`.
- Q-E (RESOLVED, see S13): a variable may expand to structurally-significant
  text in a target/prereq position. `$PAT` (PAT=`%.o`) → metarule (E-2);
  `$NAMES` (NAMES=`alpha beta`) → multiple rules, one per word (E-3). The
  parser's metarule detection (`is_metarule` = target contains `%`/`&`) must
  run on the EXPANDED target text, not the raw token. No attribute-word
  expansion (that stays as Q-B/S9).

## 10. Reference verification log (plan9port `/usr/local/plan9/bin/mk`)

Every behavior in §2 was confirmed empirically. Key experiments:

| Experiment | Setup | Result |
|---|---|---|
| Read-time (S1) | `TARG=early; t:$TARG; TARG=late` | prereq=`early` |
| Assign RHS (S2) | `A=world; B=hello $A` | B=`hello world` |
| Var is list (S4) | `SRCS=a.c b.c; t:$SRCS` | 2 prereqs |
| `$$` in header (S6) | `target: $$SRCS` | → `$SRCS` → lookup |
| Recipe var in header (S7) | `target: $prereq` | empties → broken target |
| Include path (S8) | `< $INCL`, `` < `{echo sub.mk} `` | both expand |
| Pipe-include (S8) | `<| $CMD` | expands |
| Env literal `$` (QU-1) | `DOLLAR_VAR='price$5'` | kept literal, no re-expand |
| Glob in prereq (QU-2a) | literal `globsub/*.c` | **reference does NOT glob** |
| Var to filenames (QU-2c) | `FILES=a.c b.c; t:$FILES` | 2 prereqs (no glob chars) |
| Attr-word `$` (QB-3) | `$ATTRS=VQ; t:$ATTRS:` | `unknown attribute '$'` |
| `P$PROG` call-time (QB-9) | CMPROG changed after rule | final value invoked |
| Deep chain (D-8) | `V0=leaf; ...; V1000=$V999` | `V1000=leaf` (no depth limit) |
| Direct cycle (D-1) | `A=$B; B=$A` | both `[]` (empty, no error) |
| 3-cycle partial (D-10) | `A=a$B; B=b$C; C=c$A` | A=`a`, B=`b`, C=`ca` |
| Assign-time order (D-12) | `GREETING=$FIRST world; FIRST=hello` | GREETING=` world` (FIRST was empty) |
| Stored-after-expand (D-14/15) | `A=aa; B=$A; C=$B` | B stored `aa`, C=`aa` |
| Var→metarule (E-2) | `PAT=%.o; $PAT: %.c` | `%` survives, metarule matched |
| Var→multi-target (E-3) | `NAMES=alpha beta; $NAMES:V:` | two rules (alpha, beta) |
| CLI override, rule before assign (F42-1) | `mk VAR=cli`; `t:$VAR` then `VAR=mk` in mkfile | prereq=`cli` (mkfile assign ignored) |
| CLI override, rule after assign (F42-2) | `mk VAR=cli`; `VAR=mk` then `t:$VAR` | prereq=`cli` (CLI sticky) |
| S1 baseline no CLI (F42-3) | `VAR=first; t:$VAR; VAR=second` | prereq=`first` (read-time) |
| CLI propagates to RHS (F42-4) | `mk BASE=/opt`; `DERIVED=$BASE/extra` | `DERIVED=/opt/extra` |
| Multiple CLI vars (F42-5) | `mk A=1 B=2` | both set |
| CLI var in include (F42-6) | `mk INCPATH=sub/inc.mk` | include resolves |
| Mkfile cannot force-override CLI (F42-7) | `mk VAR=cli`; mkfile `VAR=forced` | VAR=`cli` |
| Quoted space in assign (QC-1) | `SRCS="a b".c; t:$SRCS` (MKSHELL=sh) | ONE prereq `a b.c` (quote stripped) |
| rc single-quote (QC-2) | `SRCS='a b'.c; t:$SRCS` | ONE prereq `a b.c` |
| mk-rust keeps quotes (QC-4) | `SRCS="a b".c` recipe `echo [$SRCS]` | mk-rust=`["a b".c]` ✗ vs ref=`[a b.c]` ✓ |
| Backslash-space (QC-7) | `SRCS=a\ b.c; t:$SRCS` | ONE prereq `a b.c` |
| Multi-word var as target (QC-8) | `NAMES=x y; $NAMES:V:` | TWO rules (`x` and `y`) |
| Literal-prefix glue (QC-5) | `PARTS=one two; t: pre.$PARTS` | prereqs `pre.one`, `two` |
| Literal-suffix glue (QC-6) | `NAME=foo bar; t: $NAME.x` | prereqs `foo`, `bar.x` |

## 11. Implementation plan (for rust-coder handoff)

Phased, each phase leaves the build green:

1. **`var.rs`: `set` expands fully; add `set_raw`; replace depth-cap with
   cycle detection (R-REC, blocks S12).** Change `Scope::set` to run backtick
   → `Scope::expand` (recursive). Add `set_raw(name, value, prec)` that stores
   literally. Switch **`import_env` and `builtin_scope`** to `set_raw` (env
   values with `$` must not re-expand — QU-1; builtins are literal anyway).
   **`set_force` needs NO change** — recipe-time vars bypass `Scope` entirely
   (injected into a plain `HashMap` in `recipe.rs:172-182`), so they are immune
   to assign-time expansion by construction; `set_force` is only used in tests.
   **Replace `Scope::expand`'s depth-10 fixed-point cap** with a
   **visited-set cycle detector**: track variable names currently
   mid-expansion (a `HashSet<String>` threaded through the recursion); on
   revisit substitute the empty string and continue. This makes deep chains
   (D-8: 1000 levels) resolve and cycles (D-1/D-10) yield empty/partial
   matching the reference — never an error, never a hang. Unit tests: recursive
   RHS (D-14/15), 1000-deep chain (D-8), direct cycle → empty (D-1), 3-cycle
   partial (D-10), env-literal-`$` kept (QU-1), backtick still runs (S3).

2. **`parse.rs`: `parse_with_scope`.** Add the public fn; refactor
   `parse_with_includes` to take `&mut Scope`. For `Assign`, call
   `scope.set(name, value, Mkfile)` and store the expanded value in `Assign`.
   Keep `parse()` as a thin wrapper that builds builtin+env scope. Tests: §5.1
   S1/S2/S4/S6/S7.

3. **`parse.rs`: expand targets & prereqs.** In `parse_rule`, expand each raw
   target/prereq word with `scope.expand`, then split into possibly-many words
   per S11. Start with `split_whitespace` (correct for the common whole-word
   `$VAR` case, S11a). Do NOT touch the attribute token. Run the
   `$(...)`-rejection on the RAW tokens. **Metarule detection (`is_metarule` =
   target contains `%`/`&`) must run on the EXPANDED target text (S13/E-2),**
   since a variable may expand to a `%`-pattern. Tests: §5.1 S5/S9/S13, corpus
   2.1/3.1/4.1, plus S11a (whole-word multi-file var → many prereqs/targets),
   S11b (literal-glue, annotated with mk-rust's actual result if it diverges),
   S11c (quoted-space, annotate the pre-existing divergence).

4. **`include.rs`: expand path/command.** Thread scope into `IncludeContext`
   methods; `scope.expand` the include path/command before resolving. Tests: S8.

5. **`main.rs`: reorder + F-042 (S10).** Build scope (builtin+env) before parse.
   **CLI assignments are sticky-override (S10), not simple precedence.** Mirror
   plan9port `main.c`: parse CLI `VAR=value` args FIRST, applying each to the
   scope at `Precedence::CommandLine`. Then during mkfile parse, a mkfile
   `Assign` to a CLI-overridden var must be **silently ignored** (not re-set).
   Mechanic: the existing `Scope::set` precedence gate
   (`if prec < stored_prec { return false }`) already rejects a `Mkfile`-prec
   `set` when a `CommandLine`-prec value is stored (CommandLine > Mkfile), so
   S10 may already hold once CLI vars are parsed into scope before the mkfile.
   **Verify this with a test** (F42-1/F42-2/F42-7). Remove the post-parse
   assign loop (it would re-run backtick). Parse the mkfile with
   `parse_with_scope`. End-to-end check on the invest-research dictionary target.

6. **Docs.** TRACEABILITY F-045/F-002 honest; F-039/F-042 status updated;
   BUGS.md resolved entry; README note for `parse_with_scope`.

Run `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`
after each phase.
