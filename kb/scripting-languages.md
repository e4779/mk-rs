# Embedded Scripting Languages for Rust Build Tool

Research date: 2026-06-07
Context: evaluating embeddable scripting languages as a recipe language for `mk-rust` — a Rust-native Make-like build tool.

---

## Comparison Table

| Criterion | Duckscript | Rhai | Rune | Koto | mlua | Rust (xtask) |
|---|---|---|---|---|---|---|
| **GitHub Stars** | 581 | 5,416 | 2,254 | 872 | ~2,500 | N/A |
| **License** | Apache-2.0 | Apache-2.0 / MIT | Apache-2.0 / MIT | MIT | MIT | N/A |
| **Last Release** | Feb 2026 | May 2026 | May 2026 | Jun 2026 | Active | N/A |
| **Maturity** | Mature (6+ yr) | Mature (10+ yr) | Mature (5+ yr) | Mature (5+ yr) | Very mature | N/A |
| **Crate Downloads** | ~100k | ~3M+ | ~250k | ~50k | ~2M+ | N/A |
| **Syntax Style** | Shell-like | Rust/JS hybrid | Rust-like | Python-like | Lua | Rust |
| **Process Execution** | Built-in (`exec`, `spawn`) | ❌ Must register custom fns | ✅ Full `process::Command` wrapper | ✅ Full `os.command` wrapper | ✅ Via `io.popen` or ffi | ✅ Native |
| **Has Make-like features?** | Yes (SDK: fs, glob, file ops) | No (must register all) | No (has fs/http modules) | No (has fs/os modules) | No | N/A |
| **Async support** | No | No | ✅ (tokio-based) | No | Yes | Yes |
| **Safety/Sandboxing** | None | ✅ Full sandbox | ✅ Stack isolation | None | Sandbox via environments | Compile-time |
| **Embedding Code Size** | ~3 lines | ~5 lines | ~20 lines (diagnostics) | ~5 lines | ~5 lines | N/A |
| **Learning Curve** | Trivial | Moderate | High (Rust-like) | Low (Python-like) | Low | Depends |
| **Runtime Performance** | Interpreted | Fast (AST-walk) | VM-based, fast | Bytecode VM | Fast (LuaJIT) | Native |
| **Ecosystem** | cargo-make's DSL | Plugins, serde, debugger | Modules, hot-reload, WASM | Core libs + playground | Lua 5.4 ecosystem | Full Rust |
| **Recipe Suitability** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |

---

## Candidate Deep Dives

### 1. Duckscript — `duckscript` (⭐ 581)

**Already used by cargo-make.** This is the most natural fit for a Make-replacement build tool.

#### Embedding

```rust
use duckscript::{runner, types::context::Context};
use duckscriptsdk;

fn run_recipe(script: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut context = Context::new();
    duckscriptsdk::load(&mut context.commands)?;
    runner::run_script(script, context, None)?;
    Ok(())
}
```

#### Recipe example (compile .c to .o)

```sh
# A Make-like recipe in Duckscript
CC = gcc
CFLAGS = -Wall -O2

for file in ${SRC_FILES}
    # Build object file path
    objfile = replace ${file} ".c" ".o"
    objdir = dirname ${objfile}
    # Ensure output dir exists (noop if exists)
    mkdir -p obj/${objdir}

    exec gcc -c ${CFLAGS} -o obj/${objfile} ${file}

    # Check if command failed
    if is_command_defined exec
        # exec handles its own errors
    end
end
```

#### Pros
- **Trivially simple** — shell-like syntax, no learning curve
- **Built-in file ops**: `cp`, `mv`, `rm`, `mkdir`, `glob_array`, `is_path_newer`
- **Built-in process**: `exec`, `spawn`, `exit`, `pid`
- Already proven in `cargo-make` (tens of thousands of users)
- Direct embedding with 3 lines of code
- SDK commands are async-free, synchronous, and predictable

#### Cons
- No sandboxing — scripts have full access
- No async support
- Variables are global-only (no scoping)
- Syntax can be verbose for complex logic
- Relatively small community
- Last commit Feb 2026 — moderately active

#### Recipe Suitability: EXCELLENT
The best fit. Built for this exact use case. The `duckscriptsdk` has everything a build tool needs: file system ops, process execution, globbing, environment variables.

---

### 2. Rhai — `rhai` (⭐ 5,416)

**The most popular Rust embedded scripting language.** Rust-like syntax, fully sandboxed, very fast.

#### Embedding

