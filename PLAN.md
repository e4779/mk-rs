# mk-rust: Plan

> *"The Unix philosophy: Write programs that do one thing and do it well."* — Doug McIlroy
>
> *"Mk is an efficient general tool for describing and maintaining dependencies between files or programs."* — Andrew Hume

---

## 1. Project vision

mk-rust is a faithful, high-quality Rust port of Plan 9's `mk` build tool — not a clone of GNU Make, not a reimagining with Lua/JS, not a general task runner. It exists because the Go ports (dcjones, ctSkennerton) are abandoned, the Knit project conflates build logic with a full scripting VM, and the original plan9port C codebase (~4,350 LOC of Plan 9 dialect C) has outlived its portability window.

What mk-rust **is**:

- A dependency-driven build tool that reads mkfiles and runs recipes in parallel
- A direct port of Plan 9 mk semantics: pattern-based metarules, transitive closure, attribute system, `$stem`/`$target`/`$prereq` variables
- A library-first crate (`mk-core`) with a thin CLI wrapper (`mk-cli`)
- Fast, safe, portable — leverages Rust's ownership model where C used raw pointers
- 100% compatible with existing mkfiles intended for plan9port mk (sh recipes by default, rc via `$MKSHELL`)

What mk-rust is **not**:

- A build system for Cargo/Rust projects (use `cargo` for that)
- A general-purpose task runner (use `just`, `cargo-make`, or shell scripts)
- A Lua/JS/Python-based build system (duckscript may power *recipes* for power users, but the core tool is pure Rust)
- GNU Make compatible — no `.PHONY`, no pattern substitution `$(patsubst ...)`, no `--eval`
- A package manager, a daemon, or a file watcher (though a daemon mode is listed under §6 future considerations)

The cat-v.org philosophy applies: mk is a tool for maintaining files. It should be small, composable, and free of accidental complexity. The mkfile is machine-readable documentation of your pipeline. mk-rust honors that.

---

## 2. Architecture overview

### 2.1 Crate structure

```
mk-rust/                     # workspace root
├── Cargo.toml               # workspace: [workspace] members = [...]
├── PLAN.md                  # this file
├── crates/
│   ├── mk-core/             # library: lex + parse + graph + var + sched + shell + attr + archive + include
│   ├── mk-shell/            # Shell trait + sh/rc/duckscript implementations
│   └── mk-cli/              # binary: clap CLI, thin wrapper around mk-core
```

| Crate | Purpose | Dependencies |
|-------|---------|-------------|
| `mk-core` | All build logic. Exposes `build(mkfile_path, opts) -> Result<BuildOutcome>`. No I/O in public API surface — takes a `shell: &dyn Shell` and file system via a `FileSystem` trait (testable). | `regex`, `glob`, `serde` (optional, for AST debugging), `thiserror`, `log` |
| `mk-shell` | `Shell` trait definition (in mk-core), plus `sh::Shell`, `rc::Shell`, `duckscript::Shell` implementations. | `duct` (for sh), `duckscript` + `duckscriptsdk` (optional feature) |
| `mk-cli` | CLI entry point. Argument parsing, loading mkfile, calling `mk-core::build()`, formatting output. | `clap` (derive), `mk-core`, `mk-shell`, `env_logger` |

### 2.2 Key dependencies

| Crate | Used in | Purpose |
|-------|---------|---------|
| `regex` | mk-core (parse, graph) | Compiled regex for `R:` metarules, regex-based stem extraction |
| `clap` (derive) | mk-cli | CLI argument parsing (`-f`, `-n`, `-e`, `-t`, `-a`, `-p`, `-k`, etc.) |
| `thiserror` | mk-core | Structured error types (`MkError`) |
| `duct` | mk-shell (sh, rc) | Process execution with environment passing, stderr capture |
| `glob` | mk-core (graph) | Path globbing for targets/prereqs in rules |
| `serde` + `serde_json` | mk-core (optional) | AST serialization (debugging, future LSP, mkfile formatter) |
| `log` + `env_logger` | mk-cli | Verbose/debug logging |
| `crossbeam` | mk-core (sched) | Parallel job scheduling (channel-based worker pool) |
| `tempfile` | mk-core (recipe) | Temp files for inline recipe scripts |
| `filetime` | mk-core (graph) | File modification time comparison (out-of-date checks) |

### 2.3 Data flow

```
                      ┌──────────────┐
                      │   mkfile(s)   │  (user-authored text)
                      └──────┬───────┘
                             │
                    ┌────────▼────────┐
                    │    lex::Lexer   │  char-by-char → token stream
                    │  (tokenizer)    │  handles: words, colons, =, <, |, newlines, indents, #comments, backticks
                    └────────┬────────┘
                             │  TokenStream (Iterator<Item = Token>)
                    ┌────────▼────────┐
                    │  parse::Parser  │  recursive descent → AST
                    │                 │  Rules, Assignments, Includes, MetaRules
                    └────────┬────────┘
                             │  AST (Vec<Stmt>)
                    ┌────────▼────────┐
                    │   var::Scope    │  expand variables, resolve symbol table
                    │                 │  $VAR, ${VAR}, ${VAR:pat=sub}, namelists
                    └────────┬────────┘
                             │  expanded AST
                    ┌────────▼────────┐
                    │  graph::Builder │  AST → DAG
                    │                 │  apply meta-rules, transitive closure, pruning
                    └────────┬────────┘
                             │  Graph (nodes + arcs)
                    ┌────────▼────────┐
                    │ graph::Checker  │  out-of-date check (mtime comparison)
                    │  (staleness)    │  mark stale nodes, handle virtual targets
                    └────────┬────────┘
                             │  BuildPlan (topologically sorted stale nodes)
                    ┌────────▼────────┐
                    │  sched::Engine  │  parallel DAG traversal
                    │                 │  NPROC worker pool, job queue
                    └────────┬────────┘
                             │  dequeued Job
                    ┌────────▼────────┐
                    │ recipe::Runner  │  feed recipe to shell
                    │                 │  set $target, $prereq, $stem, env vars
                    └────────┬────────┘
                             │  exit code
                    ┌────────▼────────┐
                    │   BuildOutcome  │  success, partial (with -k), or failure
                    └─────────────────┘
```

Pipeline stages are distinct and sequential within a single `build()` call. Each stage produces an owned output consumed by the next stage. No shared mutable state across stages.

---

## 3. Module design (for `mk-core`)

### 3.1 `lex` — Lexer/tokenizer

**Purpose:** Convert raw mkfile text into a token stream. Handles Plan 9 mk's idiosyncratic line-assembly rules: escaped newlines (`\` at EOL → space for sh, elision for rc), comment lines (`#`), backtick shell substitutions, and quote-aware scanning.

**Key types:**

```rust
/// A single token from the mkfile.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),           // any whitespace-delimited text
    Colon,                  // :
    Equals,                 // =
    Include,                // <
    Pipe,                   // |
    Indent,                 // leading whitespace (recipe continuation)
    Newline,                // blank line or end of rule
    Eof,
}

/// Lexer converts a mkfile string into tokens.
pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    // ... shell mode (sh/rc) affects escaped-newline handling
}
```

