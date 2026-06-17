//! Standalone dependency graph visualization and diagnosis tool for mk-rs.
//!
//! Reads a mkfile and outputs its dependency graph in multiple formats
//! (ASCII, Mermaid, DOT, JSON). Supports dead-end detection, orphan
//! prerequisite listing, and structural checks. Kept as a separate binary
//! from `mk` to avoid bloating the build tool with serde and visualization
//! dependencies.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use clap::Parser;
use mk_rs_core::graph::{self, GraphScope};
use mk_rs_core::lex::{tokenize, ShellMode};
use mk_rs_core::parse::{self, Stmt};

/// Output format for graph visualization.
#[derive(Clone, Debug, clap::ValueEnum)]
enum Format {
    /// ASCII art with box-drawing characters (default; terminal-friendly).
    Ascii,
    /// Mermaid `graph` block — renders inline on GitHub/GitLab/Obsidian.
    Mermaid,
    /// Graphviz DOT — pipe to `dot -Tsvg`.
    Dot,
    /// JSON — programmatic consumption.
    Json,
}

/// Visualize and check the dependency graph of an mkfile.
///
/// Outputs the graph in ASCII art (default), Mermaid, DOT, or JSON.
/// ASCII/mermaid are for reading; DOT pipes to graphviz (`mk-graph --dot | dot -Tsvg`).
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

    /// Output format (default: ascii)
    #[arg(long = "format", short = 'F', value_enum, default_value_t = Format::Ascii)]
    format: Format,

    /// Shorthand for --format=json
    #[arg(long = "json", conflicts_with = "format")]
    json: bool,

    /// Shorthand for --format=dot
    #[arg(long = "dot", conflicts_with = "format")]
    dot: bool,

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
    let text =
        std::fs::read_to_string(&cli.file).map_err(|e| format!("{}: {}", cli.file.display(), e))?;

    // Parse
    let tokens = tokenize(&text, ShellMode::Sh)?;
    let stmts = parse::parse(&tokens)?;

    // Build recipe lookup: target_name → recipe_text
    let recipes: HashMap<String, String> = stmts
        .iter()
        .filter_map(|s| if let Stmt::Rule(r) = s { Some(r) } else { None })
        .filter_map(|r| r.recipe.clone().map(|rec| (r.targets[0].clone(), rec)))
        .collect();

    // Build graph with all concrete targets
    let target_names: Vec<String> = stmts
        .iter()
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

    let format = if cli.json {
        Format::Json
    } else if cli.dot {
        Format::Dot
    } else {
        cli.format.clone()
    };

    let out = match format {
        Format::Ascii => to_ascii(&graph, scope, cli.target.as_deref()),
        Format::Mermaid => to_mermaid(&graph, scope, cli.target.as_deref()),
        Format::Dot => graph.to_dot(scope, cli.target.as_deref()),
        Format::Json => to_json(&graph, &recipes, scope, cli.target.as_deref()),
    };
    if !out.is_empty() {
        // ascii-dag pads with blank lines for layout symmetry; trim them.
        let trimmed = match format {
            Format::Ascii => out.trim_end().to_string(),
            _ => out,
        };
        println!("{}", trimmed);
    }

    Ok(())
}

// ── Dead-end detection ──────────────────────────────────────────────────

/// Find targets that are produced (appear as `to` in an edge) but never
/// consumed (never appear as `from` in any edge). These are "output-only"
/// nodes — they exist in the graph but nothing depends on them.
fn find_dead_ends(graph: &graph::Graph) -> Vec<String> {
    let consumed: HashSet<graph::NodeIndex> = graph.arcs.iter().map(|a| a.from).collect();

    let produced: HashSet<graph::NodeIndex> = graph.arcs.iter().map(|a| a.to).collect();

    produced
        .difference(&consumed)
        .map(|idx| graph.nodes[idx.0].name.clone())
        .collect()
}

// ── Orphan detection ─────────────────────────────────────────────────────

/// Find prerequisites that are needed (appear as `from` in an edge) but
/// are never produced (never appear as `to` in any edge) and have no mtime.
/// These are inputs that don't exist and can't be built.
fn find_orphans(graph: &graph::Graph) -> Vec<String> {
    let produced: HashSet<graph::NodeIndex> = graph.arcs.iter().map(|a| a.to).collect();

    let needed: HashSet<graph::NodeIndex> = graph.arcs.iter().map(|a| a.from).collect();

    // Orphan: needed as prereq, never produced, not on disk
    needed
        .difference(&produced)
        .filter(|idx| graph.nodes[idx.0].mtime.is_none())
        .map(|idx| graph.nodes[idx.0].name.clone())
        .collect()
}

// ── ASCII art export (ascii-dag) ─────────────────────────────────────────

/// Render the graph as terminal-friendly ASCII art via the `ascii-dag` crate.
///
/// Edges flow prerequisite → target (as in DOT output). Edge labels show
/// `meta` for metarule-derived arcs and the mkfile line number otherwise.
fn to_ascii(graph: &graph::Graph, scope: GraphScope, root: Option<&str>) -> String {
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

    let mut dag = ascii_dag::Graph::new();
    // Node/edge labels are owned here and outlive `dag`. ascii-dag borrows them
    // via &'a str, so we materialize strings before construction.
    let node_labels: Vec<Option<String>> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if included.contains(&graph::NodeIndex(i)) {
                Some(if n.flags.is_virtual() {
                    format!("{}:V", n.name)
                } else {
                    n.name.clone()
                })
            } else {
                None
            }
        })
        .collect();
    let edge_labels: Vec<String> = graph
        .arcs
        .iter()
        .map(|a| edge_label(a).unwrap_or_default())
        .collect();

    for (i, lbl) in node_labels.iter().enumerate() {
        if let Some(l) = lbl {
            dag.add_node(i, l.as_str());
        }
    }
    for (i, arc) in graph.arcs.iter().enumerate() {
        if node_labels[arc.from.0].is_some() && node_labels[arc.to.0].is_some() {
            let lbl = if edge_labels[i].is_empty() {
                None
            } else {
                Some(edge_labels[i].as_str())
            };
            dag.add_edge(arc.from.0, arc.to.0, lbl);
        }
    }
    dag.render()
}

