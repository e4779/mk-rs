//! DAG builder, cycle detector, and staleness checker.
//!
//! Phase 1a scope: concrete rules only, no metarules, sequential only.
//!
//! # Architecture
//!
//! ```text
//! parse::Stmt[] → build_graph() → Graph → stale_nodes() → Vec<NodeIndex>
//! ```
//!
//! - `build_graph` constructs the full transitive closure from requested targets.
//! - Cycle detection runs as a post-pass over the built graph.
//! - Staleness determines which targets need rebuilding.

use std::collections::HashMap;

use crate::error::GraphError;
use crate::parse::{Rule, Stmt};

// ── Index types ───────────────────────────────────────────────────────────

/// Index into the graph's node vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeIndex(pub usize);

/// Index into the graph's arc vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArcIndex(pub usize);

// ── Bitflags ──────────────────────────────────────────────────────────────

/// Bitflags for node state.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeFlags(u8);

impl NodeFlags {
    pub const VIRTUAL: u8 = 1 << 0;
    pub const MADE: u8 = 1 << 1;
    pub const FAILED: u8 = 1 << 2;
    pub const CYCLE: u8 = 1 << 3;
    pub const VISITED: u8 = 1 << 4;
    pub const PRETENDING: u8 = 1 << 5;

    pub fn is_virtual(&self) -> bool {
        self.0 & Self::VIRTUAL != 0
    }
    pub fn is_made(&self) -> bool {
        self.0 & Self::MADE != 0
    }
    pub fn is_failed(&self) -> bool {
        self.0 & Self::FAILED != 0
    }
    pub fn set(&mut self, flag: u8) {
        self.0 |= flag;
    }
    pub fn clear(&mut self, flag: u8) {
        self.0 &= !flag;
    }
}

// ── Graph data structures ─────────────────────────────────────────────────

/// A target in the dependency graph.
#[derive(Debug, Clone)]
pub struct Node {
    /// Target name (file path or virtual name).
    pub name: String,
    /// Modification time from filesystem. None = virtual target or not yet stat'd.
    pub mtime: Option<std::time::SystemTime>,
    /// State flags.
    pub flags: NodeFlags,
    /// Arcs where this node is the TARGET (incoming from prerequisites).
    pub arcs_in: Vec<ArcIndex>,
}

/// A dependency edge: prerequisite → target.
#[derive(Debug, Clone)]
pub struct Arc {
    /// Source node (prerequisite).
    pub from: NodeIndex,
    /// Destination node (target).
    pub to: NodeIndex,
    /// Stem from pattern match (empty for concrete rules).
    pub stem: String,
    /// Whether this arc came from a metarule.
    pub is_meta: bool,
}

/// The full dependency graph.
#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub arcs: Vec<Arc>,
    /// Which nodes are the requested targets.
    pub targets: Vec<NodeIndex>,
}

// ── Filesystem helper ─────────────────────────────────────────────────────

/// Read the filesystem modification time of a path.
/// Returns None if the path does not exist or is otherwise inaccessible.
fn get_mtime(path: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

// ── Graph builder ─────────────────────────────────────────────────────────

/// Build a DAG from parsed statements for the given target names.
///
/// Phase 1a: only concrete rules. Metarules and regex rules are skipped.
/// Simple transitive closure from requested targets.
///
/// Returns an error if a cycle is detected or a requested target has no rule
/// and does not exist on the filesystem.
pub fn build_graph(stmts: &[Stmt], target_names: &[String]) -> Result<Graph, GraphError> {
    // 1. Collect concrete rules, index by target
    let rules: Vec<&Rule> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Rule(r) if !r.is_metarule && !r.is_regex => Some(r),
            _ => None,
        })
        .collect();

    if rules.is_empty() {
        return Err(GraphError::NoRule {
            target: target_names
                .first()
                .cloned()
                .unwrap_or_else(|| "(none)".into()),
        });
    }

    // Index rules by target name (one rule may list multiple targets)
    let mut rules_by_target: HashMap<&str, Vec<&Rule>> = HashMap::new();
    for rule in &rules {
        for target in &rule.targets {
            rules_by_target
                .entry(target.as_str())
                .or_default()
                .push(rule);
        }
    }

    // 2. Resolve target list
    let targets: Vec<String> = if target_names.is_empty() {
        // Use first target of first rule
        vec![rules[0].targets[0].clone()]
    } else {
        target_names.to_vec()
    };

    // 3. Recursively build the DAG
    let mut graph = Graph {
        nodes: Vec::new(),
        arcs: Vec::new(),
        targets: Vec::new(),
    };
    let mut name_to_index: HashMap<String, NodeIndex> = HashMap::new();

    fn build_node<'a>(
        graph: &mut Graph,
        rules_by_target: &HashMap<&str, Vec<&'a Rule>>,
        name_to_index: &mut HashMap<String, NodeIndex>,
        name: &str,
    ) -> NodeIndex {
        if let Some(&idx) = name_to_index.get(name) {
            return idx;
        }

        let mtime = get_mtime(name);
        let idx = NodeIndex(graph.nodes.len());
        graph.nodes.push(Node {
            name: name.to_string(),
            mtime,
            flags: NodeFlags::default(),
            arcs_in: Vec::new(),
        });
        name_to_index.insert(name.to_string(), idx);

        if let Some(rules) = rules_by_target.get(name) {
            // Mark virtual if any rule for this target has the V attribute
            for rule in rules {
                if rule.attributes.is_virtual() {
                    graph.nodes[idx.0].flags.set(NodeFlags::VIRTUAL);
                    break;
                }
            }

            // Phase 1a: use first rule's prereqs only
            let rule = rules[0];
            for prereq in &rule.prereqs {
                let prereq_idx = build_node(graph, rules_by_target, name_to_index, prereq);
                let arc_idx = ArcIndex(graph.arcs.len());
                graph.nodes[idx.0].arcs_in.push(arc_idx);
                graph.arcs.push(Arc {
                    from: prereq_idx,
                    to: idx,
                    stem: String::new(),
                    is_meta: false,
                });
            }
        }

        idx
    }

    for target in &targets {
        let idx = build_node(&mut graph, &rules_by_target, &mut name_to_index, target);
        graph.targets.push(idx);
    }

    // 4. Validate requested targets (must have a rule or exist on fs)
    for &target_idx in &graph.targets {
        let node = &graph.nodes[target_idx.0];
        let has_rule = rules_by_target.contains_key(node.name.as_str());
        if !has_rule && node.mtime.is_none() {
            return Err(GraphError::NoRule {
                target: node.name.clone(),
            });
        }
    }

    // 5. Cycle detection
    detect_cycles(&mut graph)?;

    Ok(graph)
}