**Public API:**

```rust
impl Lexer {
    pub fn new(input: &str, shell_mode: ShellMode) -> Self;
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError>;
}

// Convenience
pub fn tokenize(input: &str, shell_mode: ShellMode) -> Result<Vec<Token>, LexError>;
```

**Internal design notes:**

- State machine over characters: scanning words, handling quoted strings (`'...'`, `"..."`), backtick pairs, comments, escaped newlines
- `ShellMode` enum (`ShellMode::Sh`, `ShellMode::Rc`) controls escaped-newline behavior
- Backtick contents are preserved as literal text in the token stream (expansion happens later in `var` or during recipe execution)
- Indentation depth is collapsed to a single `Indent` token regardless of amount
- No regex in lexer — pure character-level scanning for correctness and speed

---

### 3.2 `parse` — Parser

**Purpose:** Convert the token stream into a structured AST (`Vec<Stmt>`). Recursive descent parser following Plan 9 mk's grammar: rules (`target: prereqs`), assignments (`VAR=value`), includes (`< file`, `<| command`), metarules, and recipes.

**Key types:**

```rust
/// Top-level mkfile statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Rule(Rule),
    Assign(Assign),
    Include(Include),
}

/// A single rule (concrete or meta).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub targets: Vec<String>,       // left of :
    pub prereqs: Vec<String>,       // right of : (or empty)
    pub attributes: Attributes,     // V, Q, N, U, D, E, P, R flags
    pub recipe: Option<String>,     // indented block (None for rules without recipes)
    pub is_metarule: bool,          // true if targets contain % or &
    pub is_regex: bool,             // true if R attribute present
    pub line: usize,                // source line number
}

/// Variable assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub name: String,
    pub value: String,
    pub attributes: Attributes,     // currently only U (unexported)
}

/// Include directive.
#[derive(Debug, Clone, PartialEq)]
pub enum Include {
    File(String),                   // < filename
    Command(String),                // <| command
}

/// Rule attributes (bitflags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attributes(u16);
// Individual attributes are tested via methods:
// .is_virtual(), .is_quiet(), .is_no_exec(), etc.
```

**Public API:**

```rust
pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError>;

// Lower-level for testing
pub struct Parser { ... }
impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self;
    pub fn parse_all(&mut self) -> Result<Vec<Stmt>, ParseError>;
}
```

**Internal design notes:**

- Recursive descent: `parse_stmt()` dispatches on first token → `parse_rule()`, `parse_assign()`, `parse_include()`
- Rule parsing (`parse_rule()`): reads target list, attributes (between colons), prereq list, then `parse_recipe()` for indented lines
- Attributes parsed between two colons: `target:VQ: prereq` → `V` and `Q` attributes on the rule
- Metarules detected by `%` or `&` in targets or prereqs; `R:` attribute marks regex metarules
- Recipe parsing: collect all `Indent` + content lines into a single recipe string
- Blank lines (double newline) terminate a recipe block
- Error recovery: report first error with line/column, don't try to continue parsing

---

### 3.3 `graph` — Dependency graph (DAG)

**Purpose:** Convert parsed rules into a directed acyclic graph of targets (nodes) and dependencies (arcs). Apply metarules to match unknown targets, perform transitive closure, prune vacuous/ambiguous edges, and check which targets are out of date.

**Key types:**

```rust
/// A target in the dependency graph.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub mtime: Option<SystemTime>,     // None = virtual or not yet checked
    pub flags: NodeFlags,              // VIRTUAL, CYCLE, READY, MADE, etc.
    pub arcs_in: Vec<ArcIndex>,        // edges where this node is a prereq
    pub arcs_out: Vec<ArcIndex>,       // edges where this node is a target
}

/// A dependency edge.
#[derive(Debug, Clone)]
pub struct Arc {
    pub from: NodeIndex,               // prerequisite
    pub to: NodeIndex,                  // target
    pub rule: Option<RuleRef>,          // rule that produced this edge
    pub stem: Option<String>,           // stem from pattern match ($stem1, $stem2, ...)
    pub is_meta: bool,                  // from metarule application
    pub match_groups: Vec<String>,      // regex submatches (for R: rules)
}

/// The full dependency graph.
#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub arcs: Vec<Arc>,
    pub default_targets: Vec<NodeIndex>,
}

// Index types for arena-style storage
pub type NodeIndex = usize;
pub type ArcIndex = usize;
pub type RuleRef = usize;  // index into rule table
```

**Public API:**

```rust
pub struct Builder {
    rules: Vec<Rule>,
    meta_rules: Vec<Rule>,
}

impl Builder {
    pub fn new(rules: Vec<Rule>, meta_rules: Vec<Rule>) -> Self;

    /// Build the full DAG for given targets. Performs:
    /// 1. Concrete rule application (direct target → prereq edges)
    /// 2. Metarule application (pattern matching unknown targets)
    /// 3. Transitive closure (recurse into prereqs)
    /// 4. Vacuous edge pruning
    /// 5. Ambiguous edge pruning
    pub fn build(&mut self, targets: &[String]) -> Result<Graph, GraphError>;

    /// Add a node (or return existing index). Idempotent.
    fn node(&mut self, name: &str) -> NodeIndex;

    /// Add an arc between nodes.
    fn arc(&mut self, from: NodeIndex, to: NodeIndex, rule: RuleRef);
}

/// Check which nodes need rebuilding.
pub fn stale_nodes(graph: &Graph) -> Vec<NodeIndex>;
```

**Internal design notes:**

- Arena-style storage: nodes and arcs stored in `Vec`, referenced by index (`NodeIndex`, `ArcIndex`). This avoids `Rc<RefCell<...>>` cycles and is cache-friendly
- `Builder` is the heart of DAG construction. It mutates its own `Graph` during building
- Metarule matching: for each unknown target, try `%` match against metarules (single `%`), then `&` (amperstand — matches exactly one target), then `R:` regex metarules
- Transitive closure: recursively resolve prereqs of prereqs. Stop at: files that exist on disk, virtual targets, or targets already in the graph (cycle detection)
- Pruning: after full graph is built, `vacuous()` removes metarule edges where the metarule's recipe was never actually needed; `ambiguous()` removes metarule edges that are redundant with concrete rules
- Cycle detection: DFS during transitive closure marks nodes as visiting/visited. A back edge → cycle error (unless `-i` flag forces)
- Staleness: a target is stale if its mtime < any prereq's mtime, or if it doesn't exist on disk, or if a prereq is itself stale

---

### 3.4 `var` — Variable system

**Purpose:** Manage variable assignments, symbol table lookup, and string expansion. Handle Plan 9 mk's variable syntax: `$VAR`, `${VAR}`, `${VAR:pattern=substitution}`, `%` stem references, and namelist expansion.

**Key types:**

```rust
/// Variable scope / symbol table.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    vars: HashMap<String, String>,
    parent: Option<Box<Scope>>,       // for nested includes — chain lookup
}

/// Namelist: a space-separated list of words, used for $stem, $prereq, etc.
#[derive(Debug, Clone)]
pub struct WordList(Vec<String>);
```

**Public API:**

