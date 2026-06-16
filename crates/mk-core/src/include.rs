//! Recursive mkfile include system.
//!
//! Resolves `< file` directives: reads, lexes, and parses included mkfiles.
//! Files get their own variable scope (child of parent). Circular includes
//! are detected via chain tracking.

use crate::error::IncludeError;
use crate::lex::{tokenize, ShellMode};
use crate::parse::{parse_with_scope, Stmt};
use crate::var::Scope;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── IncludeContext ─────────────────────────────────────────────────────────

/// Tracks the include chain for circular-dependency detection
/// and a set of all already-parsed files to prevent duplicate parsing
/// in diamond include scenarios.
#[derive(Debug, Clone)]
pub struct IncludeContext {
    /// Stack of canonical paths currently being included.
    pub chain: Vec<PathBuf>,
    /// Set of canonical paths that have already been parsed.
    /// Prevents double-parsing when the same file is included from
    /// multiple branches (diamond includes).
    pub seen: HashSet<PathBuf>,
}

impl IncludeContext {
    /// Create a fresh include context with an empty chain and empty seen set.
    pub fn new() -> Self {
        IncludeContext {
            chain: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Resolve, read, lex, and parse an included mkfile.
    ///
    /// `path` is the path as written in the mkfile (may be relative).
    /// `base_dir` is the directory of the including mkfile, used to resolve
    /// relative paths.
    /// `scope` is the parent scope — included files share the same
    /// variable namespace. The path is expanded through `scope.expand`
    /// before resolution (S8: `< $INCL` / `` < `{echo sub.mk}` ``).
    ///
    /// Returns the parsed statements from the included file, or an
    /// `IncludeError` on failure (circular include, file not found, or
    /// parse/lex errors in the included file).
    pub fn include_file(
        &mut self,
        path: &str,
        base_dir: &Path,
        scope: &mut Scope,
    ) -> Result<Vec<Stmt>, IncludeError> {
        // 0. Expand path through scope (S8)
        let expanded_path = scope.expand(path);

        // 1. Resolve path
        let resolved = if expanded_path.starts_with('/') {
            PathBuf::from(&expanded_path)
        } else {
            base_dir.join(&expanded_path)
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

        // 2b. Duplicate detection — skip if already parsed (diamond includes)
        if self.seen.contains(&canonical) {
            return Ok(Vec::new());
        }

        // 3. Read file
        let content = std::fs::read_to_string(&canonical).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                IncludeError::FileNotFound {
                    path: expanded_path.to_string(),
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
            parse_with_scope(&tokens, scope).map_err(|e| {
                IncludeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}: {e}", canonical.display()),
                ))
            })
        })();
        self.chain.pop();

        // Mark as seen after successful parse (even empty result)
        self.seen.insert(canonical);

