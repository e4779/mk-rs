use std::collections::HashMap;
use std::path::Path;

/// Result of executing a recipe through a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellResult {
    /// Exit code. 0 = success.
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
}

pub use crate::error::ShellError;

/// Abstraction for executing recipe scripts.
/// Implementations: sh (POSIX /bin/sh), rc (Plan 9 rc), duckscript (future).
pub trait Shell: Send + Sync {
    /// Human-readable shell name (e.g. "sh", "rc").
    fn name(&self) -> &str;

    /// Execute a recipe script.
    /// `recipe` — the full script text (multiline string).
    /// `env` — environment variables to pass to the shell process.
    /// `dir` — working directory for the recipe.
    fn execute(
        &self,
        recipe: &str,
        env: &HashMap<String, String>,
        dir: &Path,
    ) -> Result<ShellResult, ShellError>;

    /// Find unescaped instances of a character in a string.
    /// Used by the parser to detect assignment attributes.
    /// E.g. find unescaped '=' to separate attr from value.
    fn find_unescaped(&self, input: &str, ch: char) -> Vec<usize>;

    /// Shell-quote a string so it's safe as a shell argument.
    fn quote(&self, token: &str) -> String;
}

#[cfg(test)]
mod tests {
    // Shell trait tests are in mk-shell's ShShell tests.
}