```rust
impl Scope {
    pub fn new() -> Self;
    pub fn with_parent(parent: Scope) -> Self;
    pub fn set(&mut self, name: &str, value: &str);
    pub fn get(&self, name: &str) -> Option<&str>;
    pub fn has(&self, name: &str) -> bool;

    /// Expand variables in a string. Handles:
    ///   $VAR, ${VAR}, ${VAR:pat=sub}, ${VAR:pat=%}
    ///   $$ → literal $
    pub fn expand(&self, input: &str) -> Result<String, VarError>;
    pub fn expand_words(&self, input: &str) -> Result<Vec<String>, VarError>;
}

/// Build a scope from AST assignments + built-in defaults.
pub fn build_scope(stmts: &[Stmt]) -> Result<Scope, VarError>;
```

**Internal design notes:**

- Chain lookup: `get()` walks `self` → `parent` chain. This handles nested mkfile includes where inner vars shadow outer vars
- Expansion is recursive: `${FOO}` where `FOO=${BAR}` resolves through the chain
- Substitution syntax: `${VAR:A%B=C%D}` → replace `A.*B` in VAR's value with `C.*D`. The `%` in the pattern matches like a glob. Equivalent to Plan 9 mk's `shsub()` function
- Default variables set at scope creation time: from environment variables (with `OS.` prefix stripped), and built-ins based on the target request
- Recipe-time variables (`$target`, `$prereq`, `$stem`, `$newprereq`, `$alltarget`) are populated by the scheduler just before recipe execution, not during parse-time expansion
- Namelist expansion: `$stem` is `WordList` — `$stem(1)`, `$stem(2)`, `$stem(N)` for individual words. `$(stem)` or `$stem` returns all words space-separated

---

### 3.5 `shell` — Shell abstraction

**Purpose:** Define a `Shell` trait that abstracts recipe execution. Part of `mk-core` (the trait), with implementations in `mk-shell`.

**Key types (in mk-core):**

```rust
/// Shell abstraction for recipe execution.
pub trait Shell: Send + Sync {
    /// Return the shell name (e.g., "sh", "rc", "duckscript").
    fn name(&self) -> &str;

    /// Execute a recipe script. Returns exit code.
    /// `env` is the environment variables to pass.
    /// `dir` is the working directory.
    fn execute(&self, recipe: &str, env: &HashMap<String, String>, dir: &Path) -> Result<i32, ShellError>;

    /// Find unescaped instances of a character in a string.
    /// Used by parser for assignment attribute detection.
    fn find_unescaped(&self, input: &str, ch: char) -> Vec<usize>;

    /// Escape a token for this shell's quoting rules.
    fn quote(&self, token: &str) -> String;
}

/// How to select a shell for a rule.
#[derive(Debug, Clone)]
pub enum ShellSelector {
    Default,                    // use $MKSHELL or default
    Named(String),              // "rc", "sh", "python"
    Custom(String),             // raw command: "/bin/zsh -e"
}
```

**Public API in mk-shell:**

```rust
// sh implementation
pub struct ShShell;
impl Shell for ShShell { ... }

// rc implementation (mirrors plan9port rc.c)
pub struct RcShell;
impl Shell for RcShell { ... }

// Registry of built-in shells
pub struct ShellRegistry {
    shells: HashMap<String, Box<dyn Shell>>,
}
impl ShellRegistry {
    pub fn with_defaults() -> Self;          // registers sh, rc
    pub fn get(&self, name: &str) -> Option<&dyn Shell>;
    pub fn register(&mut self, name: &str, shell: Box<dyn Shell>);
}
```

**Internal design notes:**

- `ShShell` wraps `duct::cmd!("sh", "-c", recipe)` with proper env passing
- `RcShell` wraps `duct::cmd!("rc", "-c", recipe)` with rc-specific env format (rc uses `\x01` as array separator in environment variables — see `rc.c` in plan9port)
- Quote escaping: sh uses `'...'` (single quotes, escape `'` as `'\''`); rc uses `'...'` (single quotes, double the quote to escape: `''` → literal `'`)
- `find_unescaped` is used by the attribute parser — detects whether `=` is inside a shell quote or not. This is shell-specific because sh and rc have different quoting rules
- Future: `duckscript::Shell` — embeds the duckscript runtime directly, no subprocess needed. Higher performance for complex recipes, but adds a dependency

---

### 3.6 `recipe` — Recipe execution

**Purpose:** Glue between scheduler and shell. Prepares the execution environment, writes recipe to temp file if needed, invokes shell, and handles return codes / error reporting.

**Key types:**

```rust
/// A recipe ready to execute.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub target: String,
    pub prereqs: Vec<String>,
    pub stem: Option<String>,
    pub all_targets: Vec<String>,
    pub new_prereqs: Vec<String>,
    pub script: String,                     // the actual command text
    pub shell: ShellSelector,
    pub attributes: Attributes,
    pub working_dir: PathBuf,
    pub env: HashMap<String, String>,       // pre-expanded recipe-time vars
}

/// Outcome of a recipe execution.
#[derive(Debug, Clone)]
pub struct RecipeResult {
    pub target: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
}
```

**Public API:**

```rust
/// Execute a single recipe through the configured shell.
pub fn run(
    recipe: &Recipe,
    shell: &dyn Shell,
    opts: &RecipeOptions,
) -> Result<RecipeResult, RecipeError>;

/// Options for recipe execution.
pub struct RecipeOptions {
    pub no_exec: bool,       // -n flag: print, don't execute
    pub keep_going: bool,    // -k flag: continue on error
    pub silent: bool,        // -s flag: don't print recipes
    pub touch: bool,         // -t flag: touch targets instead of running
    pub explain: bool,       // -e flag: explain why recipes run
}
```

**Internal design notes:**

- Recipe variables (`$target`, `$prereq`, `$stem`, `$alltarget`, `$newprereq`) are expanded into `Recipe.env` before execution, so the recipe script can use them as regular env vars
- Error handling: non-zero exit → `RecipeError::CommandFailed { exit_code }`. With `-k`, the error is recorded but execution continues; without `-k`, it terminates the build
- `-n` (no-exec): print the recipe with variables expanded, return success without running
- `-t` (touch): update mtime on target file instead of running recipe
- `-e` (explain): print "target is out of date because prereqX is newer" before running
- Recipe script is passed to the shell as a string; for long recipes, it may be written to a temp file
- `Q` (quiet) attribute: suppress recipe echoing (equivalent to `@` prefix in Make)
- `N` (no-exec) attribute: never execute this rule's recipe (just print it)

---

### 3.7 `sched` — Scheduler

**Purpose:** Orchestrate parallel execution of build jobs. Walk the DAG, enqueue jobs whose prereqs are all satisfied, dispatch to worker pool, reap results.

**Key types:**

```rust
/// Build scheduler / execution engine.
pub struct Engine {
    graph: Graph,
    nproc: usize,                   // from $NPROC (default 1)
    shell_registry: ShellRegistry,
    options: BuildOptions,
}

/// One unit of work.
#[derive(Debug)]
pub struct Job {
    pub node: NodeIndex,
    pub rule_index: RuleRef,
    pub recipe: Recipe,
}

/// Final result of a build.
#[derive(Debug)]
pub struct BuildOutcome {
    pub targets_built: Vec<String>,
    pub targets_unchanged: Vec<String>,
    pub targets_failed: Vec<(String, RecipeError)>,
    pub total_duration: Duration,
}
```

