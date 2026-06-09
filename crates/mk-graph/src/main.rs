// mk-graph: Dependency graph visualizer for mk-rs.
//
// Reads a mkfile and outputs its dependency graph in JSON or DOT format.
// Separate from `mk` to keep the core build tool lean.
//
// Usage:
//   mk-graph                              # DOT to stdout
//   mk-graph --json                       # JSON to stdout
//   mk-graph --target <name>              # subgraph for target
//   mk-graph -f <mkfile> --json           # specific mkfile

use std::collections::HashSet;
use std::path::PathBuf;

use clap::Parser;
use mk_rs_core::graph::{self, GraphScope};
use mk_rs_core::lex::{tokenize, ShellMode};
use mk_rs_core::parse;

/// Visualize the dependency graph of an mkfile.
///
/// Outputs the graph in DOT format (default) or JSON.
/// Pipe to graphviz for rendering: mk-graph | dot -Tsvg > graph.svg
#[derive(Parser)]
#[command(version, about)]
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

    // Build graph with all concrete targets
    let target_names: Vec<String> = stmts.iter()
        .filter_map(|s| if let parse::Stmt::Rule(r) = s { Some(r) } else { None })
        .flat_map(|r| r.targets.iter().cloned())
        .filter(|t| !t.contains('%') && !t.contains('&'))
        .collect();

    let graph = graph::build_graph(&stmts, &target_names)?;

    // Output
    let scope = if cli.target.is_some() {
        GraphScope::Subgraph
    } else {
        GraphScope::All
    };

    if cli.json {
        println!("{}", to_json(&graph, scope, cli.target.as_deref()));
    } else {
        println!("{}", graph.to_dot(scope, cli.target.as_deref()));
    }

    Ok(())
}

// ── JSON export ──────────────────────────────────────────────────────────

/// Export graph as JSON with stage heuristic.
fn to_json(graph: &graph::Graph, scope: GraphScope, root: Option<&str>) -> String {
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
            serde_json::json!({
                "id": node.name,
                "kind": kind,
                "stage": stage
            })
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
        let targets = vec!["all".into()];
        let g = graph::build_graph(&stmts, &targets).unwrap();
        let json = to_json(&g, GraphScope::All, None);
        assert!(json.contains("\"nodes\""));
        assert!(json.contains("\"edges\""));
        assert!(json.contains("hello.c"));
    }
}
