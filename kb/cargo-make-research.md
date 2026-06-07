# cargo-make Research — Findings for mk-rust

**Repo**: https://github.com/sagiegurari/cargo-make (v0.37.24)
**Author**: Same as duckscript (Sagie Gur-Ari)
**Key dependencies**: duckscript ^0.10, duckscriptsdk ^0.11, envmnt ^0.10, petgraph ^0.8, cliparser ^0.1, run_script ^0.11

---

## 1. Duckscript Integration (Clean API Boundary)

### Architecture

cargo-make uses duckscript in three layers:

**Layer 1 — duckscript crate** (`duckscript::runner::run_script`)
- Low-level script runner. Takes script text + `Context` (variables map) + `Commands` (registered commands).
- cargo-make never calls this directly for task scripts; it goes through its script engine.

**Layer 2 — duckscriptsdk crate** (`duckscriptsdk::load`)
- Standard library of ~50+ commands (echo, exec, cd, set_env, get_env, sleep, array_push, etc.).
- cargo-make loads these via `duckscriptsdk::load(commands)?` once per script execution.
- This is the **public API** for script writers.

**Layer 3 — cargo-make's own SDK** (`scriptengine/duck_script/sdk/`)
- Adds one custom command: `cm_run_task` — allows running any cargo-make task from within duckscript.
- Registered via `sdk::load(commands, flow_info, flow_state)`.
- Supports both sync and async (`--async`) task invocation.

### How scripts are executed (the flow)

1. `scriptengine::invoke()` receives a `Task` with `script_runner = "@duckscript"`
2. `get_engine_type()` recognizes `@duckscript` → `EngineType::Duckscript`
3. `duck_script::execute()` is called with the script text, cli args, flow_info, flow_state
4. It prepends: `exit_on_error true\n@ = array ${1} ${2} ...` (maps CLI args)
5. Creates a `Context` (variable map):
   - Inserts CLI args as `$1`, `$2`, etc.
   - Imports **all environment variables** as duckscript variables (`envmnt::vars()`)
6. Loads SDK commands (duckscriptsdk + cm_run_task)
7. Calls `runner::run_script(&script_text, context, None)`
8. After execution, restores original working directory

### Key insight for mk-rust

```rust
// Clean pattern: duckscript integration is dead simple
let mut context = duckscript::types::runtime::Context::new();
context.variables.insert("key".to_string(), "value".to_string());
duckscriptsdk::load(&mut context.commands)?;
duckscript::runner::run_script(&script, context, None)?;
```

**The API boundary is clean**: 3 calls — create context, load SDK, run script. No deep coupling.

**For mk-rust**: We could do the same — embed duckscript (not as a separate binary) and provide a `duckscriptsdk`-like library of mk-specific commands (e.g., `mk_rule`, `mk_var`, `mk_shell`). Make it opt-in: users who don't want duckscript don't need it.

---

## 2. DAG/Task Dependency Model

### How it works

cargo-make **does NOT use topological sort** for dependencies. It uses a simple **depth-first flattening** approach:

1. `ExecutionPlanBuilder::build()` creates a `Vec<Step>` (ordered list)
2. `create_for_step()` recursively walks dependencies:
   ```
   create_for_step(task):
       for each dependency:
           create_for_step(dependency)  // recurse first
       if task not already in steps list:
           steps.push(task)             // append after deps
   ```
3. Cycle detection: if a task name is already in the `task_names` HashSet and it's the root → error.
4. Alias resolution: recursive with cycle detection via `seen` Vec.
5. Task extension: `Task::extend()` method handles the "extends" pattern (base task + override).

### Decision NOT to use petgraph

Despite having `petgraph = "^0.8.1"` in Cargo.toml, cargo-make doesn't use it for dependency resolution. The simple DFS approach is sufficient because:
- Tasks are small in number
- Order is sequential (no parallel execution of deps)
- The graph is implicit: tasks reference other tasks by name

### What mk-rust can adopt

The `Vec<Step>` pattern is exactly what mk-rust's `Mkfile::run("target")` should produce:
```rust
struct Step {
    name: String,
    recipe: Recipe,  // command or script to execute
}
struct ExecutionPlan {
    steps: Vec<Step>,
}
```

**Don't copy**: cargo-make's task extension/composition model (Task::extend, clear, namespacing). It's complex and not needed for mk (Plan 9 mk uses simple variable-based rules).

---

## 3. Environment Variable Handling

### The envmnt crate

cargo-make uses `envmnt` for all environment operations:
- `envmnt::set("KEY", "value")` — set env var
- `envmnt::get_or("KEY", "default")` — get with default
- `envmnt::is("KEY")` — check if true
- `envmnt::expand("text ${VAR} text", options)` — variable expansion

### Expansion style

cargo-make uses **`${VAR}` with default values** (`UnixBracketsWithDefaults`):
```rust
fn expand_value(value: &str) -> String {
    let mut options = ExpandOptions::new();
    options.expansion_type = Some(ExpansionType::UnixBracketsWithDefaults);
    options.default_to_empty = false;
    envmnt::expand(&value, Some(options))
}
```
This is exactly Plan 9 mk's `$VAR` / `${VAR}` style. The `envmnt` crate handles:
- `${VAR}` — simple substitution
- `${VAR:-default}` — with default
- `$VAR` — also works (UnixBrackets style)