**Public API:**

```rust
impl Engine {
    pub fn new(graph: Graph, nproc: usize, shells: ShellRegistry, options: BuildOptions) -> Self;

    /// Execute the build plan. Returns when all jobs complete or on first failure (if !keep_going).
    pub fn run(self, stale_nodes: &[NodeIndex]) -> Result<BuildOutcome, SchedError>;
}
```

**Internal design notes:**

- Worker pool pattern:
  - `crossbeam::unbounded` channel → job queue
  - Spawn `NPROC` threads, each pulling from the queue
  - Results sent back via a separate channel
  - Semaphore-style backpressure: if no free slot, `work()` blocks until a job finishes
- Job selection (`work()`):
  - Topological walk of stale nodes
  - A node is *ready* when all its prereqs are `MADE` (successfully built) or already up-to-date
  - Enqueue ready nodes, recurse
- Exclusive subprocess support: `E` attribute (exclusive) — when an exclusive job is scheduled, all other workers finish their current jobs and block until the exclusive job completes. This mirrors plan9port's `reserveExclusiveSubproc()`. Useful for recipes that consume all CPU/RAM or touch shared state
- `-k` (keep going): don't abort on error. Mark the node as failed, continue scheduling jobs whose prereqs are all satisfied (even if some failed)
- Signal handling: SIGINT/SIGTERM → cancel all in-flight jobs, clean up temp files, exit
- Progress reporting: via `log` crate or callback — print `target:` before each recipe runs (unless `Q` attribute or `-s` flag)
- `NPROC` variable can change mid-build (it's checked dynamically from the variable scope)

---

### 3.8 `attr` — Attribute system

**Purpose:** Parse, store, and query rule attributes. These are the single-letter flags between colons in rule headers: `target:VQ: prereq`.

**Key types:**

```rust
/// Bitflags for rule attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attributes(u16);

impl Attributes {
    // Individual attribute checks
    pub const VIRTUAL: u16    = 1 << 0;   // V: target is virtual (not a file)
    pub const QUIET: u16      = 1 << 1;   // Q: don't echo recipe
    pub const NO_EXEC: u16    = 1 << 2;   // N: print but don't execute
    pub const UNEXPORTED: u16 = 1 << 3;   // U: don't export to environment
    pub const DELETE: u16     = 1 << 4;   // D: delete target on error
    pub const EXCLUSIVE: u16  = 1 << 5;   // E: run exclusively (no parallelism)
    pub const COMPARISON: u16 = 1 << 6;   // P: custom comparison program
    pub const REGEX: u16      = 1 << 7;   // R: regex metarule
    pub const ERROR: u16      = 1 << 8;   // n: ignore errors from this recipe

    pub fn is_virtual(&self) -> bool;
    pub fn is_quiet(&self) -> bool;
    pub fn is_no_exec(&self) -> bool;
    pub fn is_unexported(&self) -> bool;
    pub fn is_delete_on_error(&self) -> bool;
    pub fn is_exclusive(&self) -> bool;
    pub fn has_comparison(&self) -> bool;
    pub fn is_regex(&self) -> bool;
    pub fn ignore_errors(&self) -> bool;

    // Parse from attribute string between colons: "VQ" → VIRTUAL | QUIET
    pub fn parse(s: &str) -> Result<Self, ParseAttrError>;

    // Apply attribute string to existing set (for assignments with attributes)
    pub fn apply(&mut self, s: &str) -> Result<(), ParseAttrError>;
}

/// Human-readable descriptions for each attribute, used by CLI help / `-e` flag.
pub const ATTR_HELP: &[(&str, &str)] = &[
    ("V", "Virtual target — not a real file"),
    ("Q", "Quiet — don't echo recipe"),
    ("N", "No-exec — print recipe, don't run"),
    ("U", "Unexported — don't put in environment"),
    ("D", "Delete target on error"),
    ("E", "Exclusive — run without parallel jobs"),
    ("P", "Custom comparison program"),
    ("R", "Regex metarule"),
    ("n", "Ignore errors"),
];
```

**Internal design notes:**

- Pure bitflags. Simple, fast. No need for a struct with booleans
- `parse("VQ")` looks up each char in a match table → returns combined flags
- Invalid attribute chars → `ParseAttrError::UnknownAttr(char)`
- `apply(s)` is for assignments with attributes: `VAR:U=value` → `U` attribute on the assignment
- Assignment attributes currently only support `U` (unexported), matching plan9port behavior
- All attribute checks are inline methods (the bitflags macro or hand-written), zero-cost

---

### 3.9 `archive` — Archive member support (Phase 2)

**Purpose:** Handle the `lib(member)` syntax where a target is a member of an archive file. Plan 9 mk supports `lib(member.o)` for object archives, automatically extracting the member recipe.

**Key types:**

```rust
/// An archive member reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveRef {
    pub archive: String,       // e.g., "libfoo.a"
    pub member: String,        // e.g., "bar.o"
}

/// Parse "lib(member)" syntax.
pub fn parse_archive_ref(name: &str) -> Option<ArchiveRef>;

/// Generate archive member rule.
/// Input:  lib.a(member.o)
/// Output: lib.a(member.o): member.o
///             ar rv lib.a member.o
pub fn archive_rule(archive_ref: &ArchiveRef) -> Rule;
```

**Internal design notes:**

- Syntax: `lib(member)` — parentheses nestable? Plan 9 mk does not nest them. Simpler: match `^(.*)\(([^)]+)\)$` — archive name + member name
- If a target `libfoo.a(bar.o)` has no explicit rule, mk automatically generates: `libfoo.a(bar.o): bar.o` with recipe `ar rv libfoo.a bar.o` (or equivalent via `$AR` variable)
- The archive file's mtime is used for staleness comparison; member extraction doesn't have its own mtime
- This is a Phase 2 feature — the MVP doesn't need it, but the architecture should leave room for it (the graph builder needs a hook for "auto-rule generation")

---

### 3.10 `include` — Include system

**Purpose:** Handle `< file` and `<| command` include directives, recursive parsing, and circular include detection.

**Key types:**

```rust
/// Include stack frame.
#[derive(Debug)]
struct IncludeFrame {
    path: PathBuf,         // absolute path to the mkfile
    line: usize,            // currently at what line in this file
}

/// Include context manages recursive includes.
#[derive(Debug)]
pub struct IncludeContext {
    stack: Vec<IncludeFrame>,       // for error reporting
    visited: HashSet<PathBuf>,      // for circular include detection
    working_dir: PathBuf,
}
```

**Public API:**

```rust
impl IncludeContext {
    pub fn new(working_dir: PathBuf) -> Self;

    /// Resolve, open, and parse an included mkfile.
    /// Returns the parsed statements, or error on circular include.
    pub fn include_file(&mut self, path: &str) -> Result<Vec<Stmt>, IncludeError>;

    /// Run a command and parse its stdout as mkfile input.
    pub fn include_command(&mut self, command: &str, shell: &dyn Shell)
        -> Result<Vec<Stmt>, IncludeError>;
}
```

**Internal design notes:**

- `< file` → resolve relative to current working directory (`-C` flag) or relative to the including mkfile's directory (plan9port behavior: relative to including file's dir)
- `<| command` → execute command via configured shell, capture stdout, parse as mkfile text. Useful for dynamic dependency generation (e.g., `gcc -M *.c`)
- Circular include detection: before opening a file, check `visited`. If present → `IncludeError::CircularInclude { chain: Vec<PathBuf> }`
- The include stack is used for error messages: "in included file `subdir/mkfile:12`"
- Included mkfiles get their own variable scope (child of parent scope), so their assignments don't leak upward

