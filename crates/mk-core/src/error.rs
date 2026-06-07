//! Centralized error types for mk-core.
//!
//! Every fallible function in mk-core returns `Result<T, MkError>`.
//! No panics in library code.

use thiserror::Error;

// ── Main error type ────────────────────────────────────────────────────────

/// Top-level error for all mk-core operations.
#[derive(Debug, Error)]
pub enum MkError {
    #[error("lex error: {0}")]
    Lex(#[from] LexError),

    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("variable error: {0}")]
    Var(#[from] VarError),

    #[error("graph error: {0}")]
    Graph(#[from] GraphError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("shell error: {0}")]
    Shell(#[from] ShellError),

    #[error("recipe error: {0}")]
    Recipe(#[from] RecipeError),

    #[error("scheduler error: {0}")]
    Sched(#[from] SchedError),

    #[error("include error: {0}")]
    Include(#[from] IncludeError),
}

// ── Sub-error types ────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LexError {
    #[error("unterminated quote at position {pos}")]
    UnterminatedQuote { pos: usize },

    #[error("unterminated backtick at position {pos}")]
    UnterminatedBacktick { pos: usize },
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("expected colon at line {line}")]
    ExpectedColon { line: usize },

    #[error("ambiguous recipe for target {target} at line {line}")]
    AmbiguousRecipe { target: String, line: usize },

    #[error("unknown attribute {attr} at line {line}")]
    UnknownAttr { attr: char, line: usize },

    #[error("unexpected token at line {line}: expected {expected}, got {got}")]
    UnexpectedToken {
        expected: String,
        got: String,
        line: usize,
    },

    #[error("empty target name at line {line}")]
    EmptyTarget { line: usize },
}

#[derive(Debug, Error)]
pub enum VarError {
    #[error("undefined variable: ${name}")]
    UndefinedVar { name: String },

    #[error("invalid variable reference: {ref_}")]
    InvalidRef { ref_: String },

    #[error("invalid substitution pattern: {pattern}")]
    InvalidPattern { pattern: String },

    #[error("recursive variable expansion: ${name}")]
    RecursiveExpansion { name: String },
}

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("cyclic dependency detected: {chain}")]
    Cycle { chain: String },

    #[error("ambiguous rules for target {target}")]
    AmbiguousTarget { target: String },

    #[error("no rule to make target {target}")]
    NoRule { target: String },

    #[error("target {target} is up to date")]
    UpToDate { target: String },
}

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("shell not found: {name}")]
    ShellNotFound { name: String },

    #[error("recipe execution failed with exit code {code}: {stderr}")]
    CommandFailed { code: i32, stderr: String },

    #[error("shell I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum RecipeError {
    #[error("recipe command failed with exit code {code}: {stderr}")]
    CommandFailed { code: i32, stderr: String },

    #[error("recipe target {target} deleted after error")]
    TargetDeleted { target: String },

    #[error("recipe I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum SchedError {
    #[error("build aborted due to errors")]
    BuildFailed,

    #[error("no targets specified")]
    NoTargets,
}

#[derive(Debug, Error)]
pub enum IncludeError {
    #[error("circular include detected: {chain}")]
    CircularInclude { chain: String },

    #[error("included file not found: {path}")]
    FileNotFound { path: String },

    #[error("include command failed: {command}")]
    CommandFailed { command: String },

    #[error("include I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Convenience alias ──────────────────────────────────────────────────────

/// Standard Result type for mk-core operations.
pub type MkResult<T> = Result<T, MkError>;

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mk_error_from_lex_error() {
        let err: MkError = LexError::UnterminatedQuote { pos: 42 }.into();
        assert!(matches!(err, MkError::Lex(_)));
    }

    #[test]
    fn mk_error_from_parse_error() {
        let err: MkError = ParseError::ExpectedColon { line: 10 }.into();
        assert!(matches!(err, MkError::Parse(_)));
    }

    #[test]
    fn mk_error_from_var_error() {
        let err: MkError = VarError::UndefinedVar {
            name: "FOO".into(),
        }
        .into();
        assert!(matches!(err, MkError::Var(_)));
    }

    #[test]
    fn mk_error_from_graph_error() {
        let err: MkError = GraphError::Cycle {
            chain: "a -> b -> a".into(),
        }
        .into();
        assert!(matches!(err, MkError::Graph(_)));
    }

    #[test]
    fn mk_error_from_io_error() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: MkError = io.into();
        assert!(matches!(err, MkError::Io(_)));
    }

    #[test]
    fn shell_error_display() {
        let err = ShellError::CommandFailed {
            code: 1,
            stderr: "gcc: fatal error".into(),
        };
        let s = err.to_string();
        assert!(s.contains("exit code 1"));
        assert!(s.contains("gcc: fatal error"));
    }

    #[test]
    fn graph_cycle_display() {
        let err = GraphError::Cycle {
            chain: "a -> b -> c -> a".into(),
        };
        assert!(err.to_string().contains("cyclic"));
        assert!(err.to_string().contains("a -> b -> c -> a"));
    }

    #[test]
    fn var_invalid_ref_display() {
        let err = VarError::InvalidRef {
            ref_: "$missing}".into(),
        };
        assert!(err.to_string().contains("$missing}"));
    }

    #[test]
    fn parse_unexpected_token_format() {
        let err = ParseError::UnexpectedToken {
            expected: "colon".into(),
            got: "xyz".into(),
            line: 5,
        };
        assert!(err.to_string().contains("expected colon"));
        assert!(err.to_string().contains("got xyz"));
    }

    #[test]
    fn include_circular_display() {
        let err = IncludeError::CircularInclude {
            chain: "a.mk -> b.mk -> a.mk".into(),
        };
        assert!(err.to_string().contains("circular"));
    }

    #[test]
    fn error_sizes_are_reasonable() {
        use std::mem::size_of;
        assert!(size_of::<LexError>() <= 16);
        assert!(size_of::<ParseError>() <= 56);
        assert!(size_of::<GraphError>() <= 32);
        assert!(size_of::<MkError>() <= 56);
    }
}
