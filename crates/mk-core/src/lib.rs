// mk-core: plan9 mk build-tool core library.
//
// Architecture:
//   mkfile text → lex → tokens → parse → AST → graph → DAG → sched → recipe exec
//
// Modules:
//   lex    — tokenizer (F-001..F-016)
//   attr   — rule attribute bitflags (F-009..F-010, F-023..F-028)
//   error  — unified error type (all modules)
//   parse  — parser: rules, assignments, includes (Phase 1b)
//   graph  — DAG builder, staleness, cycle detection (Phase 1a)
//   var    — variable system, symbol table, expansion (Phase 1a)
//   shell  — Shell trait (Phase 1a)
//   recipe — recipe execution glue (Phase 1a)
//   sched  — scheduler: serial & parallel execution (Phase 1a serial, Phase 2 parallel)
//   include — recursive mkfile includes (Phase 1b)
//   archive — lib(member) syntax (Phase 3)

pub mod lex;
pub mod attr;
pub mod error;
pub mod parse;
pub mod var;
pub mod graph;
pub mod shell;
pub mod recipe;
pub mod sched;
pub mod include;