---

## 4. TDD strategy

### 4.1 Test pyramid

```
            ┌─────────────┐
            │  Property   │  ← fuzz parsing, random mkfile generation, graph invariants
            │   tests     │      (small set, high value)
            └──────┬──────┘
       ┌───────────┴───────────┐
       │   Integration tests   │  ← end-to-end mkfile builds: C compilation, data pipelines
       │   (testdata/*.mk)     │      (medium set, documentation value)
       └───────────┬───────────┘
   ┌───────────────┴───────────────┐
   │        Unit tests              │  ← per-module: lex, parse, graph, var, attr, sched
   │   (every pub fn, edge cases)   │      (large set, fast, CI gate)
   └───────────────────────────────┘
```

### 4.2 Unit test examples by module

**`lex` unit tests:**
- Empty input → `[Eof]`
- Single word → `[Word("hello"), Eof]`
- Rule header: `target: prereq` → `[Word("target"), Colon, Word("prereq"), Newline, Eof]`
- Comment: `# this is a comment\nword` → `[Word("word"), Eof]`
- Escaped newline (sh mode): `foo\\\nbar` → `[Word("foo"), Word("bar"), Eof]`
- Escaped newline (rc mode): `foo\\\nbar` → `[Word("foobar"), Eof]`
- Backtick: `` `echo $HOME` `` → `[Word("`echo $HOME`"), Eof]`
- Single-quoted string with spaces: `'hello world'` → `[Word("'hello world'"), Eof]`
- Recipe indentation: indent → `[Indent, Word("cmd"), Newline, Eof]`
- Double newline terminates recipe: `target:\n\tcmd\n\nnext` → correct tokens

**`parse` unit tests:**
- Simple rule: `target: prereq` → `Rule { targets: ["target"], prereqs: ["prereq"], recipe: None }`
- Rule with recipe:
  ```
  target: prereq
      echo hello
  ```
  → `Rule { targets: ["target"], prereqs: ["prereq"], recipe: Some("echo hello\n") }`
- Multiple targets: `a b: c d` → `Rule { targets: ["a", "b"], prereqs: ["c", "d"], ... }`
- Attributes: `target:VQ: prereq` → `Rule { attributes: VIRTUAL | QUIET, ... }`
- Assignment: `CC = gcc` → `Assign { name: "CC", value: "gcc" }`
- Assignment with unexport: `CC:U = gcc` → `Assign { name: "CC", value: "gcc", attributes: UNEXPORTED }`
- Include: `< subdir/mkfile` → `Include::File("subdir/mkfile")`
- Include command: `<| gcc -M *.c` → `Include::Command("gcc -M *.c")`
- Metarule: `%.o: %.c` → `Rule { targets: ["%.o"], prereqs: ["%.c"], is_metarule: true }`
- Regex metarule: `([a-z]+)\\.o:R: \\1.c` → `Rule { is_regex: true, ... }`
- Error: missing colon → `ParseError::ExpectedColon`

**`graph` unit tests:**
- Single node, no prereqs: `a:` → graph with 1 node, 0 arcs
- Chain: `a: b`, `b: c` → 3 nodes, arcs: b→a, c→b
- Diamond: `a: b c`, `b: d`, `c: d` → 4 nodes, 4 arcs
- Cycle: `a: b`, `b: a` → `GraphError::Cycle`
- Metarule match: `%.o: %.c` applied to target `foo.o` → edge from `foo.c` to `foo.o` with stem `foo`
- Pruning: metarule edge removed when concrete rule exists for same target
- Virtual target: `V` attribute → node marked virtual, not checked for file existence
- Transitive closure: `target: a`, `a: b`, `b` exists on disk → graph includes `target → a → b`, stops at `b`

**`var` unit tests:**
- Simple: `$FOO` where `FOO=bar` → `"bar"`
- Bracketed: `${FOO}` → `"bar"`
- Recursive: `A=$B`, `B=hello` → `$A` → `"hello"`
- Substitution: `${FOO:.c=.o}` where `FOO=src/main.c` → `"src/main.o"`
- Pattern substitution: `${FOO:src/%/main.c=%}` → stem replacement
- `$$` → literal `$`
- Undefined variable: `$UNDEFINED` → `""` (mk convention) + warning
- Namelist: `A=1 2 3`, `$A` → `"1 2 3"`, `$A(1)` → `"1"`, `$A(2)` → `"2"`
- Environment import: `$HOME` → from `std::env::var("HOME")`

**`sched` unit tests (with mock shell):**
- Single target, one job → job dispatched, returns success
- Chain: a→b→c, all stale → jobs run in correct order (c first, then b, then a)
- Diamond: independent branches d→b and d→c → both can run in parallel (if NPROC≥2)
- NPROC=1 → sequential execution, topological order
- NPROC=4 → up to 4 concurrent jobs
- `E` attribute → exclusive job blocks other workers
- `-k` flag → failure in one job doesn't abort others (as long as prereqs satisfied)
- Recipe error → node marked failed, prereqs that depend on it not built
- `-n` flag → no recipes actually execute, all jobs "succeed"
- `-t` flag → mtime updated on targets, no recipes executed

### 4.3 Integration test examples

Test fixtures in `testdata/` directory:

**`testdata/hello/` — simplest C compilation:**
```
mkfile:
    hello: hello.c
        cc -o hello hello.c
hello.c:
    int main() { return 0; }
```
→ `make testdata/hello/hello.json` builds, checks hello exists and is newer than hello.c

**`testdata/library/` — object file compilation with metarules:**
```
mkfile:
    CC = cc
    CFLAGS = -Wall -O2
    %.o: %.c
        $CC $CFLAGS -c $stem.c -o $stem.o
    prog: main.o util.o
        $CC -o $target $prereq
```
→ builds prog from two .o files, each compiled from .c

**`testdata/data-pipeline/` — data transformation (Mike Bostock style):**
```
mkfile:
    data/processed.csv: data/raw.csv
        python transform.py $prereq > $target
    report.html: report.Rmd data/processed.csv
        R -e "rmarkdown::render('$stem.Rmd')"
```
→ simulates a data science workflow

**`testdata/includes/` — nested mkfile includes:**
```
mkfile:
    < subdir/common.mk
    target: prereq
        cmd
common.mk:
    CC = gcc
```
→ verifies include chaining, variable scope isolation