```rust
use rhai::{Engine, Scope};

fn build_recipe(script: &str) -> Result<(), rhai::EvalAltResult> {
    let engine = Engine::new();
    let mut scope = Scope::new();

    // Register custom build commands
    engine.register_fn("cc", |src: &str, flags: &str| -> String {
        let output = std::process::Command::new("gcc")
            .args(flags.split_whitespace())
            .arg("-c")
            .arg(src)
            .output()
            .expect("gcc failed");
        String::from_utf8_lossy(&output.stdout).to_string()
    });

    engine.run_with_scope(&mut scope, script)
}
```

#### Recipe example (compile .c to .o)

```rust
// Rhai recipe — needs registered Rust functions for shell commands
let cc = "gcc";
let cflags = "-Wall -O2";
let src = "src/main.c";
let obj = "obj/main.o";

mkdir("obj");  // custom registered function
let result = run_command(`${cc} ${cflags} -c ${src} -o ${obj}`);
print(result);
```

#### Pros
- Huge community (5.4k ⭐), actively maintained (10 years)
- Full sandboxing — safe for untrusted scripts
- Tight Rust integration via `register_fn`, custom types, plugins
- Very fast (AST-walk interpreter, ~1M iterations in 0.14s)
- Debugger, serde, WASM/no_std support
- `unsafe`-lite codebase

#### Cons
- **No built-in process execution** — must register everything from Rust
- No built-in file system commands — must register `mkdir`, `cp`, `glob`, etc.
- No async support
- Rust-like syntax may be overkill for simple recipes
- The heavy lifting is in the Rust host code, not the script

#### Recipe Suitability: GOOD (with custom host API)
If you build a rich host API (registering `exec`, `mkdir`, `glob`, `cp`, etc.), Rhai becomes a powerful recipe language. But you're essentially reimplementing the duckscript SDK in Rust. The sandboxing is a unique advantage for safety.

---

### 3. Rune — `rune` (⭐ 2,254)

**An embeddable Rust-like dynamic language with a VM.** Rust syntax in a dynamic language — fancier but heavier.

#### Embedding

```rust
use rune::{Context, Source, Sources, Vm, FromValue};
use std::sync::Arc;

fn run_rune_recipe(script: &str) -> rune::support::Result<()> {
    let mut context = rune_modules::default_context()?;
    let runtime = Arc::new(context.runtime()?);

    let mut sources = Sources::new();
    sources.insert(Source::memory(script)?)?;

    let unit = rune::prepare(&mut sources)
        .with_context(&context)
        .build()?;

    let mut vm = Vm::new(runtime, Arc::new(unit));
    vm.call([], ())?;
    Ok(())
}
```

#### Recipe example (compile .c to .o)

```rust
// Rune recipe — uses built-in process module
use process::Command;
use std::fs;

pub fn main() {
    let cc = "gcc";
    let cflags = "-Wall -O2";

    for entry in fs::read_dir("src")? {
        let file = entry?.path();

        if file.ends_with(".c") {
            let obj = "obj/" + file.trim_end(".c") + ".o";
            let _ = fs::create_dir_all("obj");

            let mut cmd = Command::new(cc);
            cmd.arg("-c");
            cmd.arg(cflags);
            cmd.arg(&file);
            cmd.arg("-o");
            cmd.arg(&obj);
            let output = cmd.output().await?;

            if !output.status.success() {
                panic!("gcc failed");
            }
        }
    }
}
```

#### Pros
- Rust-like syntax — natural for Rust developers
- **Built-in `process::Command` module** — real async process execution
- Hot-reloading support
- Stack isolation for safety
- Async/await, generators, pattern matching
- Serde support, macros, template strings

#### Cons
- **Complex embedding** — ~20 lines for proper setup with diagnostics
- Requires Tokio runtime (async)
- Heavy dependency tree (Tokio, etc.)
- Dynamic dispatch overhead
- The most complex option — overkill for recipes
- 68 open issues, some churn

#### Recipe Suitability: GOOD (but heavy)
Excellent if you need async process execution and Rust-like syntax. But the Tokio dependency, embedding complexity, and overall weight make it harder to justify for a build tool that should start fast.

---

### 4. Koto — `koto` (⭐ 872)

**A simple, Python-like embedded language.** Clean syntax, bytecode VM, active development.

#### Embedding

```rust
use koto::prelude::*;

fn run_koto_recipe(script: &str) -> koto::Result<()> {
    let mut koto = Koto::default();
    koto.compile_and_run(script)?;
    Ok(())
}
```

#### Recipe example (compile .c to .o)

