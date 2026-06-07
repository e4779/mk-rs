# uutils coreutils Research — Reusability in mk-rust

**Repo**: https://github.com/uutils/coreutils (MIT)
**Shallow clone**: `/tmp/uutils`
**Date**: 2026-06-07

## Recommendation Summary

**Don't depend on uutils crates directly.** uutils is a monorepo; each utility is a separate crate but `uucore` is tightly coupled to the coreutils ecosystem (fluent localization, clap, feature flags). Pulling it in would drag a massive dependency tree. Instead, **learn the patterns** and **standalone crates** listed below.

---

## 1. Lexer / Tokenizer Components

### What exists
- **`src/uu/env/src/split_iterator.rs`** — Custom shell-word splitter for GNU `env -S`. Based on the `shell_words` crate by Tomasz Miąsko, adapted for GNU env's dialect. Handles quotes (`'`, `"`), backslash escapes, `$VARIABLE` expansion, whitespace splitting.
- **`src/uu/env/src/string_parser.rs`** — Character-by-character parser over `OsStr` with UTF-8 awareness. Supports peek, consume by chunk, position tracking.
- **`src/uu/env/src/variable_parser.rs`** — Parses `${VAR}` and `${VAR:-default}` variable substitution.
- **`src/uu/env/src/string_expander.rs`** — Combines string_parser + variable_parser for full env string expansion.

### What's in `uucore`
- **`features/quoting_style/`** (uucore) — For *output* quoting (when displaying filenames in shell-safe form), not for parsing input. `ShellQuoter`, `CQuoter`, `LiteralQuoter`. Not useful for mkfile parsing.
- **`features/format/escape.rs`** (uucore) — C-style escape sequence parser (`\n`, `\t`, `\0NNN`, `\xHH`). Used by `printf` and `echo`. Good pattern for escape handling in strings.
- **`mods/line_ending.rs`** (uucore) — Line ending detection (LF, CRLF). Trivial.

### Reusable crates
| Crate | Version | Purpose |
|---|---|---|
| **`shell-words`** | 1.1.1 | Split command line per UNIX shell rules. Directly useful for mkfile rule commands. |
| **`shlex`** | 2.0.1 | Like Python's shlex — split into shell words. Lighter than shell-words. |
| **`shell-quote`** | 0.7.2 | Shell-quoting for output (not parsing). Not needed. |

**Verdict**: Use `shell-words` crate for mkfile recipe command tokenization. Write custom tokenizer for `attr: val` parsing in mk rules (trivial — split on `:` with trimming). The env split_iterator pattern is worth reading but not depending on.

---

## 2. Process Management

### What exists
- **`uucore::features::process`** (uucore, `process` feature, Unix-only) — `ChildExt` trait extending `std::process::Child`:
  - `send_signal(signal: usize)` — sends signal to child PID via `nix::sys::signal::kill`
  - `send_signal_group(signal: usize)` — sends to process group (temporarily ignores signal for self)
  - `wait_or_timeout(Duration, Option<&AtomicBool>)` — polls `try_wait()` with 100ms sleep, respects timeout and a signal flag. **Exactly the pattern mk needs for rule execution with timeout.**
  - Also provides `geteuid`, `getegid`, `getpid`, `getsid`, `getpgrp`.

- **`src/uu/timeout/`** — Full implementation building on `ChildExt::wait_or_timeout`. Shows:
  - `pre_exec` for child process setup (reset signal handlers, set process group, death signal)
  - Kill-after pattern: send TERM, wait, then SIGKILL
  - Signal handler installation for SIGALRM + SIGCHLD
  - Uses `rustix::process::set_parent_process_death_signal` (Linux) for reliable cleanup

- **`uucore::features::signals`** (uucore, `signals` feature) — Cross-platform signal name↔number mapping, `SIGPIPE` management, startup state capture (stdin/stdout/stderr closed state, SIGPIPE ignored state). The `ALL_SIGNALS` tables are platform-specific.

- **`uucore::features::pipes`** (uucore, Linux-only) — `splice()` zero-copy pipe helpers. Overkill for mk.

### Reusable crates
| Crate | Version | Purpose |
|---|---|---|
| **`rustix`** | (workspace, ~0.38) | Direct syscall wrappers. uutils already uses this. mk uses `rustix` in its Cargo.toml. |
| **`nix`** | (workspace, ~0.29) | Higher-level Unix API. uutils uses this extensively. |

**Verdict**: Write mk's own `wait_or_timeout` using `rustix` (already in deps). Pattern from uucore's `process.rs` is ~50 lines and trivially adaptable. No need to depend on uucore. The signal name mapping in `signals.rs` is useful but easy to write a minimal version.

---

## 3. Shell Argument / Expression Parsing