**`testdata/regex/` — regex metarules:**
```
mkfile:
    ([a-z]+)\.o:R: \1.c
        cc -c $stem1.c -o $target
```
→ verifies regex rule matching and grouped stem variables (`$stem1`, `$stem2`)

### 4.4 Property tests

- **Parse roundtrip**: generate random valid AST → serialize to mkfile text → parse back → assert AST equal
- **Graph invariants**: generated graphs must be acyclic, no dangling arcs, transitive closure is idempotent
- **Variable expansion**: random variable names → expansion is always deterministic (same input → same output)
- **Shell quoting**: any string, when quoted and then parsed by the shell, should produce the original string (roundtrip through `shell.quote()`)

---

## 5. Implementation phases

> See `TRACEABILITY.md` for the full F-xxx → module → phase mapping.

### Phase overview

| Phase | Features | Effort | What you can do after | Progress |
|-------|:---:|--------|-----------------------|:--------:|
| **1a** — Core MVP | 22 | ~2 weeks | Build a C program from explicit rules | 22/22 ✅ |
| **1b** — Variables & includes | 12 | ~1.5 weeks | Multi-file projects with `< file` includes | 12/12 ✅ |
| **2** — Metarules & parallel | 22 | ~2.5 weeks | `%.o: %.c` patterns, NPROC parallel builds | 0/22 |
| **3** — Aggregates & polish | 10 | ~2 weeks | Full plan9port mk compatibility | 0/10 |
| **Deferred** — Plan 9 specifics | 4 | — | `$O`, `membername`, stdout-as-mkfile | 0/4 |

*Phase 1a modules done: `lex` (✓), `attr` (✓), `error` (✓). Next: `parse`, `graph`.*

### Phase 1a: Core parser + serial execution (REAL MVP)

**Goal:** Parse simple mkfiles, build DAG from concrete rules only, execute recipes sequentially with sh. This is the smallest thing that could possibly work — you can build a C program from an explicit mkfile.

**Estimated effort:** ~2 weeks (one person, part-time)

**Features:** 22 specs (F-001..F-003, F-006, F-008..F-011, F-014..F-016, F-018, F-020..F-021, F-025, F-040..F-042, F-045, F-059, F-064..F-065, F-067..F-068)

| Module | Scope |
|--------|-------|
| `lex` | Tokenizer: words, colons, equals, indents, comments, escaped newlines (sh mode only). **No** backticks, **no** `<` include tokens yet |
| `parse` | Rules (single target), assignments, attributes (V, Q, N), recipes. **No** metarules, **no** includes, **no** multi-target rules |
| `graph` | DAG from concrete rules. Transitive closure (follow prereqs). Staleness via `mtime`. Cycle detection. Virtual targets |
| `var` | `$VAR`, `${VAR}`, `$$`. Environment import. Precedence (cmdline > file > env > builtin). **No** namelists, **no** substitution patterns |
| `shell` | `Shell` trait in mk-core. `ShShell` via `duct`. Recipe fed to stdin |
| `recipe` | First-char elision. Environment export of user vars. Error code → MkError |
| `sched` | Serial only (NPROC=1). Single job dispatch, synchronous `wait()` |
| `attr` | V, Q, N parsing. Bitflags struct |
| `cli` | `clap` derive: `-f`, `-n`, `-e`, `-t`, `-a`. Targets as positional args. `--version` |

**Tests:**
- Unit: lex (token roundtrip), parse (rule/assign), graph (3-node chain, cycle detection), var (expansion), attr (bitflags)
- Integration: `testdata/hello` — single C file → binary
- Integration: `testdata/hello-rebuild` — touch source → rebuild; no changes → up-to-date

**Exit criteria:**
- `cargo run -- -f mkfile hello` builds a C program
- Changing source → rebuilds target
- Unchanged source → "hello is up to date"
- Cycle in mkfile → error with file:line
- Missing prerequisite and no rule → error
- `-n` prints recipes without executing
- All unit tests green (target: >90% coverage on lex, parse, graph)

---

### Phase 1b: Variables, includes, basic attributes

**Goal:** Round out the core: recipe-time variables, `< file` includes, missing intermediate optimization, and CLI flags that don't require parallelism.

**Estimated effort:** ~1.5 weeks

**Features:** 12 specs (F-013, F-017, F-022..F-024, F-026, F-031, F-033..F-034, F-038, F-046..F-049, F-051, F-062, F-069)

| Module | Scope |
|--------|-------|
| `lex` | Backtick tokens (`` `cmd` ``). `<` and `|` tokens for includes |
| `parse` | Multi-target rules. Include directives (`< file`). Recipe-less rules (prereq merging) |
| `graph` | Missing intermediate optimization. `-i` flag support. Prerequisite merging from multiple rules |
| `var` | `$target`, `$prereq`, `$pid` builtins. Short-circuit eval (parse vs execution time). `$newprereq` |
| `include` | `< file` with recursive parsing. Circular include detection. Include stack for errors |
| `sched` | `-k` keep-going. `-t` touch mode. `-a` force rebuild |
| `attr` | E, U attribute support |
| `cli` | `-k`, `-i`, `-w` flags |

**Tests:**
- Unit: include (recursive, circular), var ($target/$prereq in recipe context), parse (multi-target)
- Integration: `testdata/includes` — nested mkfiles, variable scoping
- Integration: `testdata/intermediates` — missing .o file, pretend-timestamp optimization

**Exit criteria:**
- `< subdir/mkfile` includes and merges rules correctly
- Circular include → error with chain printed
- `$target` and `$prereq` expand to correct values in recipe
- Missing intermediate .o is skipped if dependents are up to date
- `-k` continues after a recipe failure
- `-t` touches targets without running recipes

---

### Phase 2: Parallelism + metarules + full variables

**Goal:** Achieve feature parity with plan9port mk's core functionality: parallel builds, pattern metarules, and all recipe-time variables.

**Estimated effort:** ~2.5 weeks

**Features:** 22 specs (F-004..F-005, F-007, F-019, F-027, F-029, F-035..F-037, F-039, F-044, F-052..F-056, F-058, F-060..F-061, F-063, F-066)

**Deliverables:**

| Area | Scope |
|------|-------|
| `sched` | NPROC-based worker pool (crossbeam channels + threads). Multiple jobs run concurrently. Semaphore for backpressure. Exclusive (`E`) job support. Progress echo (`target: ...`) |
| `parse` | `%` metarules (single `%` match), `&` metarules (ampersand matchers), `R:` regex metarules |
| `graph` | Metarule application: for each unknown target, try `%` metarules, then `&`, then `R`. Pruning: vacuous + ambiguous edge removal. Missing intermediate handling (`-i` flag) |
| `var` | `$stem`, `$target`, `$prereq`, `$newprereq`, `$alltarget`. Namelist access: `$stem(1)`, `$stem(N)`. Recipe-time variable expansion |
| `shell` | `RcShell` in mk-shell. Shell selection via `$MKSHELL` and per-rule `S:` attribute. Proper rc env format (`\x01` separator) |
| CLI | `-p N` / `$NPROC` for parallelism. `-i` for missing intermediates. `-k` for keep-going |

