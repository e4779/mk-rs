# duckscript in mk-rs

duckscript is an optional embedded recipe language for mk-rs. It runs recipes
**in-process** — no subprocess fork, no `/bin/sh` dependency. It's the same
scripting engine that powers [cargo-make](https://github.com/sagiegurari/cargo-make).

duckscript is feature-gated behind `--features duckscript`. If you don't enable
it, mk-rs uses `/bin/sh` (the default) and you pay nothing for the unused code.

## Enabling duckscript

### 1. Build with the feature flag

```bash
cargo install mk-rs --features duckscript
```

### 2. Set MKSHELL in your mkfile

Add this line to any mkfile:

```makefile
MKSHELL=duckscript
```

All recipes in that mkfile will now run through the embedded duckscript runtime.
Or set it in your environment to apply globally:

```bash
export MKSHELL=duckscript
```

### 3. Check it works

```bash
echo 'MKSHELL=duckscript' > mkfile
echo 'hello:VQ:
	echo hello from duckscript' >> mkfile
mk hello
# → hello from duckscript
```

## Anatomy of a duckscript recipe

```makefile
# mkfile
MKSHELL=duckscript

build:
    echo Building project...
    exec --fail-on-error cc -o hello hello.c
    echo Done.

clean:V:
    rm hello hello.o
```

Each recipe line is a **duckscript statement** — a command name followed by
arguments. Lines starting with `#` are comments. Indentation is significant:
mk strips the first whitespace character from each recipe line (same as sh
recipes).

## Built-in commands (duckscriptsdk)

duckscript ships with ~100 SDK commands. Below are the most useful ones for
build recipes. All commands are loaded automatically when `MKSHELL=duckscript`.

### Process execution

| Command | Description |
|---------|-------------|
| `exec` | Run a command, wait for it to finish. Use `--fail-on-error` to abort on non-zero exit. |
| `spawn` | Run a command in the background (non-blocking). |
| `exit` | Exit the script with an optional status code. |
| `watchdog` | Set a timeout; kill the process if it expires. |

```makefile
compile:
    exec --fail-on-error gcc -O2 -c src/main.c -o build/main.o
    exec --fail-on-error gcc build/main.o -o build/program
```

### File system

| Command | Description |
|---------|-------------|
| `cp` | Copy a file or directory. |
| `mv` | Move/rename a file. |
| `rm` | Remove a file. |
| `mkdir` | Create a directory (and parents with `-p`). |
| `exists` | Check if a path exists. Returns `true`/`false`. |
| `touch` | Create an empty file or update its mtime. |
| `glob_array` | Expand a glob pattern into an array variable. |
| `is_path_newer` | Check if `path_a` is newer than `path_b`. |
| `read_text` / `write_text` | Read/write file contents to/from a variable. |
| `append` | Append text to a file. |
| `basename` / `dirname` | Extract the file name or directory from a path. |
| `join_path` | Join path segments with the platform separator. |
| `canonical` | Resolve a path to its canonical absolute form. |
| `rmdir` | Remove an empty directory. |
| `list` | List directory contents into an array. |

```makefile
prepare:
    mkdir -p build/objects
    cp src/config.h build/config.h
    # Glob all .c files into an array
    glob_array "./src/*.c" sources
    echo Found files: ${sources}
```

### Environment

| Command | Description |
|---------|-------------|
| `set_env` | Set an environment variable. |
| `get_env` | Read an environment variable into a duckscript variable. |
| `set_current_directory` | Change the working directory (`cd` equivalent). |
| `which` | Locate a command on `PATH`. |

```makefile
debug_build:
    set_env CFLAGS "-g -O0"
    exec --fail-on-error cc ${CFLAGS} -c main.c
```

### Variables and strings

| Command | Description |
|---------|-------------|
| `set` | Assign a value to a variable. |
| `echo` | Print text to stdout. |
| `concat` | Join strings with a separator. |
| `split` | Split a string into an array. |
| `replace` | Replace substring occurrences. |
| `trim` / `trim_start` / `trim_end` | Strip whitespace. |
| `uppercase` / `lowercase` | Change case. |
| `ends_with` / `starts_with` / `contains` | String predicates. |
| `length` | Get string length. |

```makefile
gen_version:
    get_env GIT_HASH git_hash
    trim git_hash
    set version "1.0-${git_hash}"
    echo Building version ${version}
```

### Flow control

| Command | Description |
|---------|-------------|
| `exit_on_error` | If set to `true`, any command failure stops the script immediately. |
| `on_error` | Specify a label to jump to on error. |
| `if` / `else` / `elseif` / `end` | Conditional branching. |
| `equals` / `less_than` / `greater_than` | Comparison predicates. |
| `not` | Boolean negation. |
| `is_empty` | Check if a variable is empty. |

```makefile
conditional_build:
    exists build/output
    if ${out_value}
        echo Output exists, building incrementally...
        exec --fail-on-error cc -c main.c
    else
        echo Fresh build...
        mkdir build/output
        exec --fail-on-error cc -o build/output/prog main.c
    end
```

### Math, JSON, hashing

| Command | Description |
|---------|-------------|
| `calc` | Evaluate a math expression (e.g. `1 + 2 * 3`). |
| `hex_encode` / `hex_decode` | Hex conversion. |
| `sha256sum` / `sha512sum` | Compute file hashes. |
| `json_parse` / `json_encode` | Parse/emit JSON. |

## When to use duckscript vs sh

| Scenario | Use |
|----------|-----|
| Copying files, creating directories, moving output | **duckscript** — `cp`, `mkdir`, `mv` are faster in-process |
| Simple C/Go/Rust compilation recipes | **duckscript** — `exec` covers it, no shell needed |
| Cross-platform mkfiles (Windows, Linux, macOS) | **duckscript** — no `/bin/sh` dependency |
| Builds where fork overhead matters (thousands of recipes) | **duckscript** — zero subprocess for file ops |
| File existence checks and conditional logic | **duckscript** — `exists` + `if` is cleaner than `test -f` |
| Complex shell pipelines (`\|`, `>`, `<`) | **sh** — duckscript has no piping |
| Calling random system tools with complex flags | **sh** — `exec` works but quoting gets unwieldy |
| Shell scripts you already have written | **sh** — just drop them in, no rewrites |
| Background jobs, job control | **sh** — duckscript `spawn` is basic |
| Globbing in command arguments (`*.c`) | **sh** — duckscript globs only via `glob_array` |

**Rule of thumb:** Use duckscript when the recipe is mostly file manipulation
and simple `exec` calls. Use sh when you need piping, redirection, or complex
shell idioms.

## Limitations

duckscript is **not a shell**. It's a simple scripting language with SDK
commands. The trade-offs:

- **No piping.** You can't chain commands with `|`. Use intermediate files.
- **No shell glob expansion.** `rm *.o` won't work. Use `glob_array "*.o"` then
  loop, or pipe from `list`.
- **No input/output redirection.** `exec cc -o prog > build.log` does not
  redirect. Use `exec`'s built-in stdout capture (`--stdout-to-var`) instead.
- **No background jobs, no job control.** `spawn` starts a process and forgets
  about it — you can't `wait` or `fg`.
- **No heredocs, no subshells.** No `$(command)` or `` `command` `` inside
  recipe lines. Variable substitution happens via duckscript's `${var}`.
- **Single-line commands only.** Each recipe line is one duckscript command.
  Multi-line scripts use the existing mk recipe structure (each line is a
  statement).
- **Error handling is opt-in.** By default, duckscript continues on error. Use
  `exit_on_error true` at the top of your recipe if you want sh-style
  fail-fast behavior.

## Differences from sh recipes

When `MKSHELL=duckscript`, mk-rs still handles variable expansion the same way:
`$CFLAGS`, `${stem}`, `$target`, `$prereq` — these are expanded by mk-rs before
the recipe is passed to the duckscript runtime. The runtime additionally
expands `${var}` references from its own context (duckscript variables and
inherited environment variables).

```makefile
MKSHELL=duckscript
CC=cc
CFLAGS=-O2

%.o: %.c
    exec --fail-on-error ${CC} ${CFLAGS} -c $stem.c -o $target
```

Here `$CC`, `$CFLAGS`, `$stem`, `$target` are all mk-rs variables expanded
before duckscript sees the text.

## Further reading

- [duckscript language reference](https://github.com/sagiegurari/duckscript)
- [duckscriptsdk command reference](https://docs.rs/duckscriptsdk)
- [cargo-make](https://github.com/sagiegurari/cargo-make) — the largest duckscript user