```python
# Koto recipe — uses built-in os.command
cc = "gcc"
cflags = "-Wall -O2"

for entry in os.read_dir("src"):
    if entry.ends_with(".c"):
        obj = "obj/" + entry.trim_right(".c") + ".o"
        os.create_dir("obj")

        cmd = os.command(cc)
        cmd.args(["-c", cflags, entry, "-o", obj])
        output = cmd.wait_for_output()

        if not output.success():
            panic!("gcc failed with exit code: {output.exit_code()}")
```

#### Pros
- Clean Python-like syntax — very readable
- Built-in `os.command` module with full process control
- Bytecode VM — decent performance
- MIT license
- Active development (5 open issues)
- Good documentation and playground

#### Cons
- Smaller community (872 ⭐)
- No sandboxing
- No async support
- Fewer built-in modules than Rhai or Rune
- No hot-reloading or debugger
- Relatively new compared to Rhai

#### Recipe Suitability: GOOD
A strong candidate thanks to built-in process support and clean syntax. The main risk is the smaller ecosystem and community. If it continues growing, it could be an excellent choice.

---

### 5. mlua (Lua) — `mlua` (⭐ ~2,500)

**High-level Rust bindings for Lua 5.4/LuaJIT.** Leverages the mature Lua ecosystem.

#### Embedding

```rust
use mlua::{Lua, Result};

fn run_lua_recipe(script: &str) -> Result<()> {
    let lua = Lua::new();
    lua.load(script).exec()?;
    Ok(())
}
```

#### Recipe example

```lua
-- Lua recipe via mlua
local cc = "gcc"
local cflags = "-Wall -O2"

-- Requires registering io functions from host Rust
for file in io.popen("ls src/*.c"):lines() do
    local obj = file:gsub("%.c$", ".o"):gsub("^src", "obj")
    os.execute(cc .. " -c " .. cflags .. " " .. file .. " -o " .. obj)
end
```

#### Pros
- Battle-tested Lua ecosystem (30+ years)
- LuaJIT is extremely fast
- Simple embedding
- Full `io` and `os` libraries (if enabled)
- Very mature mlua crate

#### Cons
- Lua syntax is unusual for Rust developers (1-indexed, `~=` for not-equal)
- No built-in file globbing (must write or register)
- Must configure standard libraries carefully
- Not Rust-native — different ownership model
- Debugging can be awkward through FFI

#### Recipe Suitability: GOOD (but foreign)
Powerful and fast, but the Lua syntax and mindset is far from Rust. Best for teams already familiar with Lua.

---

### 6. Rust via xtask / build.rs (No Scripting Language)

**Use Rust itself as the "recipe language."** The `xtask` pattern or inline Rust code.

#### Example

```rust
// In xtask or build tool — Rust IS the recipe language
fn compile_c_to_o(src: &Path, obj: &Path) -> Result<()> {
    let status = Command::new("gcc")
        .args(["-c", "-Wall", "-O2"])
        .arg(src)
        .args(["-o"])
        .arg(obj)
        .status()?;

    if !status.success() {
        bail!("gcc failed for {}", src.display());
    }
    Ok(())
}

fn build_project() -> Result<()> {
    let src_dir = Path::new("src");
    let obj_dir = Path::new("obj");
    fs::create_dir_all(obj_dir)?;

    for entry in glob("src/**/*.c")? {
        let src = entry?;
        let obj = obj_dir.join(src.strip_prefix(src_dir).unwrap())
            .with_extension("o");
        fs::create_dir_all(obj.parent().unwrap())?;
        compile_c_to_o(&src, &obj)?;
    }
    Ok(())
}
```

#### Pros
- **Zero dependencies** — no embedded language at all
- Full Rust ecosystem (crates for glob, fs, process)
- Compile-time safety and type checking
- Maximum performance with zero overhead
- Natural for Rust developers
- Easy debugging (Rust tools, backtraces)

#### Cons
- **No dynamic recipes** — must recompile to change build logic
- Higher cognitive load for simple recipes
- Long compile times for the build tool itself
- Overkill for trivial tasks like "copy files"
- Loses the key advantage of scripting: quick iteration

#### Recipe Suitability: MIXED
Excellent for complex, performance-critical build logic. Poor for simple ad-hoc recipes where you want to tweak and rerun. The mk-rust project could use a hybrid: Rust for core logic, scripting for user-facing recipes.

---

## Additional Candidates Found

From the search for "rust scripting language embedded make build tool":