// ── Cycle detection ───────────────────────────────────────────────────────

/// DFS to detect back edges (cycles) in the graph.
fn detect_cycles(graph: &mut Graph) -> Result<(), GraphError> {
    for i in 0..graph.nodes.len() {
        if graph.nodes[i].flags.0 & NodeFlags::VISITED == 0 {
            let mut path = Vec::new();
            dfs_cycle_check(graph, NodeIndex(i), &mut path)?;
        }
    }
    Ok(())
}

fn dfs_cycle_check(
    graph: &mut Graph,
    current: NodeIndex,
    path: &mut Vec<NodeIndex>,
) -> Result<(), GraphError> {
    // Check if already in current path → cycle
    if graph.nodes[current.0].flags.0 & NodeFlags::CYCLE != 0 {
        // Build cycle chain description
        let cycle_start = path
            .iter()
            .position(|&idx| idx == current)
            .unwrap_or(path.len());
        let mut chain = String::new();
        for (i, &idx) in path[cycle_start..].iter().enumerate() {
            if i > 0 {
                chain.push_str(" -> ");
            }
            chain.push_str(&graph.nodes[idx.0].name);
        }
        chain.push_str(" -> ");
        chain.push_str(&graph.nodes[current.0].name);
        return Err(GraphError::Cycle { chain });
    }

    // Already fully visited → skip
    if graph.nodes[current.0].flags.0 & NodeFlags::VISITED != 0 {
        return Ok(());
    }

    // Mark as being visited (in current path)
    graph.nodes[current.0].flags.set(NodeFlags::CYCLE);
    path.push(current);

    // Recurse into prerequisites
    let prereq_indices: Vec<NodeIndex> = graph.nodes[current.0]
        .arcs_in
        .iter()
        .map(|&arc_idx| graph.arcs[arc_idx.0].from)
        .collect();
    for prereq_idx in prereq_indices {
        dfs_cycle_check(graph, prereq_idx, path)?;
    }

    // Mark fully visited, remove from current path
    path.pop();
    graph.nodes[current.0].flags.clear(NodeFlags::CYCLE);
    graph.nodes[current.0].flags.set(NodeFlags::VISITED);

    Ok(())
}

// ── Staleness checker ─────────────────────────────────────────────────────

/// Which targets need rebuilding?
///
/// A node is stale if:
/// - It is virtual and any prerequisite is stale.
/// - It does not exist on the filesystem (mtime is None for non-virtual).
/// - Any prerequisite has a newer mtime.
/// - Any prerequisite is itself stale (recursive).
///
/// Returns a Vec of stale node indices (deduplicated, topologically grouped).
pub fn stale_nodes(graph: &Graph) -> Vec<NodeIndex> {
    let n = graph.nodes.len();
    // Memoization: None = unvisited, Some(false) = not stale, Some(true) = stale
    let mut memo: Vec<Option<bool>> = vec![None; n];
    // Track visited-to-stale decisions to avoid re-adding duplicates
    let mut result: Vec<NodeIndex> = Vec::new();
    let mut in_result: Vec<bool> = vec![false; n];

    for &target_idx in &graph.targets {
        check_stale(graph, target_idx, &mut memo, &mut result, &mut in_result);
    }

    result
}

