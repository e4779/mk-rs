//! Recipe execution glue: elision, printing, shell dispatch.
//!
//! Takes a parsed Recipe and executes it through the Shell trait,
//! respecting attributes and CLI flags (-n, -e, -t, -s).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::attr::Attributes;
use crate::error::{RecipeError, ShellError};
use crate::shell::{Shell, ShellResult};

// ── Recipe ─────────────────────────────────────────────────────────────────

/// A recipe ready to execute.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// Target being built.
    pub target: String,
    /// All prerequisites.
    pub prereqs: Vec<String>,
    /// The recipe script text (raw, before first-char elision).
    pub script: String,
    /// Working directory.
    pub working_dir: PathBuf,
    /// Environment variables to pass to the shell.
    pub env: HashMap<String, String>,
    /// Rule attributes (affect execution behavior).
    pub attributes: Attributes,
    /// Stem from metarule match (None for concrete rules).
    pub stem: Option<String>,
    /// All targets of the rule (for $alltarget variable).
    pub all_targets: Vec<String>,
}

// ── Options ────────────────────────────────────────────────────────────────

/// Options controlling recipe execution behavior.
///
/// These correspond to CLI flags: -n (no-exec), -e (explain),
/// -t (touch), -q (quiet).
#[derive(Debug, Clone, Default)]
pub struct RecipeOptions {
    /// -n flag: print recipes but don't execute.
    pub no_exec: bool,
    /// -e flag: explain why recipe runs.
    pub explain: bool,
    /// -t flag: touch targets instead of running recipes.
    pub touch: bool,
    /// -q flag: quiet — don't print recipes (like Q attribute).
    pub silent: bool,
    /// Whether to use ANSI color in recipe output.
    pub color: bool,
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Apply first-char elision to each line of a recipe.
///
/// In mk, the first character of every recipe line is stripped —
/// it's the indent marker (tab or space) that mkfile syntax requires
/// to distinguish recipe lines from rule headers.
///
/// Blank lines are preserved as-is.
///
/// # Examples
///
/// ```
/// let elided = mk_rs_core::recipe::elide_first_char("\techo hello\n\techo world");
/// assert_eq!(elided, "echo hello\necho world");
/// ```
pub fn elide_first_char(recipe: &str) -> String {
    let mut out = String::with_capacity(recipe.len());
    let mut first = true;
    for line in recipe.split('\n') {
        if first {
            first = false;
        } else {
            out.push('\n');
        }
        if line.is_empty() {
            continue;
        }
        // Strip exactly one character (the indent marker).
        let mut chars = line.chars();
        chars.next(); // skip first char
        out.push_str(chars.as_str());
    }
    out
}

/// Execute a single recipe through the configured shell.
///
/// # Algorithm (Phase 1a — serial, first-char elision, Q attribute)
///
/// 1. Apply first-char elision to the recipe script.
/// 2. If not quiet (not -s and not Q attribute), print the recipe.
/// 3. If `-n` (no-exec): print recipe and return fake success.
/// 4. If `-e` (explain): print staleness reason, then continue.
/// 5. If `-t` (touch): touch the target file, return fake success.
/// 6. Execute the elided recipe through the shell.
/// 7. If exit code ≠ 0, check D attribute (delete target on error).
///
/// Returns the shell result on success, or a `RecipeError` on failure.
pub fn run(
    recipe: &Recipe,
    shell: &dyn Shell,
    opts: &RecipeOptions,
) -> Result<ShellResult, RecipeError> {
    // Parser already strips indent — recipe text is already elided.
    let script = recipe.script.clone();

    // ── Quiet check ────────────────────────────────────────────────────
    let quiet = opts.silent || recipe.attributes.is_quiet();

    if !quiet {
        if opts.color {
            // ANSI: bold target, dim recipe lines
            eprintln!("\x1b[1m{}:\x1b[0m", recipe.target);
            for line in script.lines() {
                eprintln!("\x1b[2m\t{line}\x1b[0m");
            }
        } else {
            eprintln!("{}:", recipe.target);
            for line in script.lines() {
                eprintln!("\t{line}");
            }
        }
    }

    // ── -n: no-exec ────────────────────────────────────────────────────
    if opts.no_exec {
        return Ok(ShellResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        });
    }