        result
    }

    /// Run a shell command and parse its stdout as mkfile syntax.
    ///
    /// The command is executed via `sh -c <command>` with `base_dir` as
    /// the working directory. Stdout is lexed and parsed; stderr is
    /// discarded. Returns an error if the command exits non-zero.
    /// `scope` is the parent scope; the command is expanded through
    /// `scope.expand` before execution (S8: `<| $CMD`).
    pub fn include_command(
        &mut self,
        command: &str,
        base_dir: &Path,
        scope: &mut Scope,
    ) -> Result<Vec<Stmt>, IncludeError> {
        // Expand command through scope (S8)
        let expanded_cmd = scope.expand(command);

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&expanded_cmd)
            .current_dir(base_dir)
            .output()
            .map_err(IncludeError::Io)?;

        if !output.status.success() {
            return Err(IncludeError::CommandFailed {
                command: expanded_cmd,
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let tokens = tokenize(&stdout, ShellMode::Sh).map_err(|e| {
            IncludeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;

        parse_with_scope(&tokens, scope).map_err(|e| {
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
    use crate::var::{Precedence, Scope};

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
        let mut scope = Scope::new();
        let stmts = ctx
            .include_file(included.to_str().unwrap(), &std::env::temp_dir(), &mut scope)
            .unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn include_file_not_found() {
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let result = ctx.include_file("nonexistent.mk", &PathBuf::from("."), &mut scope);
        assert!(matches!(result, Err(IncludeError::FileNotFound { .. })));
    }

    #[test]
    fn circular_include_detected() {
        let path = write_temp_mkfile("circular.mk", "CC = gcc\n");
        let canonical = path.canonicalize().unwrap();
        let dir = path.parent().unwrap().to_path_buf();

        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        // Simulate an active include chain: push the path first
        ctx.chain.push(canonical);
        // Now try to include it again → circular
        let result = ctx.include_file(path.to_str().unwrap(), &dir, &mut scope);
        assert!(matches!(result, Err(IncludeError::CircularInclude { .. })));
    }

    #[test]
    fn chain_cleared_after_successful_include() {
        let path = write_temp_mkfile("chain_test.mk", "CC = gcc\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        ctx.include_file(path.to_str().unwrap(), &dir, &mut scope).unwrap();
        assert!(ctx.chain.is_empty());
    }

    #[test]
    fn chain_cleaned_on_lex_error() {
        // Write a file with a lex error (unterminated quote)
        let bad = write_temp_mkfile("bad_lex.mk", "TARGET: prereq\n\tcmd 'oops\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let result = ctx.include_file(bad.to_str().unwrap(), &dir, &mut scope);
        assert!(result.is_err());
        // Chain must be clean even after error
        assert!(ctx.chain.is_empty());
    }

    #[test]
    fn diamond_include_skipped_on_second_encounter() {
        // A includes B and C, both include D — D should be parsed only once.
        // Simulate this by including D twice via the same context.
        let d = write_temp_mkfile("diamond_d.mk", "VAR = from_d\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();

        // First include of D — should succeed and parse the content.
        let stmts1 = ctx.include_file(d.to_str().unwrap(), &dir, &mut scope).unwrap();
        assert_eq!(stmts1.len(), 1, "first include of D should return statement");
        assert!(ctx.seen.len() == 1, "D should be in seen set");

        // Second include of D (from another branch) — should return empty.
        let stmts2 = ctx.include_file(d.to_str().unwrap(), &dir, &mut scope).unwrap();
        assert!(stmts2.is_empty(), "second include of D should be empty");
    }

    #[test]
    fn diamond_include_chain_cleared() {
        // After including D through B, the chain should be clean.
        let d = write_temp_mkfile("diamond_chain_d.mk", "VAR = d_val\n");
        let dir = std::env::temp_dir().join("mk_test_include");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();

        ctx.include_file(d.to_str().unwrap(), &dir, &mut scope).unwrap();
        assert!(ctx.chain.is_empty(), "chain should be empty after include");
        // D should still be marked as seen.
        assert!(ctx.seen.len() == 1, "seen set should contain D");
    }

    #[test]
    fn absolute_path() {
        let path = write_temp_mkfile("absolute_test.mk", "TARGET = foo\n");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &PathBuf::from("/unused"), &mut scope)
            .unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn include_empty_file() {
        let path = write_temp_mkfile("empty.mk", "");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir(), &mut scope)
            .unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn include_with_rule_and_recipe() {
        let path = write_temp_mkfile("recipe_test.mk", "target: prereq\n\techo hello\n");
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir(), &mut scope)
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
        let mut scope = Scope::new();
        let stmts = ctx
            .include_file(path.to_str().unwrap(), &std::env::temp_dir(), &mut scope)
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
        let mut scope = Scope::new();
        let stmts = ctx.include_file("sub/child.mk", &parent_dir, &mut scope).unwrap();
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
        let mut scope = Scope::new();
        let canonical_a = a_path.canonicalize().unwrap();
        let canonical_b = b_path.canonicalize().unwrap();
        ctx.chain.push(canonical_a.clone());
        ctx.chain.push(canonical_b.clone());

        // Including a.mk again → circular A -> B -> A
        let result = ctx.include_file(a_path.to_str().unwrap(), &dir, &mut scope);
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
        let mut scope = Scope::new();
        let stmts = ctx
            .include_command("echo 'TARGET = value'", &std::env::current_dir().unwrap(), &mut scope)
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
        let mut scope = Scope::new();
        let result =
            ctx.include_command("exit 1", &std::env::current_dir().unwrap(), &mut scope);
        assert!(matches!(result, Err(IncludeError::CommandFailed { .. })));
    }

    #[test]
    fn include_command_rule_with_recipe() {
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        let stmts = ctx
            .include_command(
                "printf 'target: prereq\n\techo hello\n'",
                &std::env::current_dir().unwrap(),
                &mut scope,
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

    // ── F-045 S8: include path/command expansion ──────────────────────

    #[test]
    fn f045_s8_include_path_expanded() {
        // S8: < $INCL resolves variables in the include path.
        let included = write_temp_mkfile("s8_test.mk", "CC = gcc\n");
        let parent_dir = included.parent().unwrap();
        let file_name = included.file_name().unwrap().to_str().unwrap();

        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        scope.set_raw("INCL", file_name, Precedence::Mkfile);

        let stmts = ctx
            .include_file("$INCL", parent_dir, &mut scope)
            .unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn f045_s8_include_command_expanded() {
        // S8: <| $CMD expands variables in the command.
        let mut ctx = IncludeContext::new();
        let mut scope = Scope::new();
        scope.set_raw("ECHO_CMD", "echo 'TARGET = s8_val'", Precedence::Mkfile);

        let stmts = ctx
            .include_command("$ECHO_CMD", &std::env::current_dir().unwrap(), &mut scope)
            .unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Assign(a) => {
                assert_eq!(a.name, "TARGET");
                assert_eq!(a.value, "s8_val");
            }
            _ => panic!("expected Assign"),
        }
    }
}