/// Recursively check whether `idx` is stale. Uses memoization.
fn check_stale(
    graph: &Graph,
    idx: NodeIndex,
    memo: &mut [Option<bool>],
    result: &mut Vec<NodeIndex>,
    in_result: &mut [bool],
) -> bool {
    // Return cached result
    if let Some(stale) = memo[idx.0] {
        return stale;
    }

    let node = &graph.nodes[idx.0];

    // Any stale prerequisite makes us stale
    let prereq_stale = node.arcs_in.iter().any(|&arc_idx| {
        let prereq_idx = graph.arcs[arc_idx.0].from;
        check_stale(graph, prereq_idx, memo, result, in_result)
    });

    let stale = if node.flags.is_virtual() {
        // Virtual: stale if any prereq is stale
        prereq_stale
    } else {
        // File target
        if node.mtime.is_none() {
            // Doesn't exist → stale
            true
        } else {
            // Check if any prereq is newer
            prereq_stale
                || node.arcs_in.iter().any(|&arc_idx| {
                    let prereq = &graph.nodes[graph.arcs[arc_idx.0].from.0];
                    match prereq.mtime {
                        Some(pmtime) => pmtime > node.mtime.unwrap(),
                        None => true, // prereq doesn't exist → target is stale
                    }
                })
        }
    };

    memo[idx.0] = Some(stale);
    if stale && !in_result[idx.0] {
        in_result[idx.0] = true;
        result.push(idx);
    }

    stale
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::{tokenize, ShellMode};
    use crate::parse;

    /// Helper: parse mkfile text and build graph.
    fn graph_from_str(input: &str, targets: &[&str]) -> Result<Graph, GraphError> {
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let target_names: Vec<String> = targets.iter().map(|s| s.to_string()).collect();
        build_graph(&stmts, &target_names)
    }

    // ── Graph construction ─────────────────────────────────────────────

    #[test]
    fn single_node_no_prereqs() {
        let g = graph_from_str("a:\n", &["a"]).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].name, "a");
    }

    #[test]
    fn two_node_chain() {
        let g = graph_from_str("a: b\nb:\n", &["a"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let a_idx = g.nodes.iter().position(|n| n.name == "a").unwrap();
        let b_idx = g.nodes.iter().position(|n| n.name == "b").unwrap();
        assert_eq!(g.nodes[a_idx].arcs_in.len(), 1);
        assert_eq!(g.arcs[g.nodes[a_idx].arcs_in[0].0].from, NodeIndex(b_idx));
    }

    #[test]
    fn diamond_dependency() {
        let input = "a: b c\nb: d\nc: d\nd:\n";
        let g = graph_from_str(input, &["a"]).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.arcs.len(), 4);
    }

    #[test]
    fn self_loop_cycle() {
        let result = graph_from_str("a: a\n", &["a"]);
        assert!(result.is_err());
    }

    #[test]
    fn indirect_cycle() {
        let result = graph_from_str("a: b\nb: c\nc: a\n", &["a"]);
        assert!(result.is_err());
    }

    #[test]
    fn no_rule_for_target() {
        let result = graph_from_str("a: b\n", &["nonexistent"]);
        assert!(result.is_err());
    }

    #[test]
    fn external_file_prereq() {
        let g = graph_from_str("a: b\n", &["a"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let b_idx = g.nodes.iter().position(|n| n.name == "b").unwrap();
        assert!(g.nodes[b_idx].arcs_in.is_empty());
    }

    #[test]
    fn virtual_target() {
        let input = "all:V: prog\nprog:\n";
        let g = graph_from_str(input, &["all"]).unwrap();
        let all = g.nodes.iter().find(|n| n.name == "all").unwrap();
        assert!(all.flags.is_virtual());
        assert!(all.mtime.is_none());
    }

    #[test]
    fn has_target_index() {
        let g = graph_from_str("a: b\n", &["a"]).unwrap();
        assert_eq!(g.targets.len(), 1);
        assert_eq!(g.nodes[g.targets[0].0].name, "a");
    }

    // ── Staleness ──────────────────────────────────────────────────────

    #[test]
    fn stale_nonexistent_target() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("mk_test_graph");
        let _ = std::fs::create_dir_all(&dir);
        let prereq_path = dir.join("source.txt");
        std::fs::write(&prereq_path, "hello").unwrap();

        let input = format!("target: {}\n", prereq_path.display());
        let g = graph_from_str(&input, &["target"]).unwrap();
        let stale = stale_nodes(&g);
        assert!(!stale.is_empty());
        assert!(stale.iter().any(|idx| g.nodes[idx.0].name == "target"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_prereq_newer() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("mk_test_stale");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        std::fs::write(&target_path, "old").unwrap();
        
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&prereq_path, "new").unwrap();

        let input = format!("{}: {}\n", target_path.display(), prereq_path.display());
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g);
        assert!(!stale.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn up_to_date() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("mk_test_uptodate");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        std::fs::write(&prereq_path, "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&target_path, "newer").unwrap();

        let input = format!("{}: {}\n", target_path.display(), prereq_path.display());
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g);
        assert!(stale.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
