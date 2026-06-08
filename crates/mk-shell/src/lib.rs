use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use mk_rs_core::shell::{Shell, ShellResult, ShellError};

/// POSIX /bin/sh shell implementation.
#[derive(Debug, Clone)]
pub struct ShShell;

impl Shell for ShShell {
    fn name(&self) -> &str {
        "sh"
    }

    fn execute(
        &self,
        recipe: &str,
        env: &HashMap<String, String>,
        dir: &Path,
    ) -> Result<ShellResult, ShellError> {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-e")           // exit on first error
           .arg("-c")           // read command from argument
           .arg(recipe)
           .current_dir(dir);

        // Clear and set environment
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        if !env.contains_key("PATH") {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }

        let status = cmd.status()?;

        Ok(ShellResult {
            exit_code: status.code().unwrap_or(-1),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn find_unescaped(&self, input: &str, ch: char) -> Vec<usize> {
        let mut positions = Vec::new();
        let bytes = input.as_bytes();
        let mut in_single = false;
        let mut in_double = false;
        let mut i = 0;

        while i < bytes.len() {
            match bytes[i] {
                b'\\' if !in_single => {
                    // Backslash escapes next char in sh
                    i += 2; // skip both
                    continue;
                }
                b'\'' if !in_double => {
                    in_single = !in_single;
                }
                b'"' if !in_single => {
                    in_double = !in_double;
                }
                c if c == ch as u8 && !in_single && !in_double => {
                    positions.push(i);
                }
                _ => {}
            }
            i += 1;
        }
        positions
    }

    fn quote(&self, token: &str) -> String {
        // sh quoting: wrap in single quotes, escape embedded single quotes as '\''
        if token.is_empty() {
            return "''".to_string();
        }
        if !token.contains('\'') {
            return format!("'{}'", token);
        }
        // Contains single quotes: break out of quoting, insert escaped quote
        let escaped = token.replace('\'', "'\\''");
        format!("'{}'", escaped)
    }
}

// ── Custom shell (MKSHELL) ─────────────────────────────────────────────────

/// Custom shell that uses the command from $MKSHELL.
/// E.g., MKSHELL=/bin/bash → runs /bin/bash -ec <recipe>
#[derive(Debug, Clone)]
pub struct CustomShell {
    cmd: String,
}

impl CustomShell {
    pub fn new(cmd: &str) -> Self {
        Self { cmd: cmd.to_string() }
    }
}

impl Shell for CustomShell {
    fn name(&self) -> &str { &self.cmd }

    fn execute(
        &self,
        recipe: &str,
        env: &HashMap<String, String>,
        dir: &Path,
    ) -> Result<ShellResult, ShellError> {
        let mut cmd = Command::new(&self.cmd);
        cmd.arg("-ec")
           .arg(recipe)
           .current_dir(dir);
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        if !env.contains_key("PATH") {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        let status = cmd.status()?;
        Ok(ShellResult {
            exit_code: status.code().unwrap_or(-1),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn find_unescaped(&self, input: &str, ch: char) -> Vec<usize> {
        ShShell.find_unescaped(input, ch) // same quoting as sh
    }

    fn quote(&self, token: &str) -> String {
        ShShell.quote(token)
    }
}

#[cfg(test)]
mod custom_shell_tests {
    use super::*;

    #[test]
    fn custom_shell_bash() {
        let shell = CustomShell::new("/bin/bash");
        assert_eq!(shell.name(), "/bin/bash");
        let result = shell.execute("echo hello", &HashMap::new(), Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 0);
    }
}

// ── duckscript shell ────────────────────────────────────────────────────────

/// duckscript embedded shell implementation.
#[cfg(feature = "duckscript")]
#[derive(Debug, Clone)]
pub struct DuckShell;

#[cfg(feature = "duckscript")]
impl Shell for DuckShell {
    fn name(&self) -> &str {
        "duckscript"
    }

    fn execute(
        &self,
        recipe: &str,
        env: &HashMap<String, String>,
        dir: &Path,
    ) -> Result<ShellResult, ShellError> {
        let mut context = duckscript::types::runtime::Context::new();
        // Load all env vars into duckscript context
        for (k, v) in env {
            context.variables.insert(k.clone(), v.clone());
        }
        // Load SDK commands (exec, cp, mv, mkdir, etc.)
        duckscriptsdk::load(&mut context.commands)
            .map_err(|e| ShellError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        // Set working directory
        std::env::set_current_dir(dir)
            .map_err(ShellError::Io)?;

        // Run script
        duckscript::runner::run_script(recipe, context, None)
            .map_err(|e| ShellError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        Ok(ShellResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn find_unescaped(&self, input: &str, ch: char) -> Vec<usize> {
        // duckscript doesn't have shell quoting — simple scan
        input.match_indices(ch).map(|(i, _)| i).collect()
    }

    fn quote(&self, token: &str) -> String {
        token.to_string() // duckscript doesn't need shell quoting
    }
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_shell_name() {
        assert_eq!(ShShell.name(), "sh");
    }

    #[test]
    fn execute_echo() {
        let shell = ShShell;
        let env = HashMap::new();
        let result = shell.execute("echo hello", &env, Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn execute_error() {
        let shell = ShShell;
        let env = HashMap::new();
        let result = shell.execute("exit 1", &env, Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn execute_with_env() {
        let shell = ShShell;
        let mut env = HashMap::new();
        env.insert("MYVAR".into(), "myval".into());
        // Recipe output goes to terminal (stdout inherited), not captured
        let result = shell.execute("echo $MYVAR", &env, Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn find_unescaped_equal() {
        let shell = ShShell;
        // "CC=gcc" → '=' at position 2
        let pos = shell.find_unescaped("CC=gcc", '=');
        assert_eq!(pos, vec![2]);
    }

    #[test]
    fn find_unescaped_ignores_quoted() {
        let shell = ShShell;
        // "foo '=' bar" → the '=' inside quotes is ignored
        let pos = shell.find_unescaped("foo '=' bar", '=');
        assert!(pos.is_empty());
    }

    #[test]
    fn find_unescaped_ignores_escaped() {
        let shell = ShShell;
        // "foo \\= bar" → escaped = is ignored
        let pos = shell.find_unescaped("foo \\= bar", '=');
        assert!(pos.is_empty());
    }

    #[test]
    fn quote_simple() {
        let shell = ShShell;
        assert_eq!(shell.quote("hello"), "'hello'");
    }

    #[test]
    fn quote_empty() {
        assert_eq!(ShShell.quote(""), "''");
    }

    #[test]
    fn quote_with_single_quote() {
        let shell = ShShell;
        assert_eq!(shell.quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn execute_stdout_inherited_not_captured() {
        // Recipe output goes to terminal (status() inherits stdout).
        // ShellResult.stdout/stderr should be empty.
        let shell = ShShell;
        let env = HashMap::new();
        let result = shell.execute("echo visible", &env, Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[cfg(feature = "duckscript")]
    #[test]
    fn duck_shell_name() {
        assert_eq!(DuckShell.name(), "duckscript");
    }

    #[cfg(feature = "duckscript")]
    #[test]
    fn duck_shell_execute_simple() {
        let shell = DuckShell;
        let env = HashMap::new();
        let result = shell.execute("echo hello", &env, Path::new(".")).unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
