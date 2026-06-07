//! Recursive mkfile include system.
//!
//! Resolves `< file` directives: reads, lexes, and parses included mkfiles.
//! Files get their own variable scope (child of parent). Circular includes
//! are detected via chain tracking.

use crate::error::IncludeError;
use crate::lex::{tokenize, ShellMode};
use crate::parse::{parse, Stmt};
use std::path::{Path, PathBuf};

// ── IncludeContext ─────────────────────────────────────────────────────────

/// Tracks the include chain for circular-dependency detection.
#[derive(Debug, Clone)]
pub struct IncludeContext {
    /// Stack of canonical paths currently being included.
    pub chain: Vec<PathBuf>,
}

impl IncludeContext {
    /// Create a fresh include context with an empty chain.
    pub fn new() -> Self {
        IncludeContext { chain: Vec::new() }
    }

    /// Resolve, read, lex, and parse an included mkfile.
    ///
    /// `path` is the path as written in the mkfile (may be relative).
    /// `base_dir` is the directory of the including mkfile, used to resolve
    /// relative paths.
    ///
    /// Returns the parsed statements from the included file, or an
    /// `IncludeError` on failure (circular include, file not found, or
    /// parse/lex errors in the included file).
    pub fn include_file(
        &mut self,
        path: &str,
        base_dir: &Path,
    ) -> Result<Vec<Stmt>, IncludeError> {
        // 1. Resolve path
        let resolved = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            base_dir.join(path)
        };
        let canonical = resolved.canonicalize().unwrap_or(resolved);

        // 2. Circular detection — check if this canonical path is already in the chain
        if self.chain.iter().any(|p| p == &canonical) {
            let chain_str = self
                .chain
                .iter()
                .map(|p| p.display().to_string())
                .chain(std::iter::once(canonical.display().to_string()))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(IncludeError::CircularInclude { chain: chain_str });
        }

