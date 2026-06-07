use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use mk_core::shell::{Shell, ShellResult, ShellError};

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
        cmd.arg("-e")           // exit on first error (like mk)
           .arg("-c")           // read command from argument
           .arg(recipe)
           .current_dir(dir);

        // Clear and set environment
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        // Ensure PATH exists
        if !env.contains_key("PATH") {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }

        let output = cmd.output()?;

        Ok(ShellResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
        assert!(result.stdout.contains("hello"));
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
        let result = shell.execute("echo $MYVAR", &env, Path::new(".")).unwrap();
        assert!(result.stdout.contains("myval"));
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
}
