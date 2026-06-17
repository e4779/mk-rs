#![forbid(unsafe_code)]

//! plan9 mk build-tool core library.
//!
//! mk-core is a faithful Rust port of Andrew Hume's Plan 9 `mk`. It reads mkfiles,
//! builds a dependency graph, resolves pattern-based metarules, and dispatches
//! parallel recipe execution through a shell abstraction.
//!
//! # Pipeline
//!
//! Each `build()` call runs these stages in sequence, each producing an owned
//! output consumed by the next:
//!
//! ```text
//! ┌──────────────┐
//! │   mkfile(s)   │  user-authored text
//! └──────┬───────┘
//!        │
//! ┌──────▼───────┐
//! │  lex::Lexer   │  char-by-char → token stream
//! └──────┬───────┘
//!        │  TokenStream
//! ┌──────▼───────┐
//! │ parse::Parser │  recursive descent → AST
//! └──────┬───────┘
//!        │  Vec<Stmt>
//! ┌──────▼───────┐
//! │  var::Scope   │  expand variables
//! └──────┬───────┘
//!        │  expanded AST
//! ┌──────▼───────┐
//! │ graph::Builder│  AST → DAG (metarules, transitive closure, pruning)
//! └──────┬───────┘
//!        │  Graph
//! ┌──────▼───────┐
//! │graph::Checker │  staleness (mtime comparison)
//! └──────┬───────┘
//!        │  BuildPlan
//! ┌──────▼───────┐
//! │ sched::Engine │  parallel DAG walk, NPROC worker pool
//! └──────┬───────┘
//!        │  Job queue
//! ┌──────▼───────┐
//! │recipe::Runner │  feed recipe to shell
//! └──────┬───────┘
//!        │  exit code
//! ┌──────▼───────┐
//! │ BuildOutcome  │  success, partial (with -k), or failure
//! └──────────────┘
//! ```
//!
//! # Module roster
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`lex`] | Tokenizer — comment stripping, line continuation, backtick regions, quoting |
//! | [`parse`] | Recursive-descent parser — rules, assignments, includes, attributes |
//! | [`graph`] | DAG builder — metarule application, transitive closure, cycle/staleness checks |
//! | [`var`] | Variable system — symbol table, `$VAR`/`${VAR}` expansion, namelists |
//! | [`shell`] | `Shell` trait — abstraction for recipe execution (implementations in mk-shell) |
//! | [`recipe`] | Recipe glue — env injection, attribute handling, CLI flag dispatch |
//! | [`sched`] | Scheduler — parallel DAG traversal, NPROC worker pool, keep-going support |
//! | [`attr`] | Attribute bitflags — `V`/`Q`/`N`/`U`/`D`/`E`/`P`/`R`/`n` |
//! | [`mod@include`] | Recursive `< file` includes — child scopes, circular detection |
//! | [`archive`] | `lib(member)` syntax — archive member auto-rule generation |
//! | [`error`] | Centralized error types — `MkError`, `LexError`, `ParseError`, … |

pub mod archive;
pub mod attr;
pub mod error;
pub mod graph;
pub mod include;
pub mod lex;
pub mod parse;
pub mod recipe;
pub mod sched;
pub mod shell;
pub mod var;
