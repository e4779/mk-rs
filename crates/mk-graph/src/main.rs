// mk-graph: Dependency graph visualizer for mk-rs.
//
// Reads a mkfile and outputs its dependency graph in JSON or DOT format.
// Separate from `mk` to keep the core build tool lean.
//
// Usage:
//   mk-graph                              # DOT to stdout
//   mk-graph --json                       # JSON to stdout
//   mk-graph --dead-ends                  # list dead-end targets
//   mk-graph --orphans                    # list orphan prerequisites
//   mk-graph --check                      # run all checks

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use clap::Parser;
use mk_rs_core::graph::{self, GraphScope};
use mk_rs_core::lex::{tokenize, ShellMode};
use mk_rs_core::parse::{self, Stmt};

/// Visualize and check the dependency graph of an mkfile.
///
/// Outputs the graph in DOT format (default) or JSON.
/// Pipe to graphviz: mk-graph | dot -Tsvg > graph.svg
/// Check mode: mk-graph --check
#[derive(Parser)]
#[command(
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    about
)]
struct Cli {
    /// Mkfile to read (default: mkfile)
    #[arg(short = 'f', default_value = "mkfile")]
    file: PathBuf,

    /// Output JSON instead of DOT
    #[arg(long = "json")]
    json: bool,

    /// Show subgraph for a specific target
    #[arg(long = "target", short = 't')]
    target: Option<String>,

    /// List targets that are produced but never consumed
    #[arg(long = "dead-ends")]
    dead_ends: bool,

    /// List prerequisites that are needed but have no rule and don't exist
    #[arg(long = "orphans")]
    orphans: bool,