**Tests:**
- Parallel execution: diamond DAG with sleep recipes → verify concurrent execution via timestamps
- Metarule matching: `%.o: %.c` with 3 .c files → 3 rules applied, correct stems
- Regex metarule: complex pattern with multiple capture groups → `$stem1`, `$stem2`
- Variable edge cases: recursive `$target` in prereqs, `$prereq` with multiple words
- Integration: `testdata/library` with metarules (%.o: %.c)
- Integration: `testdata/regex` with R: metarules

**Exit criteria:**
- `NPROC=4` builds diamond DAG with demonstrable parallelism (wall time < sum of recipe times)
- `%` and `R:` metarules work identically to plan9port mk
- `$stem`, `$target`, `$prereq` expand correctly inside recipes
- `-i` flag builds missing intermediate targets automatically
- `<| command` executes command and includes its stdout as mkfile
- `${VAR:%.c=%.o}` namelist transforms work
- All plan9port mk regression tests (basic rule set) pass

---

### Phase 3: Includes + dynamic features + polish

**Goal:** Full feature parity with plan9port mk, plus Rust-native improvements. Production-ready.

**Estimated effort:** ~2 weeks

**Features:** 10 specs (F-024, F-028, F-030, F-032, F-043, F-050, F-058)

**Deliverables:**

| Area | Scope |
|------|-------|
| `include` | `<| command` includes (execute command, parse stdout as mkfile). Circular include detection. Include stack for error messages. Dynamic mkfile generation |
| `graph` | Hash-based staleness (optional, from knit): `--hash` flag uses blake3 hash instead of mtime. Recipe-change tracking: if recipe text changes, target is stale |
| `archive` | `lib(member)` auto-rule generation. Archive mtime tracking |
| `recipe` | Stdout-as-mkfile: if recipe produces mkfile-parseable output on stdout, it can be treated as an include (dynamic dependency generation). Error messages with source location |
| `var` | Full `shsub()`-compatible substitution. All edge cases from plan9port test suite |
| CLI | All flags: `-d` (debug), `-k` (keep-going), `-s` (silent), `-w` (what-if), `-C dir` (chdir). `--color auto/always/never`. `--json` for machine-readable output. `--version`, `--help` |
| Docs | Man page (`mk.1.md`). README with quick-start. Integration test READMEs. API docs for mk-core |