    // ── -e: explain ────────────────────────────────────────────────────
    if opts.explain {
        eprintln!(
            "  target '{}' is out of date because: prerequisites are newer",
            recipe.target
        );
    }

    // ── -t: touch ──────────────────────────────────────────────────────
    if opts.touch {
        touch_target(&recipe.target)?;
        return Ok(ShellResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        });
    }

    // ── Execute ────────────────────────────────────────────────────────
    // Inject recipe-time variables into the environment.
    let mut env = recipe.env.clone();
    env.insert("target".to_string(), recipe.target.clone());
    env.insert("prereq".to_string(), recipe.prereqs.join(" "));
    env.insert("newprereq".to_string(), recipe.prereqs.join(" "));
    env.insert("pid".to_string(), std::process::id().to_string());
    env.insert("alltarget".to_string(), recipe.all_targets.join(" "));
    env.insert("newmember".to_string(), recipe.prereqs.join(" ")); // same as prereqs for now
    if let Some(ref stem) = recipe.stem {
        env.insert("stem".to_string(), stem.clone());
    }

    let result = shell
        .execute(&script, &env, &recipe.working_dir)
        .map_err(|e| match e {
            ShellError::CommandFailed { code, .. } => RecipeError::CommandFailed { code },
            ShellError::ShellNotFound { .. } => RecipeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                e.to_string(),
            )),
            ShellError::Io(io) => RecipeError::Io(io),
        })?;

    // ── Check exit code ────────────────────────────────────────────────
    if result.exit_code != 0 {
        // D attribute: delete target file on error.
        // Phase 1b will fully implement this; Phase 1a notes it.
        if recipe.attributes.is_delete_on_error() {
            let target_path = Path::new(&recipe.target);
            if target_path.exists() {
                std::fs::remove_file(target_path).map_err(RecipeError::Io)?;
                return Err(RecipeError::TargetDeleted {
                    target: recipe.target.clone(),
                });
            }
        }
        return Err(RecipeError::CommandFailed {
            code: result.exit_code,
        });
    }

    Ok(result)
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Ensure the target exists on disk (for -t flag).
///
/// If the file doesn't exist, creates an empty file.
/// If it does exist, leaves its mtime unchanged (mtime update requires
/// the `filetime` crate, not yet available in Phase 1a).
fn touch_target(target: &str) -> Result<(), RecipeError> {
    let path = Path::new(target);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(RecipeError::Io)?;
            }
        }
        std::fs::write(path, "").map_err(RecipeError::Io)?;
    } else {
        // Update modification time by rewriting the file with its own content
        let content = std::fs::read(path).map_err(RecipeError::Io)?;
        std::fs::write(path, content).map_err(RecipeError::Io)?;
    }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── Mock shell ─────────────────────────────────────────────────────

    struct MockShell {
        exit_code: i32,
        stdout: String,
        stderr: String,
        last_env: std::sync::Mutex<HashMap<String, String>>,
    }

    impl Shell for MockShell {
        fn name(&self) -> &str {
            "mock"
        }

        fn execute(
            &self,
            _recipe: &str,
            env: &HashMap<String, String>,
            _dir: &Path,
        ) -> Result<ShellResult, ShellError> {
            *self.last_env.lock().unwrap() = env.clone();
            Ok(ShellResult {
                exit_code: self.exit_code,
                stdout: self.stdout.clone(),
                stderr: self.stderr.clone(),
            })
        }

        fn find_unescaped(&self, _input: &str, _ch: char) -> Vec<usize> {
            vec![]
        }

        fn quote(&self, token: &str) -> String {
            token.to_string()
        }
    }

    fn make_recipe() -> Recipe {
        Recipe {
            target: "hello".into(),
            prereqs: vec!["hello.c".into()],
            script: "\tcc -o hello hello.c\n".into(),
            working_dir: PathBuf::from("."),
            env: HashMap::new(),
            attributes: Attributes::default(),
            stem: None,
            all_targets: vec!["hello".into()],
        }
    }

    // ── Elision tests ──────────────────────────────────────────────────

    #[test]
    fn elide_single_line_tab() {
        assert_eq!(elide_first_char("\techo hello"), "echo hello");
    }

    #[test]
    fn elide_single_line_space() {
        assert_eq!(elide_first_char(" echo hello"), "echo hello");
    }

    #[test]
    fn elide_multi_line() {
        let input = "\techo one\n\techo two";
        assert_eq!(elide_first_char(input), "echo one\necho two");
    }

    #[test]
    fn elide_preserves_blank_lines() {
        let input = "\techo one\n\n\techo two";
        assert_eq!(elide_first_char(input), "echo one\n\necho two");
    }

    #[test]
    fn elide_spaces_indent() {
        assert_eq!(elide_first_char("  echo hello"), " echo hello");
    }

    #[test]
    fn elide_empty_string() {
        assert_eq!(elide_first_char(""), "");
    }

    #[test]
    fn elide_single_char_lines() {
        // Single tab per line → after elision: empty strings
        assert_eq!(elide_first_char("\t\n\t"), "\n");
    }

    #[test]
    fn elide_only_blank_lines() {
        // Blank lines (no indent marker) → preserved as-is
        assert_eq!(elide_first_char("\n\n"), "\n\n");
    }

    // ── run() success tests ────────────────────────────────────────────

    #[test]
    fn run_success() {
        let shell = MockShell {
            exit_code: 0,
            stdout: "ok\n".into(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        let result = run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ok\n");
    }

    // ── run() failure tests ────────────────────────────────────────────

    #[test]
    fn run_command_failure() {
        let shell = MockShell {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error".into(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        let result = run(&recipe, &shell, &RecipeOptions::default());
        assert!(result.is_err());
        match result.unwrap_err() {
            RecipeError::CommandFailed { code } => assert_eq!(code, 1),
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    // ── -n (no-exec) tests ─────────────────────────────────────────────

    #[test]
    fn run_no_exec() {
        let shell = MockShell {
            exit_code: 0,
            stdout: "should not see this".into(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        let opts = RecipeOptions {
            no_exec: true,
            ..Default::default()
        };
        let result = run(&recipe, &shell, &opts).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    // ── -t (touch) tests ───────────────────────────────────────────────

    #[test]
    fn run_touch_creates_file() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        // Use a temp-like target that won't conflict.
        recipe.target = "/tmp/mk-test-touch-target".into();

        // Ensure it doesn't exist before.
        let _ = std::fs::remove_file(&recipe.target);

        let opts = RecipeOptions {
            touch: true,
            ..Default::default()
        };
        let result = run(&recipe, &shell, &opts).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(Path::new(&recipe.target).exists());

        // Cleanup.
        let _ = std::fs::remove_file(&recipe.target);
    }

    #[test]
    fn run_touch_existing_file() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.target = "/tmp/mk-test-touch-existing".into();

        // Create the file first.
        std::fs::write(&recipe.target, "existing content").unwrap();

        let opts = RecipeOptions {
            touch: true,
            ..Default::default()
        };
        let result = run(&recipe, &shell, &opts).unwrap();
        assert_eq!(result.exit_code, 0);
        // File should still exist with original content.
        let content = std::fs::read_to_string(&recipe.target).unwrap();
        assert_eq!(content, "existing content");

        // Cleanup.
        let _ = std::fs::remove_file(&recipe.target);
    }

    // ── Q (quiet) attribute test ───────────────────────────────────────

    #[test]
    fn run_quiet_attribute_does_not_panic() {
        let shell = MockShell {
            exit_code: 0,
            stdout: "quiet-output".into(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.attributes = Attributes::default().with(Attributes::QUIET);
        let result = run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "quiet-output");
    }

    // ── -s (silent) flag test ──────────────────────────────────────────

    #[test]
    fn run_silent_flag() {
        let shell = MockShell {
            exit_code: 0,
            stdout: "silent-output".into(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        let opts = RecipeOptions {
            silent: true,
            ..Default::default()
        };
        let result = run(&recipe, &shell, &opts).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "silent-output");
    }

    // ── -e (explain) test ──────────────────────────────────────────────

    #[test]
    fn run_explain_flag() {
        let shell = MockShell {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        let opts = RecipeOptions {
            explain: true,
            ..Default::default()
        };
        let result = run(&recipe, &shell, &opts).unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ── D (delete on error) attribute test ─────────────────────────────

    #[test]
    fn run_delete_on_error_attribute() {
        let shell = MockShell {
            exit_code: 1,
            stdout: String::new(),
            stderr: "fail".into(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.target = "/tmp/mk-test-delete-target".into();
        recipe.attributes = Attributes::default().with(Attributes::DELETE_ON_ERROR);

        // Create the target file first.
        std::fs::write(&recipe.target, "should be deleted").unwrap();

        let result = run(&recipe, &shell, &RecipeOptions::default());
        assert!(result.is_err());
        match result.unwrap_err() {
            RecipeError::TargetDeleted { target } => assert_eq!(target, recipe.target),
            other => panic!("expected TargetDeleted, got {other:?}"),
        }
        // File should be gone.
        assert!(!Path::new(&recipe.target).exists());
    }

    #[test]
    fn run_delete_on_error_no_file() {
        // D attribute when target file doesn't exist: should still error with
        // CommandFailed, not TargetDeleted.
        let shell = MockShell {
            exit_code: 1,
            stdout: String::new(),
            stderr: "fail".into(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.target = "/tmp/mk-test-delete-nonexistent".into();
        recipe.attributes = Attributes::default().with(Attributes::DELETE_ON_ERROR);

        // Ensure the file doesn't exist.
        let _ = std::fs::remove_file(&recipe.target);

        let result = run(&recipe, &shell, &RecipeOptions::default());
        assert!(result.is_err());
        match result.unwrap_err() {
            RecipeError::CommandFailed { code } => assert_eq!(code, 1),
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    // ── Defaults tests ─────────────────────────────────────────────────

    #[test]
    fn recipe_options_default_all_false() {
        let opts = RecipeOptions::default();
        assert!(!opts.no_exec);
        assert!(!opts.explain);
        assert!(!opts.touch);
        assert!(!opts.silent);
    }

    #[test]
    fn shell_result_eq() {
        let a = ShellResult {
            exit_code: 0,
            stdout: "hi".into(),
            stderr: String::new(),
        };
        let b = ShellResult {
            exit_code: 0,
            stdout: "hi".into(),
            stderr: String::new(),
        };
        assert_eq!(a, b);
    }

    // ── Recipe-time variables test ─────────────────────────────────────

    #[test]
    fn run_injects_target_prereq_pid() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        let env = shell.last_env.lock().unwrap();
        assert_eq!(env.get("target").map(|s| s.as_str()), Some("hello"));
        assert_eq!(
            env.get("prereq").map(|s| s.as_str()),
            Some("hello.c")
        );
        assert!(
            env.get("pid")
                .map(|s| s.parse::<u32>().is_ok())
                .unwrap_or(false),
            "pid should be a valid integer"
        );
    }

    #[test]
    fn run_injects_stem_for_metarule() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.stem = Some("hello".into());
        run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        let env = shell.last_env.lock().unwrap();
        assert_eq!(env.get("stem").map(|s| s.as_str()), Some("hello"));
    }

    #[test]
    fn run_injects_newprereq() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        let env = shell.last_env.lock().unwrap();
        assert!(env.contains_key("newprereq"));
        assert_eq!(
            env.get("newprereq").map(|s| s.as_str()),
            Some("hello.c")
        );
    }

    #[test]
    fn run_injects_alltarget() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let mut recipe = make_recipe();
        recipe.all_targets = vec!["hello".into(), "hello_debug".into()];
        run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        let env = shell.last_env.lock().unwrap();
        assert_eq!(
            env.get("alltarget").map(|s| s.as_str()),
            Some("hello hello_debug")
        );
    }

    #[test]
    fn run_injects_alltarget_single() {
        let shell = MockShell {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            last_env: std::sync::Mutex::new(HashMap::new()),
        };
        let recipe = make_recipe();
        run(&recipe, &shell, &RecipeOptions::default()).unwrap();
        let env = shell.last_env.lock().unwrap();
        assert_eq!(
            env.get("alltarget").map(|s| s.as_str()),
            Some("hello")
        );
    }
}