        // 3. Read file
        let content = std::fs::read_to_string(&canonical).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                IncludeError::FileNotFound {
                    path: path.to_string(),
                }
            } else {
                IncludeError::Io(e)
            }
        })?;

        // 4. Push onto chain; lex + parse; pop even on error
        self.chain.push(canonical.clone());
        let result = (|| {
            let tokens = tokenize(&content, ShellMode::Sh).map_err(|e| {
                IncludeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}: {e}", canonical.display()),
                ))
            })?;
            parse(&tokens).map_err(|e| {
                IncludeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}: {e}", canonical.display()),
                ))
            })
        })();
        self.chain.pop();
        result
    }

    /// Run a shell command and parse its stdout as mkfile syntax.
    ///
    /// The command is executed via `sh -c <command>` with `base_dir` as
    /// the working directory. Stdout is lexed and parsed; stderr is
    /// discarded. Returns an error if the command exits non-zero.
    pub fn include_command(
        &mut self,
        command: &str,
        base_dir: &Path,
    ) -> Result<Vec<Stmt>, IncludeError> {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(base_dir)
            .output()
            .map_err(IncludeError::Io)?;

        if !output.status.success() {
            return Err(IncludeError::CommandFailed {
                command: command.to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let tokens = tokenize(&stdout, ShellMode::Sh).map_err(|e| {
            IncludeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;

        parse(&tokens).map_err(|e| {
            IncludeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })
    }
}

impl Default for IncludeContext {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a temporary mkfile in a test subdirectory and return its path.
    fn write_temp_mkfile(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("mk_test_include");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn include_simple_file() {
        let included = write_temp_mkfile("common.mk", "CC = gcc\n");
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_file(included.to_str().unwrap(), &std::env::temp_dir())
            .unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn include_file_not_found() {
        let mut ctx = IncludeContext::new();
        let result = ctx.include_file("nonexistent.mk", &PathBuf::from("."));
        assert!(matches!(result, Err(IncludeError::FileNotFound { .. })));
    }

    #[test]
    fn circular_include_detected() {
        let path = write_temp_mkfile("circular.mk", "CC = gcc\n");
        let canonical = path.canonicalize().unwrap();
        let dir = path.parent().unwrap().to_path_buf();

        let mut ctx = IncludeContext::new();
        // Simulate an active include chain: push the path first
        ctx.chain.push(canonical);
        // Now try to include it again → circular
        let result = ctx.include_file(path.to_str().unwrap(), &dir);
        assert!(matches!(result, Err(IncludeError::CircularInclude { .. })));
    }

    #[test]
    fn chain_cleared_after_successful_include() {
        let path = write_temp_mkfile("chain_test.mk", "CC = gcc\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        ctx.include_file(path.to_str().unwrap(), &dir).unwrap();
        assert!(ctx.chain.is_empty());
    }

    #[test]
    fn chain_cleaned_on_lex_error() {
        // Write a file with a lex error (unterminated quote)
        let bad = write_temp_mkfile("bad_lex.mk", "TARGET: prereq\n\tcmd 'oops\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        let result = ctx.include_file(bad.to_str().unwrap(), &dir);
        assert!(result.is_err());
        // Chain must be clean even after error
        assert!(ctx.chain.is_empty());
    }

    #[test]
    fn absolute_path() {
        let path = write_temp_mkfile("absolute_test.mk", "TARGET = foo\n");
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &PathBuf::from("/unused"))
            .unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn include_empty_file() {
        let path = write_temp_mkfile("empty.mk", "");
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir())
            .unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn include_with_rule_and_recipe() {
        let path = write_temp_mkfile("recipe_test.mk", "target: prereq\n\techo hello\n");
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir())
            .unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["target"]);
                assert_eq!(r.prereqs, vec!["prereq"]);
                assert_eq!(r.recipe, Some("echo hello".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn include_with_multiple_statements() {
        let path = write_temp_mkfile(
            "multi.mk",
            "CC = gcc\nCFLAGS = -Wall\n\nprog: main.o\n\t$(CC) -o $target $prereq\n",
        );
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir())
            .unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn relative_path_resolution() {
        // Write a file to a subdirectory and include via relative path from parent
        let parent_dir = std::env::temp_dir().join("mk_test_parent");
        let child_dir = parent_dir.join("sub");
        std::fs::create_dir_all(&child_dir).unwrap();
        let sub_path = child_dir.join("child.mk");
        std::fs::write(&sub_path, "VAR = child_value\n").unwrap();

        let mut ctx = IncludeContext::new();
        let stmts = ctx.include_file("sub/child.mk", &parent_dir).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn include_context_default() {
        let ctx = IncludeContext::default();
        assert!(ctx.chain.is_empty());
    }

    #[test]
    fn circular_include_chain_message() {
        let dir = std::env::temp_dir().join("mk_test_chain_msg");
        std::fs::create_dir_all(&dir).unwrap();
        let a_path = dir.join("a.mk");
        let b_path = dir.join("b.mk");
        std::fs::write(&a_path, "CC = gcc\n").unwrap();
        std::fs::write(&b_path, "CXX = g++\n").unwrap();

        let mut ctx = IncludeContext::new();
        let canonical_a = a_path.canonicalize().unwrap();
        let canonical_b = b_path.canonicalize().unwrap();
        ctx.chain.push(canonical_a.clone());
        ctx.chain.push(canonical_b.clone());

        // Including a.mk again → circular A -> B -> A
        let result = ctx.include_file(a_path.to_str().unwrap(), &dir);
        match result {
            Err(IncludeError::CircularInclude { chain }) => {
                assert!(chain.contains("a.mk"));
                assert!(chain.contains("b.mk"));
                assert!(chain.contains(" -> "));
            }
            other => panic!("expected CircularInclude, got {other:?}"),
        }
    }

    #[test]
    fn include_command_simple() {
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_command("echo 'TARGET = value'", &std::env::current_dir().unwrap())
            .unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Assign(a) => {
                assert_eq!(a.name, "TARGET");
                assert_eq!(a.value, "value");
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn include_command_failed() {
        let mut ctx = IncludeContext::new();
        let result =
            ctx.include_command("exit 1", &std::env::current_dir().unwrap());
        assert!(matches!(result, Err(IncludeError::CommandFailed { .. })));
    }

    #[test]
    fn include_command_rule_with_recipe() {
        let mut ctx = IncludeContext::new();
        let stmts = ctx
            .include_command(
                "printf 'target: prereq\n\techo hello\n'",
                &std::env::current_dir().unwrap(),
            )
            .unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["target"]);
                assert_eq!(r.prereqs, vec!["prereq"]);
                assert_eq!(r.recipe, Some("echo hello".into()));
            }
            _ => panic!("expected Rule"),
        }
    }
}