**Exit criteria:**
- Passes all plan9port mk regression tests (ported to Rust test format)
- `cargo install --path crates/mk-cli` installs on Linux/macOS
- Man page in `man/man1/mk.1`
- `< 2 seconds to check 100-target project with no changes
- Zero panics in normal operation — all errors propagate as `Result`
- `lib(member)` syntax generates archive rules automatically
- `-d[egp]` debug output matches plan9port format
- Recipe stdout can feed back as mkfile input (dynamic includes)

---

## 6. Open design questions

### 6.1 Threading model: crossbeam threads vs tokio async

**Question:** Should parallelism use std::thread via crossbeam channels, or tokio async tasks?

| Approach | Pros | Cons |
|----------|------|------|
| **crossbeam threads** | Simple, matches plan9port fork() model. No runtime dependency. Deterministic. Lower compile times. Channels map well to job queue. | Threads are heavier than tasks. 100 targets with NPROC=100 would be 100 threads. |
| **tokio async** | Lightweight tasks — NPROC=100 is fine. Can use `tokio::process::Command` for recipe execution. `Semaphore` for backpressure. | Adds tokio dependency (~20+ crates). Async I/O is irrelevant for mk (it's CPU-bound fork/exec). Mixing sync File I/O with async can cause issues. |

**Background:** plan9port mk uses `fork()`+`exec()`. The Go ports use goroutines. For a Rust port, the job structure is:
1. Wait for a free slot (semaphore)
2. Spawn a child process (recipe execution)
3. Wait for child to exit (blocking)
4. Signal completion, free slot

This is inherently blocking. Async adds no benefit and adds complexity.

**Tentative answer:** **crossbeam threads.** The parallelism model is a fixed-size thread pool (NPROC threads max), each blocking on `Command::status()`. Simple, fast, no async runtime.

---

### 6.2 Graph memory model: arena (Vec + indices) vs Rc/RefCell

**Question:** How to store the DAG where nodes have bidirectional references?

| Approach | Pros | Cons |
|----------|------|------|
| **Arena (Vec<Node>, Vec<Arc>, usize indices)** | Contiguous memory, cache-friendly. Clear ownership (Graph owns everything). No Rc cycles. Deterministic drop order. Matches plan9port C model (arrays of structs). | Manual index management. Need to be careful not to hold indices across mutations. `NodeIndex` is just `usize` — no type safety against wrong arena. |
| **Rc<RefCell<Node>>** | Each node is an independent allocation. Back-references via `Weak`. No index errors. Rust-idiomatic at first glance. | Interior mutability required for graph building → RefCell borrow panics at runtime. Cycles need Weak. Allocation per node. Slower iteration (pointer chasing). |

**Background:** plan9port C uses a linked list of `Node` structs, each with an `Arc*` to its prereqs and a `Node* next` for the rule's target list. The Go ports use `map[string]*Node` with `[]*Edge`. The Rust port should improve on both.

**Tentative answer:** **Arena (Vec + indices).** Use `NodeIndex(usize)` and `ArcIndex(usize)` newtypes for type safety. Graph building is a single phase — after the graph is built, it's immutable (no more mutations). This eliminates the main risk of index invalidation. Benchmarks will confirm if the contiguous layout outperforms pointer chasing (expected: yes, especially for DAG traversal).

---

### 6.3 Recipe interpreter: external shell vs embedded duckscript

**Question:** Should recipe execution use an external shell process, or embed a scripting language directly?

| Approach | Pros | Cons |
|----------|------|------|
| **External shell (sh/rc)** | Faithful to Plan 9 mk. Users already know sh. No new dependencies. Recipes use existing tools. Simple error model. | Fork/exec per recipe. Environment passing overhead. Platform-specific shell availability. |
| **Embedded duckscript** | No subprocess — faster. Built-in file ops (cp, mv, glob). Cross-platform (no /bin/sh dependency). Already proven in cargo-make. | Adds a dependency. New syntax for users to learn. Not a real shell — can't run arbitrary binaries without `exec`. |
| **Hybrid**: use duckscript as an optimization, external shell as default | Best of both. Simple recipes run in-process. Complex recipes (calling gcc, python, R) still use sh. Configurable per rule. | Two code paths. Complexity in shell selection. |

**Background:** The Plan 9 mk philosophy is that recipes are shell scripts. Both `sh` and `rc` are first-class. The `S:` attribute already supports custom interpreters. Duckscript would just be another interpreter.

**Tentative answer:** **External shell as default, duckscript as optional.** `$MKSHELL` defaults to `sh -c`. `S[duckscript]` attribute enables in-process execution for recipes that only need file ops. Feature-gate duckscript behind a Cargo feature flag (`duckscript`). This keeps the core crate lean while offering a performance option.

---

### 6.4 Staleness detection: mtime vs hash-based

**Question:** Should mk-rust use mtime comparison (like plan9port mk) or content hashing (like knit)?

| Approach | Pros | Cons |
|----------|------|------|
| **mtime only** | Fast — just stat() each file. Simple. Standard behavior — matches plan9port mk and GNU Make. | Touch breaks it. Unchanged content triggers rebuilds. Clock skew issues. No recipe-change detection. |
| **Hash-based (blake3)** | Accurate — only rebuild when content changes. Enables dynamic task elision (if rebuilt prereq produces identical content, dependents skip). Recipe-change triggered rebuilds. | Each build reads every file to compute hash (I/O cost). Slower for large files. Different behavior from Plan 9 mk → compat concern. |

**Background:** GNU Make and plan9port mk use mtime. Knit defaults to hashes. The mtime approach is "good enough" for most use cases. Hash-based is more correct but slower.

**Tentative answer:** **mtime by default, hash-based as an option.** `--hash` flag switches to blake3 content hashing. `P:` attribute (custom comparison) already supports arbitrary staleness logic. Recipe-change tracking can be implemented by hashing the recipe text and storing the hash in `.mk.state` — independent of target file hashing.

---

### 6.5 File watching / daemon mode

**Question:** Should mk-rust support a daemon mode that watches files and auto-rebuilds?

| Approach | Pros | Cons |
|----------|------|------|
| **Daemon mode (`mk -w` or `mk --watch`)** | Useful for rapid iteration. Matches tools like `fswatch`, `cargo watch`. Could leverage inotify/kqueue for efficiency. | Scope creep. mk is a build tool, not a daemon. Plan 9 mk never had this. Adds complexity (signal handling in threads, file watching dependency). |
| **Separate tool (`mk-watch`)** | Clean separation. Use `notify` crate for file watching, run `mk` on changes. Single-purpose Unix tool. | Extra binary. Duplication of target resolution logic. |
| **Don't build it** | Keeps mk-rust focused. Users can script: `while inotifywait .; do mk; done`. | User ergonomics. |

**Tentative answer:** **Don't build it.** mk is a build tool, not a daemon. Plan 9 mk never had this feature. Users who want watch mode can use `watchexec`, `cargo watch`, or a shell one-liner. If demand is high, a separate `mk-watch` tool using the `notify` crate is better than bloating mk-core.

---

### 6.6 Cross-platform support: Windows

**Question:** How much effort should go into Windows support?

- Plan 9 mk assumes Unix: `fork()`+`exec()`, `/bin/sh`, symlinks, signal handling
- The Go ports are Unix-only
- Rust can be cross-platform, but the effort is non-trivial

**Tentative answer:** **Unix-first, Windows-later.** Phase 1–3 target Linux and macOS. Windows support can be explored as a Phase 4 if there's demand. The `Shell` trait and `std::process::Command` abstractions already provide a path. The main hurdles: no `/bin/sh` by default (would need `cmd.exe` shell), different path separators, no `fork()` semantics for NPROC.

---

## 7. Rust idioms to embrace

### 7.1 Error handling: `Result<T, MkError>` everywhere

```rust
// Centralized error type via thiserror
#[derive(Error, Debug)]
pub enum MkError {
    #[error("lex error at line {line}: {message}")]
    Lex { line: usize, message: String },
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("variable error: {0}")]
    Var(#[from] VarError),
    #[error("graph error: {0}")]
    Graph(#[from] GraphError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    // ...
}
```

No panics in library code. Every fallible operation returns `Result`. The CLI layer is the only place where `unwrap()` or `expect()` are acceptable (and even then, prefer `?` in `main()`).

### 7.2 Builder pattern for Rule, Node, Recipe

```rust
let rule = Rule::builder()
    .target("hello.o")
    .prereq("hello.c")
    .recipe("cc -c hello.c -o hello.o")
    .attribute(Attributes::VIRTUAL)
    .build()?;
```

Complex structs with optional fields should have builders. This avoids constructor overloads and makes test code readable.

### 7.3 Trait-based Shell abstraction

Already described in §3.5. The key insight is that `trait Shell` cleanly abstracts the strategy pattern that plan9port C implements via function pointers. It's the most natural Rust translation.

### 7.4 Iterators for graph traversal

```rust
impl Graph {
    /// Iterate over prereqs of a node.
    pub fn prereqs(&self, node: NodeIndex) -> impl Iterator<Item = &Node> { ... }

    /// Topological sort of stale nodes (Kahn's algorithm).
    pub fn topological_sort(&self, roots: &[NodeIndex]) -> Vec<NodeIndex> { ... }

    /// Walk the DAG in dependency order.
    pub fn walk(&self, root: NodeIndex) -> DagWalk { ... }
}
```

Avoid manual index loops in client code. Provide iterator adapters. This makes scheduler logic declarative.

### 7.5 Serde for AST (debugging, future LSP)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule { ... }
```

`serde` `Serialize`/`Deserialize` derives on all AST types. This enables:
- `mk --print-ast --json` → dump parsed AST as JSON (debugging, tooling)
- Future: mkfile formatter (`mk fmt`) — deserialize, reformat, serialize
- Future: LSP server — parse mkfile, serve AST queries

Feature-gated behind `serde` feature flag to keep the default dependency footprint small.

### 7.6 Clap derive for CLI

```rust
#[derive(Parser)]
#[command(name = "mk", about = "maintain dependencies between files")]
struct Cli {
    /// Mkfile to read (default: mkfile)
    #[arg(short = 'f', default_value = "mkfile")]
    file: PathBuf,

    /// Print recipes but do not execute
    #[arg(short = 'n')]
    no_exec: bool,

    /// Touch targets instead of running recipes
    #[arg(short = 't')]
    touch: bool,

    /// Keep going after errors
    #[arg(short = 'k')]
    keep_going: bool,

    /// Maximum parallel jobs (overrides $NPROC)
    #[arg(short = 'p')]
    nproc: Option<usize>,

    /// Targets to build (default: first target in mkfile)
    targets: Vec<String>,
}
```

Clean, self-documenting, auto-generated `--help`. Aligns with Rust CLI conventions.

### 7.7 Criterion for benchmarks

```rust
// benchmarks/graph_benchmark.rs
fn bench_dag_build_1000_nodes(c: &mut Criterion) {
    c.bench_function("dag_build_1000_nodes", |b| {
        b.iter(|| {
            let rules = generate_rules(1000);
            let mut builder = Builder::new(rules, vec![]);
            builder.build(&["all"]).unwrap()
        });
    });
}
```

Use `criterion` for benchmarks of:
- Lexing large mkfiles
- DAG construction (metarule matching)
- Variable expansion of deeply nested references
- Hash-based staleness on large file trees

Not a development gate — just visibility into performance regression.

### 7.8 No unsafe code

Target `#![forbid(unsafe_code)]` on `mk-core`. The only potential need for `unsafe` would be optimization of hot paths — and that can be deferred until benchmarks prove it necessary. The C codebase has many raw pointer casts; Rust's ownership model eliminates the need for almost all of them.

---

*This plan is a living document. Decisions marked "Tentative answer" are subject to change as implementation reveals new constraints. Open questions in §6 should be resolved before reaching the relevant phase.*
