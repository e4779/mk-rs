// mk-cli: Command-line interface for mk-rust.
//
// Plan 9 mk compatible build tool.
// Thin wrapper around mk-core: parse args → read mkfile → build DAG → execute.

use std::collections::HashMap;
use std::path::PathBuf;

use clap::Parser;

use mk_core::graph::build_graph;
use mk_core::lex::{tokenize, ShellMode};
use mk_core::parse::Stmt;
use mk_core::sched::{execute, ResolvedRule, SchedOptions};
use mk_core::var::{builtin_scope, import_env, Precedence};
use mk_shell::ShShell;

/// mk — maintain (make) related files
///
/// Reads dependency rules from a mkfile and executes recipes
/// to bring targets up to date.
#[derive(Parser)]
#[command(name = "mk", version, about)]
struct Cli {
    /// Mkfile to read (default: mkfile)
    #[arg(short = 'f', default_value = "mkfile")]
    file: PathBuf,

    /// Print commands but do not execute
    #[arg(short = 'n')]
    no_exec: bool,

    /// Explain why each target is (or is not) being made
    #[arg(short = 'e')]
    explain: bool,

    /// Touch targets instead of running recipes
    #[arg(short = 't')]
    touch: bool,

    /// Assume all targets are out of date
    #[arg(short = 'a')]
    all: bool,

    /// Keep going after errors
    #[arg(short = 'k')]
    keep_going: bool,

    /// Force missing intermediate targets to be built (stub — Phase 1b)
    #[arg(short = 'i')]
    force_intermediates: bool,

    /// Silent mode: don't print recipes before execution
    #[arg(short = 's')]
    silent: bool,

    /// Debug output: p (parse), g (graph), e (execution) (stub — Phase 2)
    #[arg(short = 'd')]
    debug: Option<String>,

    /// What-if mode: pretend listed targets are modified (stub — Phase 2)
    #[arg(short = 'w')]
    whatif: Option<String>,

    /// Change to directory before building
    #[arg(short = 'C')]
    directory: Option<PathBuf>,

    /// Targets to build (default: first target in mkfile)
    targets: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("mk: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Stub flags (parsed, not yet implemented)
    let _ = &cli.force_intermediates;
    let _ = &cli.whatif;

    // -d debug flag: print requested debug categories
    if let Some(ref debug) = cli.debug {
        for ch in debug.chars() {
            match ch {
                'p' => eprintln!("mk: debug: parsing enabled"),
                'g' => eprintln!("mk: debug: graph building enabled"),
                'e' => eprintln!("mk: debug: execution enabled"),
                _ => eprintln!("mk: warning: unknown debug flag '{}'", ch),
            }
        }
    }

    // Chdir if -C specified
    if let Some(ref dir) = cli.directory {
        std::env::set_current_dir(dir)?;
    }

    // Read mkfile: try -f argument first, fall back to "mkfile"
    let input = std::fs::read_to_string(&cli.file).or_else(|_| {
        std::fs::read_to_string("mkfile")
    }).map_err(|_| {
        format!(
            "no mkfile: could not read '{}' or 'mkfile'",
            cli.file.display()
        )
    })?;

    // Lex + Parse
    let tokens = tokenize(&input, ShellMode::Sh)?;
    let stmts = mk_core::parse::parse(&tokens)?;

    // Build variable scope: built-ins, environment, mkfile assignments
    let mut scope = builtin_scope();
    import_env(&mut scope);
    for stmt in &stmts {
        if let Stmt::Assign(a) = stmt {
            scope.set(&a.name, &a.value, Precedence::Mkfile);
        }
    }

    // Build rules map: target name → resolved rule
    let mut rules: HashMap<String, ResolvedRule> = HashMap::new();
    for stmt in &stmts {
        if let Stmt::Rule(r) = stmt {
            for t in &r.targets {
                rules.insert(
                    t.clone(),
                    ResolvedRule {
                        recipe: r.recipe.clone().unwrap_or_default(),
                        attributes: r.attributes,
                    },
                );
            }
        }
    }

    // Determine targets: CLI args or first target of first rule
    let target_names: Vec<String> = if cli.targets.is_empty() {
        let first_rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s {
                Some(r)
            } else {
                None
            }
        });
        match first_rule {
            Some(r) => vec![r.targets[0].clone()],
            None => {
                eprintln!("mk: no targets specified and no rules in mkfile");
                std::process::exit(1);
            }
        }
    } else {
        cli.targets.clone()
    };

    // Build DAG
    let mut graph = build_graph(&stmts, &target_names)?;

    // MKSHELL: allow switching shell via env variable (F-053)
    // Only sh is supported for now; rc support comes in Phase 3.
    let mkshell = scope.get("MKSHELL").unwrap_or("/bin/sh").to_string();
    if mkshell.contains("rc") {
        eprintln!("mk: warning: rc shell not yet supported, using sh");
    }

    // Build sched options from CLI flags
    let mkflags = std::env::args()
        .skip(1)
        .filter(|a| a.starts_with('-') || a.contains('='))
        .collect::<Vec<_>>()
        .join(" ");
    let mkargs = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-') && !a.contains('='))
        .collect::<Vec<_>>()
        .join(" ");

    let opts = SchedOptions {
        keep_going: cli.keep_going,
        no_exec: cli.no_exec,
        explain: cli.explain,
        touch: cli.touch,
        silent: cli.silent,
        all: cli.all,
        nproc: 1, // sequential by default; $NPROC env var overrides
        force_intermediates: cli.force_intermediates,
        mkshell,
        mkflags,
        mkargs,
    };

    // Build environment from variable scope
    let env = scope.export();

    let working_dir = std::env::current_dir()?;

    // Execute
    let shell = ShShell; // always sh for now, RcShell in Phase 3
    let outcome = execute(&mut graph, &rules, &shell, &working_dir, &env, &opts)?;

    // Print failures (only reachable with -k)
    if !outcome.failed.is_empty() {
        for (target, msg) in &outcome.failed {
            eprintln!("mk: {}: {}", target, msg);
        }
        std::process::exit(1);
    }

    Ok(())
}