    /// Run all checks: dead-ends + orphans
    #[arg(long = "check")]
    check: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("mk-graph: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Read mkfile
    let text = std::fs::read_to_string(&cli.file)
        .map_err(|e| format!("{}: {}", cli.file.display(), e))?;

    // Parse
    let tokens = tokenize(&text, ShellMode::Sh)?;
    let stmts = parse::parse(&tokens)?;

    // Build recipe lookup: target_name → recipe_text
    let recipes: HashMap<String, String> = stmts.iter()
        .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
        .filter_map(|r| r.recipe.clone().map(|rec| (r.targets[0].clone(), rec)))
        .collect();

    // Build graph with all concrete targets
    let target_names: Vec<String> = stmts.iter()
        .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
        .flat_map(|r| r.targets.iter().cloned())
        .filter(|t| !t.contains('%') && !t.contains('&'))
        .collect();

    let graph = graph::build_graph(&stmts, &target_names)?;

    // ── Check modes ──────────────────────────────────────────────────
    let do_dead = cli.dead_ends || cli.check;
    let do_orphans = cli.orphans || cli.check;
    let check_mode = do_dead || do_orphans;

    let mut errors = 0;

    if do_dead {
        let dead = find_dead_ends(&graph);
        if dead.is_empty() {
            eprintln!("mk-graph: no dead-end targets found");
        } else {
            eprintln!("mk-graph: {} dead-end target(s):", dead.len());
            for name in &dead {
                eprintln!("  {}", name);
            }
            errors += dead.len();
        }
    }

    if do_orphans {
        let orph = find_orphans(&graph);
        if orph.is_empty() {
            eprintln!("mk-graph: no orphan prerequisites found");
        } else {
            eprintln!("mk-graph: {} orphan prerequisite(s):", orph.len());
            for name in &orph {
                eprintln!("  {}", name);
            }
            errors += orph.len();
        }
    }

    if check_mode {
        if errors > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    // ── Graph output ─────────────────────────────────────────────────
    let scope = if cli.target.is_some() {
        GraphScope::Subgraph
    } else {
        GraphScope::All
    };

    if cli.json {
        println!("{}", to_json(&graph, &recipes, scope, cli.target.as_deref()));
    } else {
        println!("{}", graph.to_dot(scope, cli.target.as_deref()));
    }

    Ok(())
}

// ── Dead-end detection ──────────────────────────────────────────────────

/// Find targets that are produced (appear as `to` in an edge) but never
/// consumed (never appear as `from` in any edge). These are "output-only"
/// nodes — they exist in the graph but nothing depends on them.
fn find_dead_ends(graph: &graph::Graph) -> Vec<String> {
    let consumed: HashSet<graph::NodeIndex> = graph.arcs.iter()
        .map(|a| a.from)
        .collect();

    let produced: HashSet<graph::NodeIndex> = graph.arcs.iter()
        .map(|a| a.to)
        .collect();

    produced.difference(&consumed)
        .map(|idx| graph.nodes[idx.0].name.clone())
        .collect()
}

// ── Orphan detection ─────────────────────────────────────────────────────

/// Find prerequisites that are needed (appear as `from` in an edge) but
/// are never produced (never appear as `to` in any edge) and have no mtime.
/// These are inputs that don't exist and can't be built.
fn find_orphans(graph: &graph::Graph) -> Vec<String> {
    let produced: HashSet<graph::NodeIndex> = graph.arcs.iter()
        .map(|a| a.to)
        .collect();

    let needed: HashSet<graph::NodeIndex> = graph.arcs.iter()
        .map(|a| a.from)
        .collect();

    // Orphan: needed as prereq, never produced, not on disk
    needed.difference(&produced)
        .filter(|idx| graph.nodes[idx.0].mtime.is_none())
        .map(|idx| graph.nodes[idx.0].name.clone())
        .collect()
}

// ── JSON export ──────────────────────────────────────────────────────────

/// Export graph as JSON with stage heuristic and recipe text.
fn to_json(graph: &graph::Graph, recipes: &HashMap<String, String>, scope: GraphScope, root: Option<&str>) -> String {
    let included: HashSet<graph::NodeIndex> = match scope {
        GraphScope::All => (0..graph.nodes.len()).map(graph::NodeIndex).collect(),
        GraphScope::Subgraph => {
            let root_idx = root
                .and_then(|n| graph.nodes.iter().position(|node| node.name == n))
                .map(graph::NodeIndex);
            match root_idx {
                Some(idx) => reachable_from_nodes(graph, idx),
                None => {
                    eprintln!("mk-graph: target '{}' not found", root.unwrap_or(""));
                    return String::new();
                }
            }
        }
    };

    let nodes: Vec<serde_json::Value> = graph.nodes.iter().enumerate()
        .filter(|(i, _)| included.contains(&graph::NodeIndex(*i)))
        .map(|(_, node)| {
            let kind = if node.flags.is_virtual() { "virtual" } else { "file" };
            let stage = stage_heuristic(&node.name, node.flags.is_virtual());
            let recipe = recipes.get(&node.name);
            let mut obj = serde_json::json!({
                "id": node.name,
                "kind": kind,
                "stage": stage
            });
            if let Some(rec) = recipe {
                obj["recipe"] = serde_json::Value::String(rec.clone());
            }
            obj
        })
        .collect();

    let edges: Vec<serde_json::Value> = graph.arcs.iter()
        .filter(|arc| included.contains(&arc.from) && included.contains(&arc.to))
        .map(|arc| {
            serde_json::json!({
                "from": graph.nodes[arc.from.0].name,
                "to": graph.nodes[arc.to.0].name,
                "line": arc.line
            })
        })
        .collect();

    let output = serde_json::json!({ "nodes": nodes, "edges": edges });
    serde_json::to_string(&output).unwrap_or_default()
}

/// Collect all nodes reachable from `start` via outgoing edges.
fn reachable_from_nodes(graph: &graph::Graph, start: graph::NodeIndex) -> HashSet<graph::NodeIndex> {
    let mut visited = HashSet::new();
    let mut stack = vec![start];
    while let Some(idx) = stack.pop() {
        if visited.insert(idx) {
            for &arc_idx in &graph.nodes[idx.0].arcs_in {
                let prereq = graph.arcs[arc_idx.0].from;
                stack.push(prereq);
            }
        }
    }
    visited
}

/// Guess the pipeline stage from a file path.
fn stage_heuristic(name: &str, is_virtual: bool) -> &'static str {
    if is_virtual {
        return "virtual";
    }
    if name.starts_with("data/raw/") {
        "raw"
    } else if name.starts_with("data/processed/") || name.starts_with("data/bars/") {
        "processed"
    } else if name.starts_with("reports/") || name.starts_with("templates/") {
        "report"
    } else {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_export_has_nodes_and_edges() {
        let input = "all: hello.o world.o\nhello.o: hello.c\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let recipes: HashMap<String, String> = stmts.iter()
            .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
            .filter_map(|r| r.recipe.clone().map(|rec| (r.targets[0].clone(), rec)))
            .collect();
        let targets = vec!["all".into()];
        let g = graph::build_graph(&stmts, &targets).unwrap();
        let json = to_json(&g, &recipes, GraphScope::All, None);
        assert!(json.contains("\"nodes\""));
        assert!(json.contains("\"edges\""));
        assert!(json.contains("hello.c"));
    }

    #[test]
    fn json_includes_recipe_text() {
        let input = "all:V: hello.c\n\techo build\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let recipes: HashMap<String, String> = stmts.iter()
            .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
            .filter_map(|r| r.recipe.clone().map(|rec| (r.targets[0].clone(), rec)))
            .collect();
        let targets = vec!["all".into()];
        let g = graph::build_graph(&stmts, &targets).unwrap();
        let json = to_json(&g, &recipes, GraphScope::All, None);
        assert!(json.contains("echo build"));
    }

    #[test]
    fn dead_ends_detects_output_only() {
        let input = "a: b\nb:\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["a".into()]).unwrap();
        let dead = find_dead_ends(&g);
        assert!(dead.contains(&"a".to_string()));
        assert!(!dead.contains(&"b".to_string()));
    }

    #[test]
    fn orphans_detects_missing_prereqs() {
        let input = "a: b c\nd:\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["a".into(), "d".into()]).unwrap();
        let orph = find_orphans(&g);
        // b and c have no rule and don't exist → virtual → orphans
        assert!(orph.iter().any(|s| s == "b"));
        assert!(orph.iter().any(|s| s == "c"));
    }
}