// ── Mermaid export ───────────────────────────────────────────────────────

/// Render the graph as a ```mermaid graph LR``` block.
///
/// Renders inline on GitHub, GitLab, Obsidian, and most LLM chat UIs.
/// Virtual targets use the `{{...}}` stadium shape; file targets use `[...]`.
fn to_mermaid(graph: &graph::Graph, scope: GraphScope, root: Option<&str>) -> String {
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

    // Mermaid node ids can't contain spaces or some special chars; use n{index}.
    let mut out = String::from("```mermaid\ngraph LR\n");
    for (i, node) in graph.nodes.iter().enumerate() {
        if !included.contains(&graph::NodeIndex(i)) {
            continue;
        }
        let id = format!("n{}", i);
        let label = mermaid_escape(&node.name);
        if node.flags.is_virtual() {
            out.push_str(&format!("  {}({{{}}})\n", id, label));
        } else {
            out.push_str(&format!("  {}[{}]\n", id, label));
        }
    }
    for arc in &graph.arcs {
        if !included.contains(&arc.from) || !included.contains(&arc.to) {
            continue;
        }
        match edge_label(arc) {
            Some(lbl) => out.push_str(&format!(
                "  n{} -- \"{}\" n{}\n",
                arc.from.0,
                mermaid_escape(&lbl),
                arc.to.0
            )),
            None => out.push_str(&format!("  n{} --> n{}\n", arc.from.0, arc.to.0)),
        }
    }
    out.push_str("```");
    out
}

/// Compact edge label: `meta` for metarule arcs, else `L{line}` when known.
/// Returns None for plain concrete-rule edges without notable metadata.
fn edge_label(arc: &graph::Arc) -> Option<String> {
    if arc.is_meta {
        Some("meta".to_string())
    } else if arc.line > 0 {
        Some(format!("L{}", arc.line))
    } else {
        None
    }
}

/// Escape a label for Mermaid: wrap in quotes if it contains special chars.
/// Mermaid treats `[`, `]`, `(`, `)`, `{`, `}`, `"` as syntax.
fn mermaid_escape(s: &str) -> String {
    let needs_quote = s.contains(['[', ']', '(', ')', '{', '}', '"', ' ']);
    if needs_quote {
        format!("\"{}\"", s.replace('"', "#quot;"))
    } else {
        s.to_string()
    }
}

// ── JSON export ──────────────────────────────────────────────────────────

/// Export graph as JSON with stage heuristic and recipe text.
fn to_json(
    graph: &graph::Graph,
    recipes: &HashMap<String, String>,
    scope: GraphScope,
    root: Option<&str>,
) -> String {
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

    let nodes: Vec<serde_json::Value> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| included.contains(&graph::NodeIndex(*i)))
        .map(|(_, node)| {
            let kind = if node.flags.is_virtual() {
                "virtual"
            } else {
                "file"
            };
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

    let edges: Vec<serde_json::Value> = graph
        .arcs
        .iter()
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
fn reachable_from_nodes(
    graph: &graph::Graph,
    start: graph::NodeIndex,
) -> HashSet<graph::NodeIndex> {
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
        let recipes: HashMap<String, String> = stmts
            .iter()
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
        let recipes: HashMap<String, String> = stmts
            .iter()
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

    #[test]
    fn ascii_render_contains_node_names_and_edge_labels() {
        let input = "prog: main.o\nmain.o: main.c\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["prog".into()]).unwrap();
        let ascii = to_ascii(&g, GraphScope::All, None);
        assert!(
            ascii.contains("main.c"),
            "ascii should list node names: {ascii}"
        );
        assert!(ascii.contains("prog"));
        // ascii-dag uses box-drawing glyphs to connect nodes
        assert!(
            ascii.contains('[') && ascii.contains(']'),
            "ascii should bracket node labels: {ascii}"
        );
    }

    #[test]
    fn ascii_marks_virtual_targets() {
        let input = "all:V: prog\nprog:\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["all".into()]).unwrap();
        let ascii = to_ascii(&g, GraphScope::All, None);
        assert!(
            ascii.contains("all:V"),
            "virtual target should be suffixed with :V: {ascii}"
        );
    }

    #[test]
    fn mermaid_is_valid_block_with_edges() {
        let input = "prog: main.o util.o\nmain.o: main.c\nutil.o: util.c\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["prog".into()]).unwrap();
        let m = to_mermaid(&g, GraphScope::All, None);
        assert!(
            m.starts_with("```mermaid\n"),
            "should open fenced block: {m}"
        );
        assert!(m.ends_with("```"), "should close fenced block: {m}");
        assert!(m.contains("graph LR"));
        assert!(
            m.contains("-- ") || m.contains("-->"),
            "should have at least one edge: {m}"
        );
        assert!(m.contains("main.o"));
    }

    #[test]
    fn mermaid_uses_hexagon_shape_for_virtual() {
        let input = "all:V: prog\nprog:\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = graph::build_graph(&stmts, &["all".into()]).unwrap();
        let m = to_mermaid(&g, GraphScope::All, None);
        assert!(
            m.contains("({"),
            "virtual target should use {{...}} shape: {m}"
        );
    }
}
