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
//! ┌─────────────────────────────┐
//! │   mkfile(s)                  │  user-authored text
//! └──────────────┬──────────────┘
//!                │
//! ┌──────────────▼──────────────┐
//! │  lex::Lexer                 │  char-by-char → token stream
//! └──────────────┬──────────────┘
//!                │  TokenStream
//! ┌──────────────▼───────────────────────────┐
//! │ parse::parse_with_scope(tokens, &mut    │  recursive descent → AST with
//! │   Scope)                                 │  rule-header vars + assignment
//! │                                          │  RHS expanded at read-time (F-045)
//! └──────────────┬───────────────────────────┘
//!                │  Vec<Stmt> (headers/RHS already expanded;
//!                │     recipe bodies still literal, expanded at exec time)
//! ┌──────────────▼───────────────────────────┐
//! │ graph::build_graph(stmts, targets)       │  AST → DAG (metarules, transitive
//! │                                          │  closure, pruning, cycle detection)
//! └──────────────┬───────────────────────────┘
//!                │  Graph
//! ┌──────────────▼───────────────────────────┐
//! │ graph::stale_nodes(&graph)              │  mtime staleness; virtual targets
//! │                                          │  unconditionally stale (no file)
//! └──────────────┬───────────────────────────┘
//!                │  stale NodeIndex set
//! ┌──────────────▼───────────────────────────┐
//! │ sched::Engine                            │  parallel DAG walk, NPROC pool,
//! │                                          │  keep-going (-k), exclusive (E)
//! └──────────────┬───────────────────────────┘
//!                │  Job queue
//! ┌──────────────▼───────────────────────────┐
//! │ recipe::run()                            │  feed recipe to shell; inject
//! │                                          │  $target/$prereq/$stem env vars
//! └──────────────┬───────────────────────────┘
//!                │  exit code
//! ┌──────────────▼───────────────────────────┐
//! │ BuildOutcome                             │  success, partial (-k), or failure
//! └──────────────────────────────────────────┘
//! ```
//!
//! Variable expansion is split across two stages: rule headers and assignment
//! RHS are expanded at parse-time via the threaded `Scope` (F-045); recipe
//! bodies are kept literal and `$target`/`$prereq`/`$stem` are injected as
//! environment variables just before shell dispatch.
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
