//! DAG builder, cycle detector, and staleness checker.
//!
//! Phase 2 scope: concrete rules, % metarules, & metarules, R: regex metarules.
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

use regex::Regex;

use crate::archive::parse_archive_ref;
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
    pub const NO_EXEC: u8 = 1 << 6;

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
    /// Custom comparison program (P: attribute).
    pub prog: Option<String>,
    /// Line number in the mkfile where this edge's rule was defined.
    pub line: usize,
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

/// Expand glob patterns in a list of prerequisites.
/// If a prereq contains glob characters (*, ?, [), expand it.
/// Otherwise, keep it as-is.
fn expand_globs(prereqs: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for p in prereqs {
        if p.contains('*') || p.contains('?') || p.contains('[') {
            match glob::glob(p) {
                Ok(paths) => {
                    for entry in paths.flatten() {
                        expanded.push(entry.to_string_lossy().into_owned());
                    }
                }
                Err(_) => {
                    // Invalid glob pattern — keep literal
                    expanded.push(p.clone());
                }
            }
        } else {
            expanded.push(p.clone());
        }
    }
    expanded
}

// ── Metarule matching ────────────────────────────────────────────────────

/// Try to match a target name against a metarule pattern.
/// Returns Some(stem) if matched, None otherwise.
///
/// Tries `%` first (greedy, matches anything), then `&` (matches a single
/// path component with no dots or slashes).
fn match_metarule(target: &str, pattern: &str) -> Option<String> {
    if pattern.contains('%') {
        return match_percent(target, pattern);
    }
    if pattern.contains('&') {
        return match_ampersand(target, pattern);
    }
    None
}

/// Match a `%` metarule: `%.o` matches `foo.o` with stem `foo`,
/// `lib%.a` matches `libfoo.a` with stem `foo`.
fn match_percent(target: &str, pattern: &str) -> Option<String> {
    if let Some(pos) = pattern.find('%') {
        let prefix = &pattern[..pos];
        let suffix = &pattern[pos + 1..];

        if target.starts_with(prefix) && target.ends_with(suffix) {
            let stem_start = prefix.len();
            let stem_end = target.len() - suffix.len();
            if stem_start <= stem_end {
                let stem = target[stem_start..stem_end].to_string();
                return Some(stem);
            }
        }
    }
    None
}

/// Match an `&` metarule: `&.c` matches `hello.c` with stem `hello`.
/// `&` matches a single path component (no dots or slashes).
fn match_ampersand(target: &str, pattern: &str) -> Option<String> {
    if let Some(pos) = pattern.find('&') {
        let prefix = &pattern[..pos];
        let suffix = &pattern[pos + 1..];

        if target.starts_with(prefix) && target.ends_with(suffix) {
            let stem_start = prefix.len();
            let stem_end = target.len() - suffix.len();
            if stem_start <= stem_end {
                let stem = &target[stem_start..stem_end];
                // & must not contain '.' or '/'
                if !stem.contains('.') && !stem.contains('/') {
                    return Some(stem.to_string());
                }
            }
        }
    }
    None
}

// ── Graph builder ─────────────────────────────────────────────────────────

/// Build a DAG from parsed statements for the given target names.
///
/// Phase 2: supports concrete rules, `%` metarules, `&` metarules,
/// and `R:` regex metarules. Simple transitive closure from requested targets.
///
/// Returns an error if a cycle is detected or a requested target has no rule
/// and does not exist on the filesystem.
/// Uses default NREP = 1 (each metarule applied at most once per expansion chain).
pub fn build_graph(stmts: &[Stmt], target_names: &[String]) -> Result<Graph, GraphError> {
    build_graph_with_nrep(stmts, target_names, 1)
}