| Crate | Stars | Notes |
|---|---|---|
| **koto-lang/koto** | 872 | Covered above — built-in `os.command` |
| **gluon-lang/gluon** | ~3k | Statically typed, ML-like. Not as active. |
| **mun-lang/mun** | ~1.7k | Ahead-of-time compiled, not well-suited for scripting |
| **boa-dev/boa** | ~7.3k | JavaScript engine in Rust. Heavy, JS syntax. |
| **rulox** | ~38 | Toy implementation of Lox. Not production-ready. |
| **extism** | ~4k | WASM-based plugin system. Interesting but heavy for recipes. |

**Boa (JavaScript)** — 7.3k ⭐, actively maintained. Full ES2020+ support. Possible but heavy — JS for recipes feels wrong for a Rust build tool.

**Extism (WASM plugins)** — Compile recipes to WASM and run via embedded runtime. Novel approach but adds significant complexity.

---

## Recipe Language Comparison: Make-to-Script Translation

A typical GNU Make recipe:

```make
%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

all: $(patsubst src/%.c,obj/%.o,$(wildcard src/*.c))
```

How each candidate expresses this:

| Language | Recipe code | Lines | Clarity |
|---|---|---|---|
| **Duckscript** | `for f in ${files} ... exec gcc -c ${CFLAGS} -o ${obj} ${f}` | ~8 | High — shell-like |
| **Rhai** | `let result = run_command(`${cc} -c ${cflags} -c ${src} -o ${obj}`)` | ~8 | High — but needs host fns |
| **Rune** | `let mut cmd = Command::new(cc); cmd.arg("-c")...` | ~12 | Medium — verbose |
| **Koto** | `cmd = os.command(cc); cmd.args(["-c", cflags, ...])` | ~10 | High — clean |
| **Lua** | `os.execute(cc .. " -c " .. cflags .. " " .. src .. " -o " .. obj)` | ~6 | Medium — concat-heavy |
| **Rust** | `Command::new("gcc").args([...]).status()` | ~10 | Medium — verbose but safe |

---

## Recommendation

### Primary: Duckscript (duckscript)

**Why:** It's already proven in cargo-make (the most popular Rust build tool). It has:
- Built-in `exec` and `spawn` for running commands
- Built-in file operations (`glob_array`, `cp`, `mkdir`, `is_path_newer`)
- Built-in string manipulation and environment access
- The simplest embedding API (3 lines)
- Zero async — synchronous execution is correct for build tools
- Already designed for Make-like workflows

### Secondary: Rhai, with a custom host API

Choose Rhai if you need:
- Sandboxing (untrusted user recipes)
- Strong Rust integration (pass complex types back and forth)
- A larger community and longer-term maintenance
- The ability to build a rich DSL on top of a powerful base

The tradeoff: you'll need to write and maintain the "SDK" of host functions (or contribute to the existing `packages` in Rhai's codebase).

### Not Recommended (for this project):
- **Rune** — too heavy (Tokio dep), embedding too complex, async unnecessary
- **Koto** — promising but too small for a core dependency
- **mlua** — foreign syntax, not Rust-native
- **Rust-only (xtask)** — no dynamic scripting capability

---

## Appendix: Embedding Code Size Comparison

```rust
// Duckscript — 3 lines
let mut ctx = Context::new();
duckscriptsdk::load(&mut ctx.commands)?;
runner::run_script(script, ctx, None)?;

// Rhai — 5 lines
let engine = Engine::new();
let mut scope = Scope::new();
engine.register_fn("exec", |cmd: &str| run_shell(cmd));
engine.run_with_scope(&mut scope, script)?;

// Rune — 20+ lines (context, sources, diagnostics, compile, vm call)
let mut context = rune_modules::default_context()?;
let runtime = Arc::new(context.runtime()?);
// ... plus sources, options, diagnostics, prepare, build, vm...

// Koto — 4 lines
let mut koto = Koto::default();
koto.compile_and_run(script)?;

// mlua — 3 lines
let lua = Lua::new();
lua.load(script).exec()?;
```

---

## Final Verdict

| Decision | Score (1-10) |
|---|---|
| ✅ **Duckscript** | 9/10 |
| ✅ **Rhai** (with custom API) | 7/10 |
| ❌ **Rune** | 5/10 |
| ❌ **Koto** | 6/10 |
| ❌ **mlua** | 5/10 |
| ❌ **Rust-only** | 4/10 |

**Duckscript** wins for this specific use case — a Make recipe language — because it was designed for it. The author of duckscript (sagiegurari) also authored cargo-make, so the language and the use case are tightly aligned. The only real drawbacks are the small community and lack of sandboxing.
