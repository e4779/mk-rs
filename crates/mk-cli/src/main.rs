// mk-cli: Command-line interface for mk-rust.
//
// Plan 9 mk compatible build tool.
// Thin wrapper around mk-core: parse args → read mkfile → build DAG → execute.

use std::io::IsTerminal;
use std::collections::HashMap;
use std::path::PathBuf;

use clap::Parser;

use mk_rs_core::graph::build_graph_with_nrep;
use mk_rs_core::lex::{tokenize, ShellMode};
use mk_rs_core::parse::Stmt;
use mk_rs_core::sched::{execute, ResolvedRule, SchedOptions};
use mk_rs_core::var::{builtin_scope, import_env, Precedence};
use mk_rs_shell::{CustomShell, ShShell};
#[cfg(feature = "duckscript")]
use mk_rs_shell::DuckShell;

/// mk — maintain (make) related files
///
/// Reads dependency rules from a mkfile and executes recipes
/// to bring targets up to date.
#[derive(Parser)]
#[command(
    name = "mk",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    about
)]
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

    /// Quiet mode: don't print recipes before execution
    #[arg(short = 'q')]
    silent: bool,

    /// Color output: auto, always, never
    #[arg(long, default_value = "auto")]
    color: String,

    /// Debug output: p (parse), g (graph), e (execution) (stub — Phase 2)
    #[arg(short = 'd')]
    debug: Option<String>,

    /// What-if mode: pretend listed targets are modified (stub — Phase 2)
    #[arg(short = 'w')]
    whatif: Option<String>,

    /// Change to directory before building
    #[arg(short = 'C')]
    directory: Option<PathBuf>,

    /// Output dependency graph in DOT format and exit (all targets)
    #[arg(long = "graph")]
    graph: bool,

    /// Output dependency graph for a specific target (implies --graph; DOT format)
    #[arg(long = "graph-of")]
    graph_of: Option<String>,

    /// Targets to build (default: first target in mkfile)
    targets: Vec<String>,
}

/// Simple % pattern matching (subset of graph.rs::match_metarule)
fn match_simple(target: &str, pattern: &str) -> Option<String> {
    if let Some(pos) = pattern.find('%') {
        let prefix = &pattern[..pos];
        let suffix = &pattern[pos + 1..];
        if target.starts_with(prefix) && target.ends_with(suffix) {
            let stem = &target[prefix.len()..target.len() - suffix.len()];
            return Some(stem.to_string());
        }
    }
    None
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
    let stmts = mk_rs_core::parse::parse(&tokens)?;

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
                        all_targets: r.targets.clone(),
                    },
                );
            }
        }
    }

    // Determine targets: CLI args or first target of first rule
    // For --graph mode, show all targets
    let target_names: Vec<String> = if cli.targets.is_empty() {
        if cli.graph || cli.graph_of.is_some() {
            // Collect all concrete (non-pattern) targets for graph
            stmts.iter()
                .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
                .flat_map(|r| r.targets.iter().cloned())
                .filter(|t| !t.contains('%') && !t.contains('&'))
                .collect()
        } else {
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
        }
    } else {
        cli.targets.clone()
    };

    // Read $NREP from the variable scope (default "1" via builtin_scope).
    // F-056: NREP limits metarule recursion depth.
    // Guard against NREP=0 (no expansion) — mk convention: NREP >= 1.
    let nrep = scope
        .get("NREP")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);

    // Build DAG
    let mut graph = build_graph_with_nrep(&stmts, &target_names, nrep)?;

    // --graph / --graph-of: output DOT and exit
    if cli.graph || cli.graph_of.is_some() {
        use mk_rs_core::graph::GraphScope;
        let scope = if cli.graph_of.is_some() {
            GraphScope::Subgraph
        } else {
            GraphScope::All
        };
        let dot = graph.to_dot(scope, cli.graph_of.as_deref());
        if !dot.is_empty() {
            println!("{}", dot);
        }
        return Ok(());
    }

    // Resolve metarule recipes for graph nodes without explicit rules
    for node in &graph.nodes {
        if !rules.contains_key(&node.name) {
            for stmt in &stmts {
                if let Stmt::Rule(r) = stmt {
                    if r.is_metarule && !r.is_regex {
                        for pat in &r.targets {
                            if !pat.contains('%') && !pat.contains('&') { continue; }
                            if match_simple(&node.name, pat).is_some() {
                                rules.insert(node.name.clone(), ResolvedRule {
                                    recipe: r.recipe.clone().unwrap_or_default(),
                                    attributes: r.attributes,
                                    all_targets: vec![node.name.clone()], // concrete target, not pattern %.o
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // MKSHELL: allow switching shell via env variable (F-053)
    let mkshell = scope.get("MKSHELL").unwrap_or("/bin/sh").to_string();

    // Select shell via $MKSHELL
    let shell: Box<dyn mk_rs_core::shell::Shell> = {
        if mkshell.contains("duckscript") || mkshell.ends_with(".ds") {
            #[cfg(not(feature = "duckscript"))]
            {
                return Err("mk: duckscript support not compiled in (rebuild with --features duckscript)".to_string().into());
            }
            #[cfg(feature = "duckscript")]
            {
                Box::new(DuckShell)
            }
        } else if mkshell == "/bin/sh" || mkshell == "sh" {
            Box::new(ShShell)
        } else {
            Box::new(CustomShell::new(&mkshell))
        }
    };

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

    // Resolve --color flag to a boolean
    let use_color = match cli.color.as_str() {
        "always" => true,
        "never" => false,
        _ => std::io::stderr().is_terminal(),
    };

    let opts = SchedOptions {
        keep_going: cli.keep_going,
        no_exec: cli.no_exec,
        explain: cli.explain,
        touch: cli.touch,
        silent: cli.silent,
        all: cli.all,
        nproc: 1, // sequential by default; $NPROC env var overrides
        force_intermediates: cli.force_intermediates,
        mkshell: shell.name().to_string(),
        mkflags,
        mkargs,
        color: use_color,
    };

    // Build environment from variable scope
    let env = scope.export();

    let working_dir = std::env::current_dir()?;

    let outcome = execute(&mut graph, &rules, shell.as_ref(), &working_dir, &env, &opts)?;

    // Print failures (only reachable with -k)
    if !outcome.failed.is_empty() {
        for (target, msg) in &outcome.failed {
            eprintln!("mk: {}: {}", target, msg);
        }
        std::process::exit(1);
    }

    Ok(())
}