### What exists
- **`src/uu/expr/src/syntax_tree.rs`** — **Full Pratt parser** for `expr` expressions. ~700 lines. Handles:
  - Binary ops (numeric: +-*/%, relational: `< <= = != >= >`, string: `:` match, `index`, `and`, `or`)
  - Ternary-like `substr` with 3 children
  - Unary `length` and `+`
  - Precedence climbing (Pratt parsing)
  - Recursive tree-walking evaluator with explicit stack (avoids stack overflow)
  - Outputs `NumOrStr` (big integer or byte-string)
  - **Directly relevant**: mk needs to parse conditions (dependency expression) and attribute values. The Pratt parser structure maps well to mkfile's `expr -> term -> atom` grammar.

- **`src/uu/test/src/parser.rs`** — Custom tokenizer/parser for `test`/[ expressions (~350 lines). Recognizes:
  - Unary operators (`-z`, `-n`, `-f`, `-d`, etc.)
  - Binary operators (`=`, `!=`, `-eq`, `-nt`, etc.)
  - Boolean ops (`-a`, `-o`)
  - Parentheses and `!`
  - Expression re-parsing with precedence for ambiguous cases
  - **Useful pattern reference** for mk's condition parser (boolean operators, file tests).

- **`uucore::features::format/`** (uucore) — `printf`-style format string parser + escape sequence parser. Used by `printf`, `echo`, `dd`, `seq`. Pattern for `%`-format spec parsing.

### Verdict
The `expr` Pratt parser is the **most reusable pattern**. For mkfile rule parsing, study this file:
- `src/uu/expr/src/syntax_tree.rs` — token-based precedence-climbing parser
- Adapt for mk's grammar (targets, prerequisites, recipes) — don't depend on the crate.

---

## 4. File System / Metadata

### What exists
- **`uucore::features::fs`** (uucore, `fs` feature) — `FileInformation` struct wrapping `rustix::fs::Stat` with `file_size()`, `number_of_links()`, `inode()`, `PartialEq` (dev+ino comparison). Also:
  - `canonicalize()` — generalized path canonicalization with symlink resolution modes and missing-component handling (better than `std::fs::canonicalize`)
  - `normalize_path()` — removes `..` and `.` segments
  - `display_permissions()` — Unix permission string formatting
  - `are_hardlinks_to_same_file()`, `is_symlink_loop()`, `paths_refer_to_same_file()`
  - `sane_blksize` — safe block size extraction from metadata

- **`uucore::features::fsext`** (uucore, `fsext` feature) — Filesystem type detection, statfs wrapper, mount info reading. Used by `stat -f`, `df`.

- **`src/uu/stat/`** — Full `stat` implementation using `fsext` + `time`. Shows how to extract and format file timestamps (atime, mtime, ctime) with sub-second precision.

- **`uucore::features::time`** (uucore, `time` feature) — `format_system_time()` using `jiff` crate. Converts `SystemTime` to formatted string with `strftime`-like format. Supports large timestamps with fallback.

### Verdict
**`rustix::fs::stat`** (already in mk's deps) provides everything needed: `st_mtime`, `st_atime`, `st_ctime` (as `Timestamps` struct with `seconds` and `nanoseconds`). No need for uucore wrappers. The `canonicalize()` function is nice but `dunce` crate solves the common Windows case.

---

## 5. Environment Handling

### What exists
- **`src/uu/env/`** — Full GNU `env` implementation, ~2500 lines. Contains:
  - `split_iterator.rs` — shell-word splitter for `-S` option
  - `variable_parser.rs` — `${VAR:-default}` expansion
  - `string_expander.rs` — full string expansion pipeline
  - `native_int_str.rs` — OsStr abstraction for cross-platform char access
  - Signal management (`--ignore-signal`, `--default-signal`, `--block-signal`)
  - `-C` chdir support, `-0` null-terminated output, `--argv0`

- **`uucore`** — No dedicated env module. Basic `std::env` usage throughout.

### Verdict
For mk-rust, `std::env::{var, set_var, remove_var, vars}` is sufficient. The `shell-words` crate handles the only non-trivial env interaction (parsing recipe command lines that may contain variable expansions). The env variable parser pattern is worth reading if mk needs `${VAR:-default}` in makefiles, but that's a future concern.

---

## 6. Parallel Execution

### What exists
- **`src/uu/sort/`** — Uses **rayon** (`par_sort_unstable_by`) for in-memory parallel sort (~10-40% speedup on multi-core).
  - `Cargo.toml` deps: `rayon = { workspace = true }`
  - Thread pool configured in `sort.rs`:
    ```rust
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(settings.threads)
        .build_global()
        .unwrap();
    ```
  - Fallback to sequential sort if thread pool setup fails.

- **`src/uu/sort/src/ext_sort/threaded.rs`** — Two-thread pipeline for external sort (one thread reads/writes temp files, another sorts). Uses `std::sync::mpsc` channels (`SyncSender`/`Receiver`) for thread communication.
  - Pattern: I/O thread feeds chunks to sort thread via channel; sort thread sends sorted chunks back.
  - **Directly relevant**: mk could use similar pattern for parallel rule execution with bounded concurrency.

- **`src/uu/timeout/src/timeout.rs`** — `AtomicBool` signal flag for cross-thread communication.

### Verdict
mk-rust already plans to use `rayon` for parallel job execution. The `sort` example confirms the pattern. For running shell commands in parallel (mk recipes), use `std::process::Command` + `rayon::scope` or a simple thread pool with `mpsc` channels. The `ext_sort/threaded.rs` pattern is instructive for bounding concurrent recipe execution.

---

## 7. External Crate Search Results

### Shell word parsing
| Crate | Description | For mk-rust |
|---|---|---|
| **`shell-words`** 1.1.1 | Split command line per UNIX shell rules. 10M+ downloads. **Recommended.** | Parse recipe command into `Vec<String>` for `Command::new()` |
| **`shlex`** 2.0.1 | Lighter alternative. Like Python `shlex`. | Lighter dep, but fewer features |

### Glob matching
| Crate | Description | For mk-rust |
|---|---|---|
| **`glob`** 0.3.3 | Std glob matching. Heavy (regex-based). | Currently used by uucore parser-glob feature |
| **`glob-match`** 0.2.1 | Fast glob matcher, no regex. **Recommended** for simple `*.c` patterns. Lighter. | `wax` 0.7.0: Opinionated globs with extended syntax |
| **`wax`** 0.7.0 | Modern glob with compile-time validation, extended syntax (**/**, `{a,b}`) | Best for complex patterns |

**For mk**: `glob-match` is best — mk targets are simple globs (`*.c`, `*.o`), no need for regex.

### mkfile parsers
| Crate | Description |
|---|---|
| **None found** | No crate parses Plan 9 mkfile syntax. Must write custom parser. |

---

## 8. Specific Learning Points

### Pattern: wait_or_timeout (for mk rule execution)
**File**: `src/uucore/src/lib/features/process.rs` (lines 79-105)
```rust
fn wait_or_timeout(
    &mut self,
    timeout: Duration,
    signaled: Option<&AtomicBool>,
) -> io::Result<Option<ExitStatus>> {
    if timeout == Duration::from_micros(0) {
        return self.wait().map(Some);
    }
    drop(self.stdin.take());  // Close stdin to avoid hangs
    let start = Instant::now();
    loop {
        if let Some(status) = self.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout
            || signaled.is_some_and(|s| s.load(atomic::Ordering::Relaxed))
        {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(None)
}
```
**Adapt for mk**: Replace `timeout` check with "no more parallel slots available" check; the signal flag pattern is useful for "cancel all running jobs".

### Pattern: Pratt parser (for mk condition parsing)
**File**: `src/uu/expr/src/syntax_tree.rs` (lines 804-850)
Uses precedence climbing:
1. `parse_expression()` → `parse_precedence(0)`
2. `parse_precedence(p)` → while next op has higher precedence, advance
3. `parse_simple_expression()` → literal, paren, unary

Directly applicable to mk condition parsing (`$file.c` → atoms, `|` → or, etc.)

### Pattern: Signal handling during subprocess execution
**File**: `src/uu/timeout/src/timeout.rs` (lines 181-230)
- Install SIGCHLD handler to ensure `wait()` works even if parent ignored SIGCHLD
- In `pre_exec`: reset terminal signals to default, preserve SIGPIPE state
- Uses `AtomicBool` flag (`SIGNALED`) to communicate signal receipt across threads

---

## 9. What NOT to depend on from uutils

| Component | Reason |
|---|---|
| **`uucore` crate** | Tightly coupled to coreutils: flüent i18n, clap, feature flags, workspace types. Heavy dependency. |
| **`features/signals.rs`** | Signal tables are 300+ lines of platform-specific constants. Write `Signal::try_from(n)` with `rustix` directly. |
| **`features/fs/FileInformation`** | Thin wrapper over `rustix::fs::Stat`. Use `rustix` directly. |
| **`features/process`** | Only `ChildExt` is useful, and it's ~50 lines. Inline it. |
| **`features/format/`** | printf formatting is overkill. mk doesn't need format spec parsing. |
| **`features/quoting_style/`** | Output quoting only. mk needs input parsing (shell-words). |

## 10. Summary: What mk-rust should use

### Add as dependencies
1. **`shell-words`** 1.1.1 — Recipe command parsing
2. **`glob-match`** 0.2.1 — Target wildcard matching (instead of glob crate)

### Study / adapt patterns (not depend on)
1. **Pratt parser** from `src/uu/expr/src/syntax_tree.rs` — for mk condition parsing
2. **`wait_or_timeout`** from `features/process.rs` — for bounded recipe execution
3. **Ext sort threaded pipeline** from `src/uu/sort/src/ext_sort/threaded.rs` — for parallel job executor

### Already handled by existing deps
- `rustix` — file metadata, process spawning, signals
- `std::process::Command` — recipe execution (with `shell-words` for argument parsing)
- `std::env` — environment variable management

### Utilities to reference for edge cases
- `src/uu/test/src/parser.rs` — if mk needs `test`-like condition evaluation in rules
- `src/uu/env/src/split_iterator.rs` — if mk needs variable expansion in recipe strings