### Env value types

cargo-make supports **many env value types** via its `EnvValue` enum:
```rust
enum EnvValue {
    Value(String),       // static string
    Boolean(bool),       // true/false
    Number(isize),
    List(Vec<String>),
    Unset(EnvValueUnset),
    Script(EnvValueScript),   // run script to get value
    Decode(EnvValueDecode),   // map value via lookup
    Conditional(EnvValueConditioned),
    PathGlob(EnvValuePathGlob),
    Profile(IndexMap<String, EnvValue>),
}
```

### For mk-rust

**Adopt**: The `EnvValue` pattern is clean but **too complex** for mk-rust. We only need:
- `Value(String)` — with `$VAR` / `${VAR}` expansion (this is mk's core model)
- Maybe `Boolean` / `List` for convenience

**Don't adopt**: `Script`, `Decode`, `Conditional`, `PathGlob`, `Profile` — these add complexity that mk-rust doesn't need. Plan 9 mk handles everything via shell recipes and variables.

---

## 4. Cross-Platform Shell Handling

### The `run_script` crate

cargo-make uses `run_script` (also by same author) as its OS script runner:
```rust
run_script::run(script_text, cli_arguments, &options)
```

The `run_script` crate:
- On **Unix**: writes script to temp file, runs with `sh -c` or custom runner
- On **Windows**: writes script to temp `.cmd`/`.bat` file or runs with `cmd /c`
- Provides: stdout capture, exit code, print commands, timeout

### cargo-make's script engine dispatch

The `scriptengine` module handles **6 engine types**:
1. **OS** — default: system shell (`sh` on Unix, `cmd` on Windows)
2. **Duckscript** — `@duckscript` runner
3. **Rust** — `@rust` runner (compiles and runs Rust code inline)
4. **Shell2Batch** — `@shell` runner (converts shell commands to batch)
5. **Generic** — custom runner with file extension
6. **Shebang** — detects `#!` line to select runner

### How it picks the shell

```rust
fn get_engine_type(script, script_runner, script_extension) {
    match script_runner {
        Some("@duckscript") → EngineType::Duckscript
        Some("@rust")        → EngineType::Rust
        Some("@shell")       → EngineType::Shell2Batch
        Some(other) → EngineType::OS (with custom runner)
        None → detect shebang or fallback to OS
    }
}
```

### For mk-rust

**Adopt the Shell trait design**:
```rust
trait Shell {
    fn name(&self) -> &str;
    fn execute(&self, script: &str, args: &[String]) -> Result<i32>;
}
```

cargo-make's `EngineType` enum is a good model, but we can simplify:

```rust
enum ShellKind {
    Sh,           // /bin/sh on Unix
    Cmd,          // cmd.exe on Windows
    Duckscript,   // inline duckscript
    Custom(String), // user-specified runner
}
```

**Don't adopt**: Shell2Batch conversion, `@rust` inline compilation, `@shell` alias. These are cargo-make-specific. mk-rust should just delegate to the system shell and optionally support duckscript.

---

## 5. CLI Design

### Available flags

```
cargo make [OPTIONS] [TASK] [-- TASK_ARGS...]

--makefile <FILE>       Custom Makefile.toml path
--profile <NAME>        Profile name (development, release, etc.)
--loglevel <LEVEL>      Log level (verbose, info, error, off)
--no-workspace          Disable workspace support
--no-on-error           Disable on-error task
--allow-private         Allow running private tasks
--skip-init-end-tasks   Skip init/end tasks
--skip-tasks-pattern    Skip tasks matching pattern
--print-steps           Only print execution plan
--list-steps            List all known tasks
--diff-steps            Diff execution plans
--env <KEY=VALUE>       Set environment variable
--env-file <FILE>       Load env vars from file
--cwd <DIR>             Working directory
--output-format         Output format (default, json)
--time-summary          Print timing summary
--completion <SHELL>    Generate shell completions
--experimental          Enable experimental features
--help / --version
```

### What mk-rust should adopt

| Feature | Adopt? | Rationale |
|---------|--------|-----------|
| `--loglevel` | Yes | Essential for debugging |
| `--env KEY=VALUE` | Yes | mk's `-e` flag equivalent |
| `--print-steps` | Yes | Dry-run / debug (like `mk -n`) |
| `--help` / `--version` | Yes | Standard |
| `--cwd` | Yes | Plan 9 mk doesn't have this, but useful |
| `--list-steps` | Maybe | `mk -a` shows all targets |
| `--profile` | No | Not needed — mk is simpler |
| `--completion` | No | Out of scope |
| `--experimental` | No | Over-engineered for mk-rust |
| `--time-summary` | No | Nice-to-have, not essential |

**Don't adopt**: The `--no-workspace`, `--no-on-error`, `--allow-private`, `--skip-*` flags. These are cargo-make-specific complexity.

---

## 6. Plugin/Extension System

### How plugins work

cargo-make's plugin system is **duckscript-based**:

```toml
[plugins]
aliases = { my-plugin = "impl1" }

[plugins.impl]
impl1 = { script = "echo hello from plugin" }
```

A task can specify `plugin = "my-plugin"`. The plugin runner:
1. Resolves aliases (with cycle detection)
2. Creates a duckscript context with task metadata injected as variables
3. Runs the plugin's script via duckscript
4. The plugin script can inspect task properties via `task.command`, `task.args`, `task.script`, etc.

### Plugin SDK (types.rs)

```rust
pub struct Plugin {
    pub script: String,
}
pub struct Plugins {
    pub aliases: Option<IndexMap<String, String>>,
    pub plugins: IndexMap<String, Plugin>,
}
```

### For mk-rust

**Don't adopt this plugin system**. It's purpose-built for cargo-make's feature set (installing crates, running commands, etc.). For mk-rust, the "plugin" is just writing a recipe that calls another rule or runs a shell script. That's the Unix philosophy — no plugin framework needed.

If we want duckscript hooks later, make them recipe-level, not plugin-level.

---

## 7. Testing Approach

### Structure

- Test files are inlined via `#[path = "mod_test.rs"]` — each module has its test file alongside.
- Mock-heavy: uses environment variable injection (envmnt) for testing.
- Integration tests in `src/lib/test/` — workspace structures, etc.
- `expect-test` crate for snapshot-style assertions.
- Minimal mocking of external processes (cargo-make tests mostly test config parsing, plan generation, and condition evaluation).

### Good patterns

```rust
// Inline tests next to code
#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_test;
```

### For mk-rust

We can borrow:
1. Inline tests per module (`#[path = "module_test.rs"]`)
2. Test environment variable setup/teardown patterns
3. Execution plan generation tests (feed a config, verify step order)
4. Shell execution tests using `assert_cmd` or similar

---

## 8. Clean Code Worth Learning From

### 1. `scriptengine/mod.rs` — Engine dispatch
The pattern of detecting engine type from a string marker (`@duckscript`, `@rust`) and dispatching to the right handler is clean. The `get_engine_type()` -> `invoke()` chain is straightforward.

### 2. `execution_plan.rs` — Recursive DFS flattening
The dependency flattening code is surprisingly simple for what it does. No petgraph needed. Cycle detection via a `seen` Vec.

### 3. `environment/mod.rs` — `expand_value()` function
```rust
fn expand_value(value: &str) -> String {
    let mut options = ExpandOptions::new();
    options.expansion_type = Some(ExpansionType::UnixBracketsWithDefaults);
    options.default_to_empty = false;
    envmnt::expand(&value, Some(options))
}
```
This is the entire variable expansion layer. We should use `envmnt` too — it already does Plan 9 mk's `$VAR`/`${VAR}` style.

### 4. `duck_script/mod.rs` — Clean duckscript embedding
```rust
fn execute(script, cli_arguments, flow_info, flow_state, validate) {
    let mut context = create_common_context(cli_arguments);
    load_sdk(&mut context.commands, flow_info, flow_state)?;
    runner::run_script(&script_text, context, None)?;
    Ok(true)
}
```
Three function calls, no magic. This is the pattern to replicate in mk-rust.

---

## 9. What NOT to Do (Too Complex for mk-rust)

| Feature | Why Not |
|---------|---------|
| `Task::extend()` with `clear` flag | Inheritance model is over-engineered. mk just needs variable-based rules. |
| Plugin system with duckscript scripts | Over-abstracted. Users can just write recipes. |
| Workspace emulation across members | Cargo-specific, not needed for mk-rust. |
| `EnvValue` with Script/Decode/Conditional types | Too many env value types. mk only needs `$VAR` substitution. |
| `@rust` script runner (compile & run Rust inline) | Novel but adds dependency on rustc at runtime. Out of scope. |
| Shell2Batch conversion | Windows-specific complexity. Let the OS handle it. |
| Watch mode (file watching) | Separate concern. mk-rust should leave this to tools like watchexec. |
| On-error tasks / cleanup tasks | mk's simple error model (recipe fails → mk fails) is sufficient. |
| Profile system (dev/release profiles) | Plan 9 mk has no profiles. Keep it simple. |

---

## Summary: Action Items for mk-rust

1. **Use `envmnt` for env expansion** — already supports `${VAR}` and `$VAR` style.
2. **Embed duckscript like cargo-make does** — 3 function calls (create_ctx, load SDK, run).
3. **Dependency resolution = DFS flattening** — no petgraph needed.
4. **Shell trait** = `EngineType` dispatch but simpler (Sh, Cmd, Duckscript, Custom).
5. **CLI: --loglevel, --env, --print-steps, --cwd** — adopt these 4 flags.
6. **ExecutionPlan = Vec<Step>** — simple ordered list.
7. **Skip**: plugins, task inheritance, workspaces, profiles, watch mode.