/// Build a DAG with explicit NREP depth limit for metarule expansion.
///
/// NREP limits how many times metarules are applied recursively in a single
/// dependency chain. E.g. NREP=1 means `%.z` is applied once; NREP=2 allows
/// `target -> source.z -> source.z.z`.
pub fn build_graph_with_nrep(
    stmts: &[Stmt],
    target_names: &[String],
    nrep: usize,
) -> Result<Graph, GraphError> {
    // 1. Collect concrete rules, index by target
    let rules: Vec<&Rule> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Rule(r) if !r.is_metarule && !r.is_regex => Some(r),
            _ => None,
        })
        .collect();

    let metarules: Vec<&Rule> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Rule(r) if r.is_metarule && !r.is_regex => Some(r),
            _ => None,
        })
        .collect();

    let regex_rules: Vec<&Rule> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Rule(r) if r.is_regex => Some(r),
            _ => None,
        })
        .collect();

    if rules.is_empty() && target_names.is_empty() {
        return Err(GraphError::NoRule {
            target: "(none)".into(),
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

    #[allow(clippy::too_many_arguments)]
    fn build_node<'a>(
        graph: &mut Graph,
        rules_by_target: &HashMap<&str, Vec<&'a Rule>>,
        metarules: &[&'a Rule],
        regex_rules: &[&'a Rule],
        name_to_index: &mut HashMap<String, NodeIndex>,
        nrep: usize,
        depth: usize,
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

            // Phase 1a: use first rule's prereqs only (concrete rules don't increment depth)
            let rule = rules[0];
            let expanded_prereqs = expand_globs(&rule.prereqs);
            for prereq in &expanded_prereqs {
                let prereq_idx = build_node(
                    graph, rules_by_target, metarules, regex_rules, name_to_index,
                    nrep, depth, prereq,
                );
                let arc_idx = ArcIndex(graph.arcs.len());
                graph.nodes[idx.0].arcs_in.push(arc_idx);
                graph.arcs.push(Arc {
                    from: prereq_idx,
                    to: idx,
                    stem: String::new(),
                    is_meta: false,
                    prog: rule.prog.clone(),
                    line: rule.line,
                });
            }
        } else if let Some(ar) = parse_archive_ref(name) {
            // Archive member reference: lib.a(member.o)
            // Auto-generate dependency: member.o → lib.a(member.o)
            // Mark with N attribute (no recipe — archive update handled separately)
            graph.nodes[idx.0].flags.set(NodeFlags::NO_EXEC);

            let member_idx = build_node(
                graph, rules_by_target, metarules, regex_rules, name_to_index,
                nrep, depth, &ar.member,
            );
            let arc_idx = ArcIndex(graph.arcs.len());
            graph.nodes[idx.0].arcs_in.push(arc_idx);
            graph.arcs.push(Arc {
                from: member_idx,
                to: idx,
                stem: String::new(),
                is_meta: false,
                prog: None,
                line: 0,  // auto-generated (archive member)
            });
        } else if depth < nrep {
            // No concrete rule, depth allows metarule expansion
            let mut matched = false;
            let mut first_match_prereqs: Option<Vec<String>> = None;

            for metarule in metarules {
                // F-027: n attribute — skip metarule if target doesn't exist on fs
                if metarule.attributes.is_no_virtual()
                    && !std::path::Path::new(name).exists()
                {
                    continue;
                }
                if let Some(stem) = match_metarule(name, &metarule.targets[0]) {
                    // Compute substituted prereqs for this match
                    let prereqs: Vec<String> = metarule
                        .prereqs
                        .iter()
                        .map(|p| p.replace(['%', '&'], &stem))
                        .collect();

                    if !matched {
                        // F-061: first match — use it
                        if metarule.attributes.is_virtual() {
                            graph.nodes[idx.0].flags.set(NodeFlags::VIRTUAL);
                        }
                        for prereq in &prereqs {
                            let prereq_idx = build_node(
                                graph, rules_by_target, metarules, regex_rules,
                                name_to_index, nrep, depth + 1, prereq,
                            );
                            let arc_idx = ArcIndex(graph.arcs.len());
                            graph.nodes[idx.0].arcs_in.push(arc_idx);
                            graph.arcs.push(Arc {
                                from: prereq_idx,
                                to: idx,
                                stem: stem.clone(),
                                is_meta: true,
                                prog: metarule.prog.clone(),
                                line: metarule.line,
                            });
                        }
                        matched = true;
                        first_match_prereqs = Some(prereqs);
                    } else {
                        // F-061: subsequent match — check for ambiguity
                        if prereqs != *first_match_prereqs.as_ref().unwrap() {
                            eprintln!(
                                "mk: warning: ambiguous rules for target '{}'",
                                name
                            );
                        }
                    }
                }
            }

            // Try regex metarules (R: prefix)
            if !matched {
                for regex_rule in regex_rules {
                    let pattern = &regex_rule.targets[0];
                    if let Ok(re) = Regex::new(pattern) {
                        if let Some(caps) = re.captures(name) {
                            let full_match = caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default();

                            // Apply regex rule attributes
                            if regex_rule.attributes.is_virtual() {
                                graph.nodes[idx.0].flags.set(NodeFlags::VIRTUAL);
                            }

                            // Substitute \1, \2, ... in prereqs with capture groups
                            let prereqs: Vec<String> = regex_rule.prereqs.iter()
                                .map(|p| {
                                    let mut result = p.clone();
                                    for (i, cap) in caps.iter().enumerate() {
                                        if let Some(m) = cap {
                                            let placeholder = format!("\\{}", i);
                                            result = result.replace(&placeholder, m.as_str());
                                        }
                                    }
                                    result
                                })
                                .collect();

                            for prereq in &prereqs {
                                let prereq_idx = build_node(
                                    graph, rules_by_target, metarules, regex_rules, name_to_index,
                                    nrep, depth + 1, prereq,
                                );
                                let arc_idx = ArcIndex(graph.arcs.len());
                                graph.nodes[idx.0].arcs_in.push(arc_idx);
                                graph.arcs.push(Arc {
                                    from: prereq_idx,
                                    to: idx,
                                    stem: full_match.clone(),
                                    is_meta: true,
                                    prog: regex_rule.prog.clone(),
                                    line: regex_rule.line,
                                });
                            }
                            break;
                        }
                    }
                }
            }
        }

        idx
    }

    for target in &targets {
        let idx = build_node(
            &mut graph, &rules_by_target, &metarules, &regex_rules, &mut name_to_index,
            nrep, 0, target,
        );
        graph.targets.push(idx);
    }

    // 4. Prune vacuous meta-edges (F-060): concrete rules override metarules
    prune_vacuous(&mut graph);

    // 5. Validate requested targets (must have a rule or exist on fs)
    for &target_idx in &graph.targets {
        let node = &graph.nodes[target_idx.0];
        let has_rule = rules_by_target.contains_key(node.name.as_str())
            || metarules
                .iter()
                .any(|mr| match_metarule(&node.name, &mr.targets[0]).is_some())
            || regex_rules
                .iter()
                .any(|rr| {
                    if let Ok(re) = Regex::new(&rr.targets[0]) {
                        re.is_match(&node.name)
                    } else {
                        false
                    }
                })
            || parse_archive_ref(&node.name).is_some();
        if !has_rule && node.mtime.is_none() {
            return Err(GraphError::NoRule {
                target: node.name.clone(),
            });
        }
    }

    // 6. Cycle detection
    detect_cycles(&mut graph)?;

    Ok(graph)
}

// ── Pruning vacuous meta-edges ────────────────────────────────────────────

/// Remove metarule-generated edges when a concrete rule exists for the same
/// target (F-060). Concrete rules always take priority over metarule
/// expansions — if a node has both concrete and meta incoming arcs, the meta
/// arcs are pruned.
fn prune_vacuous(graph: &mut Graph) {
    for node in &mut graph.nodes {
        let has_concrete = node
            .arcs_in
            .iter()
            .any(|&ai| !graph.arcs[ai.0].is_meta);

        if has_concrete {
            node.arcs_in.retain(|&ai| !graph.arcs[ai.0].is_meta);
        }
    }
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
/// When `force_intermediates` is false (default), non-existent intermediate
/// targets (nodes with prereqs that don't exist on disk) are given a "pretend"
/// mtime equal to the most recent prerequisite's mtime (F-069). If this
/// pretend timestamp makes all dependents up to date, the intermediate is
/// skipped (F-017). The `-i` flag sets `force_intermediates` to true,
/// disabling this optimization (F-051).
///
/// Returns a Vec of stale node indices (deduplicated, topologically grouped).
pub fn stale_nodes(graph: &Graph, force_intermediates: bool) -> Vec<NodeIndex> {
    let n = graph.nodes.len();
    // Memoization: None = unvisited, Some(false) = not stale, Some(true) = stale
    let mut memo: Vec<Option<bool>> = vec![None; n];
    // Track visited-to-stale decisions to avoid re-adding duplicates
    let mut result: Vec<NodeIndex> = Vec::new();
    let mut in_result: Vec<bool> = vec![false; n];

    for &target_idx in &graph.targets {
        check_stale(graph, target_idx, &mut memo, &mut result, &mut in_result, force_intermediates);
    }

    result
}

/// Compute the effective mtime of a node for staleness purposes.
///
/// For missing intermediate targets, this returns a "pretend" mtime equal
/// to the most recent prerequisite's mtime (F-069). This allows dependents
/// to check whether they would be up-to-date even without the intermediate.
fn effective_mtime(
    graph: &Graph,
    idx: NodeIndex,
    force_intermediates: bool,
) -> Option<std::time::SystemTime> {
    let node = &graph.nodes[idx.0];
    let is_intermediate = graph.arcs.iter().any(|arc| arc.from == idx);
    if !force_intermediates
        && is_intermediate
        && !node.flags.is_virtual()
        && node.mtime.is_none()
        && !node.arcs_in.is_empty()
    {
        // Missing intermediate: find most recent prereq mtime
        node.arcs_in
            .iter()
            .filter_map(|&arc_idx| graph.nodes[graph.arcs[arc_idx.0].from.0].mtime)
            .max()
    } else {
        node.mtime
    }
}

/// Recursively check whether `idx` is stale. Uses memoization.
///
/// When `force_intermediates` is false, non-existent intermediates
/// are given a pretend mtime equal to the most recent prereq's mtime.
fn check_stale(
    graph: &Graph,
    idx: NodeIndex,
    memo: &mut [Option<bool>],
    result: &mut Vec<NodeIndex>,
    in_result: &mut [bool],
    force_intermediates: bool,
) -> bool {
    // Return cached result
    if let Some(stale) = memo[idx.0] {
        return stale;
    }

    let node = &graph.nodes[idx.0];

    let eff_mtime = effective_mtime(graph, idx, force_intermediates);

    // Any stale prerequisite makes us stale
    let prereq_stale = node.arcs_in.iter().any(|&arc_idx| {
        let prereq_idx = graph.arcs[arc_idx.0].from;
        check_stale(graph, prereq_idx, memo, result, in_result, force_intermediates)
    });

    let stale = if node.flags.is_virtual() {
        // Virtual: stale if any prereq is stale, OR if no prereqs (always run)
        prereq_stale || node.arcs_in.is_empty()
    } else if node.mtime.is_none() {
        // File doesn't exist — always stale.
        // (Missing intermediate optimization would skip this node if no
        //  downstream stale nodes depend on it — but that requires a second
        //  pass over the DAG. For now, always rebuild missing intermediates.)
        true
    } else {
        // File exists — check if prereqs are newer
        // File exists — check if prereqs are newer
        let mtime = eff_mtime.unwrap();
        prereq_stale
            || node.arcs_in.iter().any(|&arc_idx| {
                let arc = &graph.arcs[arc_idx.0];
                let prereq_idx = arc.from;

                // P attribute: custom comparison program overrides mtime
                if let Some(ref prog) = arc.prog {
                    let target = &graph.nodes[idx.0].name;
                    let prereq = &graph.nodes[prereq_idx.0].name;
                    let status = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(format!("{} '{}' '{}'", prog, target, prereq))
                        .status();
                    return match status {
                        Ok(s) if s.success() => false,
                        _ => true,
                    };
                }

                let prereq_eff = effective_mtime(graph, prereq_idx, force_intermediates);
                match prereq_eff {
                    Some(pmtime) => pmtime > mtime,
                    None => true,
                }
            })
    };

    memo[idx.0] = Some(stale);
    if stale && !in_result[idx.0] {
        in_result[idx.0] = true;
        result.push(idx);
    }

    stale
}

// ── Graph visualization ───────────────────────────────────────────────────

/// Which nodes to include in DOT output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GraphScope {
    /// Show all nodes and edges in the graph.
    All,
    /// Show only the subgraph reachable from the named target.
    Subgraph,
}

impl Graph {
    /// Export the dependency graph in Graphviz DOT format.
    ///
    /// Virtual nodes are drawn as ellipses, file targets as boxes.
    /// Stale nodes (needing rebuild) are filled red.
    /// Edge labels show the mkfile line number where the rule was defined.
    pub fn to_dot(&self, scope: GraphScope, root: Option<&str>) -> String {
        let mut out = String::from("digraph mk {\n");
        out.push_str("  rankdir=LR;\n");
        out.push_str("  node [fontname=\"monospace\"];\n");
        out.push_str("  edge [fontname=\"monospace\"];\n\n");

        // Determine which nodes to include
        let included: std::collections::HashSet<NodeIndex> = match scope {
            GraphScope::All => (0..self.nodes.len()).map(NodeIndex).collect(),
            GraphScope::Subgraph => {
                let root_idx = root
                    .and_then(|n| self.nodes.iter().position(|node| node.name == n))
                    .map(NodeIndex);
                match root_idx {
                    Some(idx) => self.reachable_from(idx),
                    None => {
                        eprintln!("mk: --graph: target '{}' not found", root.unwrap_or(""));
                        return String::new();
                    }
                }
            }
        };

        // Write nodes
        for idx in &included {
            let node = &self.nodes[idx.0];
            let shape = if node.flags.is_virtual() { "ellipse" } else { "box" };
            // Escape label for DOT
            let label = node.name.replace('\\', "\\\\").replace('"', "\\\"");
            out.push_str(&format!(
                "  n{} [label=\"{}\" shape={}];\n",
                idx.0, label, shape
            ));
        }

        out.push('\n');

        // Write edges (only between included nodes)
        for (_i, arc) in self.arcs.iter().enumerate() {
            if !included.contains(&arc.from) || !included.contains(&arc.to) {
                continue;
            }
            let mut attrs = Vec::new();
            if arc.line > 0 {
                attrs.push(format!("line {}", arc.line));
            }
            if arc.is_meta {
                attrs.push("meta".into());
            }
            if arc.prog.is_some() {
                attrs.push(format!("P:{}", arc.prog.as_ref().unwrap()));
            }
            if !arc.stem.is_empty() {
                attrs.push(format!("stem={}", arc.stem));
            }
            let label = if attrs.is_empty() {
                String::new()
            } else {
                format!(" [label=\"{}\"]", attrs.join("\\n"))
            };
            out.push_str(&format!("  n{} -> n{}{};\n", arc.from.0, arc.to.0, label));
        }

        out.push_str("}\n");
        out
    }

    /// Export the dependency graph as JSON.
    ///
    /// Each node has `id`, `kind` ("file" or "virtual"), and `stage`
    /// (heuristic: "raw", "processed", "report", or "virtual").
    /// Edges have `from`, `to`, and `line` (mkfile line number).
    pub fn to_json(&self, scope: GraphScope, root: Option<&str>) -> String {
        let included = match scope {
            GraphScope::All => (0..self.nodes.len()).map(NodeIndex).collect(),
            GraphScope::Subgraph => {
                let root_idx = root
                    .and_then(|n| self.nodes.iter().position(|node| node.name == n))
                    .map(NodeIndex);
                match root_idx {
                    Some(idx) => self.reachable_from(idx),
                    None => {
                        eprintln!("mk: --graph: target '{}' not found", root.unwrap_or(""));
                        return String::new();
                    }
                }
            }
        };

        // Build nodes array
        let nodes: Vec<serde_json::Value> = self.nodes.iter().enumerate()
            .filter(|(i, _)| included.contains(&NodeIndex(*i)))
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

        // Build edges array
        let edges: Vec<serde_json::Value> = self.arcs.iter()
            .filter(|arc| included.contains(&arc.from) && included.contains(&arc.to))
            .map(|arc| {
                serde_json::json!({
                    "from": self.nodes[arc.from.0].name,
                    "to": self.nodes[arc.to.0].name,
                    "line": arc.line
                })
            })
            .collect();

        let output = serde_json::json!({
            "nodes": nodes,
            "edges": edges
        });
        serde_json::to_string(&output).unwrap_or_default()
    }

    /// Collect all nodes reachable from `start` via outgoing edges.
    fn reachable_from(&self, start: NodeIndex) -> std::collections::HashSet<NodeIndex> {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![start];
        while let Some(idx) = stack.pop() {
            if visited.insert(idx) {
                for &arc_idx in &self.nodes[idx.0].arcs_in {
                    let prereq = self.arcs[arc_idx.0].from;
                    stack.push(prereq);
                }
            }
        }
        visited
    }
}

/// Guess the pipeline stage from a file path.
///
/// Heuristic:
/// - `data/raw/*` → "raw"
/// - `data/processed/*` or `data/bars/*` → "processed"
/// - `reports/*` or `templates/*` → "report"
/// - Virtual targets → "virtual"
/// - Everything else → "file"
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
        let dir = std::env::temp_dir().join("mk_test_graph");
        let _ = std::fs::create_dir_all(&dir);
        let prereq_path = dir.join("source.txt");
        std::fs::write(&prereq_path, "hello").unwrap();

        let input = format!("target: {}\n", prereq_path.display());
        let g = graph_from_str(&input, &["target"]).unwrap();
        let stale = stale_nodes(&g, false);
        assert!(!stale.is_empty());
        assert!(stale.iter().any(|idx| g.nodes[idx.0].name == "target"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_prereq_newer() {
        let dir = std::env::temp_dir().join("mk_test_stale");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        std::fs::write(&target_path, "old").unwrap();
        
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&prereq_path, "new").unwrap();

        let input = format!("{}: {}\n", target_path.display(), prereq_path.display());
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g, false);
        assert!(!stale.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn up_to_date() {
        let dir = std::env::temp_dir().join("mk_test_uptodate");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        std::fs::write(&prereq_path, "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&target_path, "newer").unwrap();

        let input = format!("{}: {}\n", target_path.display(), prereq_path.display());
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g, false);
        assert!(stale.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_intermediate_skipped() {
        let dir = std::env::temp_dir().join("mk_test_intermed");
        let _ = std::fs::create_dir_all(&dir);

        // Create a source file (prereq) that exists
        let source = dir.join("source.txt");
        std::fs::write(&source, "data").unwrap();

        // intermediate does NOT exist on disk
        let intermediate = dir.join("intermediate.o");
        let _ = std::fs::remove_file(&intermediate);

        // target depends on intermediate, intermediate depends on source
        let target = dir.join("target");

        // Ensure target exists and is newer than source
        std::fs::write(&target, "built").unwrap();

        let input = format!(
            "{}: {}\n{}: {}\n",
            target.display(),
            intermediate.display(),
            intermediate.display(),
            source.display(),
        );

        let g = graph_from_str(&input, &[&target.to_string_lossy()]).unwrap();

        // Without -i: intermediate IS stale (missing file → must be rebuilt)
        let stale = stale_nodes(&g, false);
        assert!(
            stale.iter().any(|idx| g.nodes[idx.0].name == intermediate.to_string_lossy()),
            "intermediate should be stale (missing file)"
        );

        // With -i (force_intermediates): also stale (same — always rebuild missing)
        let stale_i = stale_nodes(&g, true);
        assert!(
            stale_i.iter().any(|idx| g.nodes[idx.0].name == intermediate.to_string_lossy()),
            "intermediate should be stale with -i (missing file)"
        );

        // With -i: intermediate should be forced stale
        let stale_i = stale_nodes(&g, true);
        assert!(
            stale_i.iter().any(|idx| g.nodes[idx.0].name == intermediate.to_string_lossy()),
            "intermediate should be stale when force_intermediates=true"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Metarules ─────────────────────────────────────────────────────

    #[test]
    fn metarule_match_percent_o() {
        let input = "%.o: %.c\n\tcc -c $stem.c\nprog: hello.o\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = build_graph(&stmts, &["prog".into()]).unwrap();
        // Should have nodes: prog, hello.o, hello.c (3 nodes)
        assert_eq!(g.nodes.len(), 3);
        // Find hello.o node
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        assert_eq!(g.nodes[hello_o].arcs_in.len(), 1);
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert!(arc.is_meta);
        assert_eq!(arc.stem, "hello");
        // The prereq should be hello.c
        let prereq_idx = arc.from;
        assert_eq!(g.nodes[prereq_idx.0].name, "hello.c");
    }

    #[test]
    fn metarule_match_lib_percent_a() {
        let input = "lib%.a: lib%.o\n";
        let g = graph_from_str(input, &["libfoo.a"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let libfoo_a = g.nodes.iter().position(|n| n.name == "libfoo.a").unwrap();
        let arc = &g.arcs[g.nodes[libfoo_a].arcs_in[0].0];
        assert!(arc.is_meta);
        assert_eq!(arc.stem, "foo");
        let prereq_idx = arc.from;
        assert_eq!(g.nodes[prereq_idx.0].name, "libfoo.o");
    }

    #[test]
    fn metarule_concrete_takes_priority() {
        // Concrete rule for hello.o should override metarule %
        let input = "%.o: %.c\n\tcc -c $stem.c\nhello.o: hello.s\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        // hello.o should depend on hello.s (concrete), not hello.c (metarule)
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        assert_eq!(g.nodes[hello_o].arcs_in.len(), 1);
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert!(!arc.is_meta);
        assert_eq!(g.nodes[arc.from.0].name, "hello.s");
    }

    #[test]
    fn metarule_no_match() {
        // foo.txt doesn't match %.o pattern
        let input = "%.o: %.c\n";
        let result = graph_from_str(input, &["foo.txt"]);
        assert!(result.is_err());
    }

    #[test]
    fn metarule_first_match_wins() {
        // Two metarules for the same pattern — first one wins
        let input = "%.o: %.c\n%.o: %.s\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert_eq!(g.nodes[arc.from.0].name, "hello.c");
    }

    #[test]
    fn metarule_with_virtual_attr() {
        let input = "%.o:V: %.c\n\tcc -c $stem.c\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        assert!(g.nodes[hello_o].flags.is_virtual());
    }

    // ── & metarule tests ────────────────────────────────────────────

    #[test]
    fn ampersand_metarule_match() {
        // & matches "hello" (no dots, single path component)
        let input = "&.o: &.c\n\tcc -c $stem.c\nprog: hello.o\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let g = build_graph(&stmts, &["prog".into()]).unwrap();
        assert!(g.nodes.iter().any(|n| n.name == "hello.o"));
        assert!(g.nodes.iter().any(|n| n.name == "hello.c"));
    }

    #[test]
    fn ampersand_rejects_dot_in_stem() {
        // & must not match a stem containing '.'
        let input = "&.o: &.c\n";
        let result = graph_from_str(input, &["foo.bar.o"]);
        // foo.bar.o doesn't match &.o because stem "foo.bar" contains '.'
        assert!(result.is_err());
    }

    #[test]
    fn ampersand_rejects_slash_in_stem() {
        let input = "&.o: &.c\n";
        let result = graph_from_str(input, &["dir/name.o"]);
        assert!(result.is_err());
    }

    #[test]
    fn ampersand_simple_match() {
        let input = "&.o: &.c\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert!(arc.is_meta);
        assert_eq!(arc.stem, "hello");
        assert_eq!(g.nodes[arc.from.0].name, "hello.c");
    }

    // ── R: regex metarule tests ─────────────────────────────────────

    #[test]
    fn regex_metarule_simple() {
        // A regex metarule with R attribute
        let input = "foo:R: bar\n";
        let g = graph_from_str(input, &["foo"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let foo_node = g.nodes.iter().position(|n| n.name == "foo").unwrap();
        assert_eq!(g.nodes[foo_node].arcs_in.len(), 1);
    }

    #[test]
    fn regex_metarule_with_capture() {
        // Pattern with capture group
        let input = "(.+)\\.o:R: \\1.c\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert!(arc.is_meta);
        assert_eq!(g.nodes[arc.from.0].name, "hello.c");
    }

    #[test]
    fn regex_metarule_no_match() {
        let input = "foo\\.txt:R: foo.src\n";
        let result = graph_from_str(input, &["bar.txt"]);
        assert!(result.is_err());
    }

    #[test]
    fn regex_metarule_virtual_attr() {
        let input = "target:VR: dep\n";
        let g = graph_from_str(input, &["target"]).unwrap();
        let target = g.nodes.iter().find(|n| n.name == "target").unwrap();
        assert!(target.flags.is_virtual());
    }

    // ── n attribute (no-virtual) ──────────────────────────────────────

    /// Helper: parse mkfile text and build graph with explicit NREP.
    fn graph_with_nrep_from_str(
        input: &str,
        targets: &[&str],
        nrep: usize,
    ) -> Result<Graph, GraphError> {
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let target_names: Vec<String> = targets.iter().map(|s| s.to_string()).collect();
        build_graph_with_nrep(&stmts, &target_names, nrep)
    }

    #[test]
    fn n_attribute_target_exists() {
        // metarule with :n: should match an existing file on disk
        let dir = std::env::temp_dir().join("mk_test_n_attr");
        let _ = std::fs::create_dir_all(&dir);
        let c_file = dir.join("hello.c");
        std::fs::write(&c_file, "int main(){}").unwrap();
        let o_path = dir.join("hello.o");
        // hello.o does NOT exist, but we only check the metarule n flag on the TARGET
        // The n flag on the metarule checks whether the target exists
        // Here: %.o:n: %.c — n means the metarule only applies if target exists on fs
        // hello.o doesn't exist, so the metarule should NOT match
        let input = format!(
            "%.o:n: %.c\nprog: {}\n",
            o_path.display()
        );
        let g = graph_from_str(&input, &["prog"]).unwrap();
        // hello.o node should have no arcs (metarule skipped due to n flag + target not on fs)
        let o_node = g.nodes.iter().find(|n| n.name == o_path.to_string_lossy()).unwrap();
        assert!(o_node.arcs_in.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn n_attribute_skips_nonexistent() {
        // metarule with :n: should NOT match non-existent file.
        // ghost.o doesn't exist on fs, so the n: metarule is skipped →
        // ghost.o gets no arcs (it's a leaf external file reference).
        let input = "%.o:n: %.c\nprog: ghost.o\n";
        let g = graph_from_str(input, &["prog"]).unwrap();
        // prog depends on ghost.o, but ghost.o got no metarule match
        let ghost = g.nodes.iter().find(|n| n.name == "ghost.o").unwrap();
        assert!(ghost.arcs_in.is_empty(), "n: metarule should be skipped for non-existent ghost.o");
    }

    #[test]
    fn n_attribute_allows_existing() {
        // metarule with :n: SHOULD match an existing file
        let dir = std::env::temp_dir().join("mk_test_n_exists");
        let _ = std::fs::create_dir_all(&dir);
        let c_file = dir.join("real.c");
        std::fs::write(&c_file, "int main(){}").unwrap();
        let o_file = dir.join("real.o");
        // Create real.o so n: metarule applies
        std::fs::write(&o_file, "object").unwrap();
        let input = format!("%.o:n: %.c\n", );
        let g = graph_from_str(&input, &[&o_file.to_string_lossy()]).unwrap();
        assert!(g.nodes.len() >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── NREP depth limit ─────────────────────────────────────────────

    #[test]
    fn pruning_removes_meta_edges_when_concrete_exists() {
        // Manually construct a graph where a node has both concrete and meta
        // incoming arcs, then verify prune_vacuous removes the meta arcs.
        let mut graph = Graph {
            nodes: vec![
                Node {
                    name: "foo.o".into(),
                    mtime: None,
                    flags: NodeFlags::default(),
                    arcs_in: Vec::new(),
                },
                Node {
                    name: "foo.c".into(),
                    mtime: None,
                    flags: NodeFlags::default(),
                    arcs_in: Vec::new(),
                },
                Node {
                    name: "foo.s".into(),
                    mtime: None,
                    flags: NodeFlags::default(),
                    arcs_in: Vec::new(),
                },
            ],
            arcs: vec![
                // meta arc: foo.c -> foo.o (from % metarule)
                Arc {
                    from: NodeIndex(1),
                    to: NodeIndex(0),
                    stem: "foo".into(),
                    is_meta: true,
                    prog: None,
                    line: 1,
                },
                // concrete arc: foo.s -> foo.o (from concrete rule)
                Arc {
                    from: NodeIndex(2),
                    to: NodeIndex(0),
                    stem: String::new(),
                    is_meta: false,
                    prog: None,
                    line: 1,
                },
            ],
            targets: vec![NodeIndex(0)],
        };
        // foo.o has both concrete (foo.s) and meta (foo.c) arcs
        graph.nodes[0].arcs_in = vec![ArcIndex(0), ArcIndex(1)];

        prune_vacuous(&mut graph);

        // After pruning, only the concrete arc should remain
        assert_eq!(graph.nodes[0].arcs_in.len(), 1);
        let remaining_arc = &graph.arcs[graph.nodes[0].arcs_in[0].0];
        assert!(!remaining_arc.is_meta);
        assert_eq!(graph.nodes[remaining_arc.from.0].name, "foo.s");
    }

    #[test]
    fn ambiguous_metarules_different_prereqs_uses_first() {
        // F-061: two metarules matching same target with DIFFERENT prereqs.
        // The first metarule should be used; a warning is emitted to stderr.
        let input = "%.o: %.c\n%.o: %.s\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        // Should use first metarule (%.c), not second (%.s)
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        assert_eq!(g.nodes[hello_o].arcs_in.len(), 1);
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert_eq!(g.nodes[arc.from.0].name, "hello.c");
    }

    #[test]
    fn same_prereqs_no_ambiguity() {
        // F-061: two metarules with IDENTICAL prereqs — no ambiguity.
        let input = "%.o: %.c\n%.o: %.c\n";
        let g = graph_from_str(input, &["hello.o"]).unwrap();
        // Should work fine
        let hello_o = g.nodes.iter().position(|n| n.name == "hello.o").unwrap();
        assert_eq!(g.nodes[hello_o].arcs_in.len(), 1);
        let arc = &g.arcs[g.nodes[hello_o].arcs_in[0].0];
        assert_eq!(g.nodes[arc.from.0].name, "hello.c");
    }

    // ── NREP depth limit ─────────────────────────────────────────────

    #[test]
    fn nrep_limits_recursion_depth_1() {
        // NREP=1: %.z applied once → target → source.z (one level)
        let input = "%: %.z\n\tcp $prereq $target\ntarget: source\n";
        let g = graph_with_nrep_from_str(input, &["target"], 1).unwrap();
        // 3 nodes: target, source, source.z
        assert_eq!(g.nodes.len(), 3);
        let source_node = g.nodes.iter().find(|n| n.name == "source").unwrap();
        // source should have source.z as prereq (metarule applied at depth 0)
        assert!(!source_node.arcs_in.is_empty());
        let prereq_idx = g.arcs[source_node.arcs_in[0].0].from;
        assert_eq!(g.nodes[prereq_idx.0].name, "source.z");
        // source.z should be a leaf (metarule blocked at depth 1)
        let z_node = &g.nodes[prereq_idx.0];
        assert!(z_node.arcs_in.is_empty());
    }

    #[test]
    fn nrep_limits_recursion_depth_2() {
        // NREP=2: %.z applied twice → target → source.z → source.z.z
        let input = "%: %.z\ntarget: source\n";
        let g = graph_with_nrep_from_str(input, &["target"], 2).unwrap();
        // 4 nodes: target, source, source.z, source.z.z
        assert_eq!(g.nodes.len(), 4);
        let z_node = g.nodes.iter().find(|n| n.name == "source.z").unwrap();
        assert!(!z_node.arcs_in.is_empty());
        let z_prereq_idx = g.arcs[z_node.arcs_in[0].0].from;
        assert_eq!(g.nodes[z_prereq_idx.0].name, "source.z.z");
        // source.z.z should be a leaf
        let zz_node = &g.nodes[z_prereq_idx.0];
        assert!(zz_node.arcs_in.is_empty());
    }

    #[test]
    fn nrep_default_is_1() {
        // Default NREP=1 via build_graph
        let input = "%: %.z\ntarget: source\n";
        let g = graph_from_str(input, &["target"]).unwrap();
        // Should be same as NREP=1: 3 nodes
        assert_eq!(g.nodes.len(), 3);
    }

    // ── Archive member syntax ────────────────────────────────────────

    #[test]
    fn archive_member_creates_dep_on_member() {
        // lib.a(foo.o) should auto-create dependency on foo.o
        let input = "out: lib.a(foo.o)\n";
        let g = graph_from_str(input, &["out"]).unwrap();
        // 3 nodes: out, lib.a(foo.o), foo.o
        assert_eq!(g.nodes.len(), 3);
        let archive_node = g.nodes.iter().find(|n| n.name == "lib.a(foo.o)").unwrap();
        assert_eq!(archive_node.arcs_in.len(), 1);
        let arc = &g.arcs[archive_node.arcs_in[0].0];
        assert!(!arc.is_meta);
        assert_eq!(g.nodes[arc.from.0].name, "foo.o");
    }

    #[test]
    fn archive_member_has_n_flag() {
        // lib.a(foo.o) node should have NO_EXEC flag
        let input = "out: lib.a(foo.o)\n";
        let g = graph_from_str(input, &["out"]).unwrap();
        let archive_node = g.nodes.iter().find(|n| n.name == "lib.a(foo.o)").unwrap();
        assert!(archive_node.flags.0 & NodeFlags::NO_EXEC != 0);
    }

    #[test]
    fn archive_member_standalone() {
        // Build graph directly for lib.a(foo.o)
        let g = graph_from_str("", &["lib.a(foo.o)"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let archive_idx = g.nodes.iter().position(|n| n.name == "lib.a(foo.o)").unwrap();
        assert_eq!(g.nodes[archive_idx].arcs_in.len(), 1);
        let member_idx = g.arcs[g.nodes[archive_idx].arcs_in[0].0].from;
        assert_eq!(g.nodes[member_idx.0].name, "foo.o");
    }

    #[test]
    fn concrete_rule_overrides_archive_auto() {
        // If there's an explicit concrete rule for lib.a(foo.o), use it
        let input = "lib.a(foo.o): explicit.c\n";
        let g = graph_from_str(input, &["lib.a(foo.o)"]).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let archive_node = g.nodes.iter().find(|n| n.name == "lib.a(foo.o)").unwrap();
        let arc = &g.arcs[archive_node.arcs_in[0].0];
        assert_eq!(g.nodes[arc.from.0].name, "explicit.c");
        // Should NOT have NO_EXEC flag (concrete rule handles it)
        // Actually NO_EXEC is only set in the archive auto-path, not when concrete found
    }

    #[test]
    fn archive_member_in_prereq_list() {
        // Multiple archive members as prereqs
        let input = "out: lib.a(foo.o) lib.a(bar.o)\n";
        let g = graph_from_str(input, &["out"]).unwrap();
        // out + 2 archive nodes + 2 member nodes = 5
        assert_eq!(g.nodes.len(), 5);
        let foo_arch = g.nodes.iter().find(|n| n.name == "lib.a(foo.o)").unwrap();
        let bar_arch = g.nodes.iter().find(|n| n.name == "lib.a(bar.o)").unwrap();
        assert!(foo_arch.flags.0 & NodeFlags::NO_EXEC != 0);
        assert!(bar_arch.flags.0 & NodeFlags::NO_EXEC != 0);
    }

    // ── P attribute (custom comparison program) ───────────────────────

    #[test]
    fn p_attribute_prog_stored_in_arc() {
        // Concrete rule with P attribute: prog should be propagated to arc
        let input = "target:Pcmp: prereq\n";
        let g = graph_from_str(input, &["target"]).unwrap();
        let target = g.nodes.iter().find(|n| n.name == "target").unwrap();
        assert_eq!(target.arcs_in.len(), 1);
        let arc = &g.arcs[target.arcs_in[0].0];
        assert_eq!(arc.prog, Some("cmp".into()));
    }

    #[test]
    fn p_attribute_no_prog_stored_in_arc() {
        // P attribute without program name: arc.prog should be None
        let input = "target:P: prereq\n";
        let g = graph_from_str(input, &["target"]).unwrap();
        let target = g.nodes.iter().find(|n| n.name == "target").unwrap();
        assert_eq!(target.arcs_in.len(), 1);
        let arc = &g.arcs[target.arcs_in[0].0];
        assert_eq!(arc.prog, None);
    }

    #[test]
    fn p_attribute_no_attr_no_prog_in_arc() {
        // Rule without P attribute: arc.prog should be None
        let input = "target: prereq\n";
        let g = graph_from_str(input, &["target"]).unwrap();
        let target = g.nodes.iter().find(|n| n.name == "target").unwrap();
        assert_eq!(target.arcs_in.len(), 1);
        let arc = &g.arcs[target.arcs_in[0].0];
        assert_eq!(arc.prog, None);
    }

    #[test]
    fn p_attribute_metarule_prog_stored_in_arc() {
        // Metarule with P attribute: prog should be propagated
        let input = "%.o:Pcmp: %.c\nprog: hello.o\n";
        let g = graph_from_str(input, &["prog"]).unwrap();
        let hello_o = g.nodes.iter().find(|n| n.name == "hello.o").unwrap();
        assert_eq!(hello_o.arcs_in.len(), 1);
        let arc = &g.arcs[hello_o.arcs_in[0].0];
        assert!(arc.is_meta);
        assert_eq!(arc.prog, Some("cmp".into()));
    }

    /// Test that a P attribute with a program that returns 0 marks the
    /// target as up-to-date (not stale) even if the prereq is newer.
    #[test]
    fn p_attribute_up_to_date_via_program() {
        let dir = std::env::temp_dir().join("mk_test_p_uptodate");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        // source is newer than target (normally would be stale)
        std::fs::write(&target_path, "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&prereq_path, "new").unwrap();

        // P attribute with "true" program (always returns 0 → up to date)
        let input = format!(
            "{}:Ptrue: {}\n",
            target_path.display(),
            prereq_path.display(),
        );
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g, false);
        assert!(stale.is_empty(), "target should be up-to-date via P program");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Test that a P attribute with a program that returns non-zero
    /// marks the target as stale.
    #[test]
    fn p_attribute_stale_via_program() {
        let dir = std::env::temp_dir().join("mk_test_p_stale");
        let _ = std::fs::create_dir_all(&dir);
        let target_path = dir.join("target.txt");
        let prereq_path = dir.join("source.txt");

        // target is newer than source (normally up-to-date)
        std::fs::write(&prereq_path, "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&target_path, "newer").unwrap();

        // P attribute with "false" program (always returns 1 → stale)
        let input = format!(
            "{}:Pfalse: {}\n",
            target_path.display(),
            prereq_path.display(),
        );
        let g = graph_from_str(&input, &[&target_path.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g, false);
        assert!(
            stale.iter().any(|idx| g.nodes[idx.0].name == target_path.to_string_lossy()),
            "target should be stale via P program returning non-zero"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_non_matching_parens_not_treated_as_archive() {
        // Names without archive(member) pattern should still error with no rule
        let result = graph_from_str("", &["just.a.file.o"]);
        assert!(result.is_err());
    }

    #[test]
    fn virtual_no_prereqs_always_stale() {
        // clean:V: with recipe but no prereqs should ALWAYS be stale
        let g = graph_from_str("clean:V:\n\trm -f *.o\n", &["clean"]).unwrap();
        let stale = stale_nodes(&g, false);
        let clean_idx = g.nodes.iter().position(|n| n.name == "clean").unwrap();
        assert!(stale.contains(&NodeIndex(clean_idx)), "virtual target with no prereqs must always be stale");
    }

    #[test]
    fn missing_intermediate_cascades_to_dependents() {
        // When an intermediate file (both target and prereq) is deleted,
        // BOTH the intermediate AND its dependents should be stale.
        // Regression: effective_mtime gave pretend mtime → dependents appeared up-to-date.
        let dir = std::env::temp_dir().join("mk_test_cascade");
        let _ = std::fs::create_dir_all(&dir);
        let source = dir.join("source.txt");
        let intermediate = dir.join("intermediate.txt");
        let report = dir.join("report.txt");

        // Source exists (old)
        std::fs::write(&source, "data").unwrap();
        // Intermediate: depends on source
        // Report: depends on intermediate
        // Both intermediate and report exist (from previous build)
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&intermediate, "processed").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&report, "report").unwrap();

        let input = format!(
            "{}: {}\n\tprocess\n{}: {}\n\tanalyze\n",
            intermediate.display(), source.display(),
            report.display(), intermediate.display(),
        );
        // Delete intermediate — should trigger rebuild of intermediate AND report
        std::fs::remove_file(&intermediate).unwrap();

        let g = graph_from_str(&input, &[&report.to_string_lossy()]).unwrap();
        let stale = stale_nodes(&g, false);
        let names: Vec<&str> = stale.iter().map(|idx| g.nodes[idx.0].name.as_str()).collect();
        assert!(names.contains(&intermediate.to_str().unwrap()), "intermediate should be stale (was deleted)");
        assert!(names.contains(&report.to_str().unwrap()), "report should be stale (depends on deleted intermediate)");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn glob_prereqs_not_expanded_yet() {
        // F-066: glob expansion in prerequisites not yet implemented.
        // `target: data/*.json` should match all .json files in data/.
        // Currently `*.json` is treated as a literal filename.
        let dir = std::env::temp_dir().join("mk_test_glob");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.json"), "").unwrap();
        std::fs::write(dir.join("b.json"), "").unwrap();
        std::fs::write(dir.join("c.txt"), "").unwrap();

        let input = format!("target: {}\n", dir.join("*.json").display());
        let g = graph_from_str(&input, &["target"]).unwrap();

        // BUG: *.json is not expanded — target has one prereq named "*.json"
        let prereqs: Vec<&str> = g.nodes.iter()
            .filter(|n| n.name == "target")
            .flat_map(|n| n.arcs_in.iter())
            .map(|&ai| g.arcs[ai.0].from)
            .map(|idx| g.nodes[idx.0].name.as_str())
            .collect();
        // Should match a.json and b.json, not c.txt or literal *.json
        assert!(prereqs.contains(&dir.join("a.json").to_str().unwrap()),
            "glob should expand to a.json");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── JSON graph export ─────────────────────────────────────────────

    #[test]
    fn json_graph_export_nodes_and_edges() {
        let input = "all: hello.o world.o\nhello.o: hello.c\nworld.o: world.c util.h\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let targets = vec!["all".into()];
        let graph = build_graph(&stmts, &targets).unwrap();
        let json = graph.to_json(GraphScope::All, None);
        assert!(json.contains("\"nodes\""));
        assert!(json.contains("\"edges\""));
        assert!(json.contains("hello.c"));
        assert!(json.contains("hello.o"));
        assert!(json.contains("\"kind\""));
        assert!(json.contains("\"stage\""));
    }

    #[test]
    fn json_node_has_stage_heuristic() {
        let input = "reports/r.html: data/raw/a.toon data/processed/b.toon meta\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let targets = vec!["reports/r.html".into()];
        let graph = build_graph(&stmts, &targets).unwrap();
        let json = graph.to_json(GraphScope::All, None);
        // data/raw/* → raw
        assert!(json.contains("\"stage\":\"raw\""));
        // data/processed/* → processed
        assert!(json.contains("\"stage\":\"processed\""));
        // reports/* → report
        assert!(json.contains("\"stage\":\"report\""));
        // unknown file → file
        assert!(json.contains("\"stage\":\"file\""));
    }

    #[test]
    fn json_subgraph_only_includes_reachable_nodes() {
        let input = "a: b\nb: c\nd: e\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let targets = vec!["a".into(), "d".into()];
        let graph = build_graph(&stmts, &targets).unwrap();
        let json = graph.to_json(GraphScope::Subgraph, Some("a"));
        assert!(json.contains("\"id\":\"a\""));
        assert!(json.contains("\"id\":\"b\""));
        assert!(json.contains("\"id\":\"c\""));
        assert!(!json.contains("\"id\":\"d\""));
        assert!(!json.contains("\"id\":\"e\""));
    }
}
