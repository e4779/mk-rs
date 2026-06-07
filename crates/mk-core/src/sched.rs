//! Build scheduler — orchestrates DAG traversal and recipe execution.
//!
//! Phase 1a: sequential execution only (NPROC=1). No metarules. No parallelism.
//!
//! # Architecture
//!
//! ```text
//! Graph + ResolvedRule[] → topological_sort() → execute() → BuildOutcome
//! ```
//!
//! - `topological_sort` orders stale nodes so prerequisites build first.
//! - `execute` walks the sorted nodes and dispatches to `run_recipe`.

use crate::attr::Attributes;
use crate::error::SchedError;
use crate::graph::{Graph, NodeIndex, stale_nodes};
use crate::recipe::{Recipe, RecipeOptions, run as run_recipe};
use crate::shell::{Shell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ── Outcome ────────────────────────────────────────────────────────────────

/// Outcome of a build.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// Targets that were successfully built.
    pub built: Vec<String>,
    /// Targets that were already up to date.
    pub unchanged: Vec<String>,
    /// Targets that failed, with error message.
    pub failed: Vec<(String, String)>,
}

// ── Options ────────────────────────────────────────────────────────────────

/// Scheduler options (CLI flags that affect scheduling behavior).
#[derive(Debug, Clone)]
pub struct SchedOptions {
    /// -k: keep going after errors
    pub keep_going: bool,
    /// -n: no-exec (print only)
    pub no_exec: bool,
    /// -e: explain why recipes run
    pub explain: bool,
    /// -t: touch targets instead of executing
    pub touch: bool,
    /// -s: silent (don't print recipes)
    pub silent: bool,
    /// -a: assume all targets are out of date
    pub all: bool,
    /// -i: force missing intermediate targets to be built (F-017, F-051)
    pub force_intermediates: bool,
}

impl Default for SchedOptions {
    fn default() -> Self {
        Self {
            keep_going: false,
            no_exec: false,
            explain: false,
            touch: false,
            silent: false,
            all: false,
            force_intermediates: false,
        }
    }
}

// ── Resolved rule ──────────────────────────────────────────────────────────

/// A resolved rule: target has these prereqs, this recipe, these attributes.
#[derive(Debug, Clone)]
pub struct ResolvedRule {
    pub recipe: String,
    pub attributes: Attributes,
}

// ── Topological sort ───────────────────────────────────────────────────────

/// Topological sort using recursive post-order DFS.
///
/// Returns nodes in dependency order: leaves first, root targets last.
/// This is the correct execution order — prerequisites must be built before
/// the targets that depend on them.
fn topological_sort(graph: &Graph, targets: &[NodeIndex]) -> Vec<NodeIndex> {
    let mut visited: HashSet<usize> = HashSet::new();
    let mut order: Vec<NodeIndex> = Vec::new();

    fn visit(
        graph: &Graph,
        idx: NodeIndex,
        visited: &mut HashSet<usize>,
        order: &mut Vec<NodeIndex>,
    ) {
        if !visited.insert(idx.0) {
            return;
        }
        for &arc_idx in &graph.nodes[idx.0].arcs_in {
            visit(graph, graph.arcs[arc_idx.0].from, visited, order);
        }
        order.push(idx);
    }

    for target in targets {
        visit(graph, *target, &mut visited, &mut order);
    }

    // Post-order: leaves pushed first, roots last. Correct for execution.
    order
}

// ── Recipe construction ────────────────────────────────────────────────────

/// Build a Recipe struct from a graph node and resolved rule.
///
/// Extracts stem from metarule arcs for `$stem` variable expansion.
fn build_recipe(
    graph: &Graph,
    node_idx: NodeIndex,
    rule: &ResolvedRule,
    working_dir: &PathBuf,
    env: &HashMap<String, String>,
) -> Recipe {
    let node = &graph.nodes[node_idx.0];
    let prereqs: Vec<String> = node
        .arcs_in
        .iter()
        .map(|&arc_idx| {
            let arc = &graph.arcs[arc_idx.0];
            graph.nodes[arc.from.0].name.clone()
        })
        .collect();

    // Extract stem from metarule arc (if any)
    let stem = node
        .arcs_in
        .iter()
        .filter_map(|&arc_idx| {
            let arc = &graph.arcs[arc_idx.0];
            if arc.is_meta && !arc.stem.is_empty() {
                Some(arc.stem.clone())
            } else {
                None
            }
        })
        .next();

    Recipe {
        target: node.name.clone(),
        prereqs,
        script: rule.recipe.clone(),
        working_dir: working_dir.clone(),
        env: env.clone(),
        attributes: rule.attributes,
        stem,
        all_targets: vec![node.name.clone()],
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Execute the build plan for stale nodes in the given graph.
///
/// Walks nodes in topological order, executing recipes sequentially.
/// Returns outcome with built/unchanged/failed lists.
pub fn execute(
    graph: &mut Graph,
    rules: &HashMap<String, ResolvedRule>,
    shell: &dyn Shell,
    working_dir: &PathBuf,
    env: &HashMap<String, String>,
    opts: &SchedOptions,
) -> Result<BuildOutcome, SchedError> {
    // 1. Get stale nodes
    let stale = stale_nodes(graph, opts.force_intermediates);

    // Build a set for O(1) membership check
    let mut stale_set: HashSet<usize> = stale.iter().map(|idx| idx.0).collect();

    // stale_nodes uses Iterator::any which short-circuits:
    // if a virtual/intermediate node's first prereq is stale,
    // sibling prereqs may never be visited.
    // Fix up: any file target with no mtime (doesn't exist) is stale.
    let sorted = topological_sort(graph, &graph.targets);
    for &node_idx in &sorted {
        let node = &graph.nodes[node_idx.0];
        if !stale_set.contains(&node_idx.0)
            && !node.flags.is_virtual()
            && node.mtime.is_none()
        {
            stale_set.insert(node_idx.0);
        }
    }

    // -a: assume all targets are out of date
    if opts.all {
        stale_set.clear();
        for node_idx in &sorted {
            stale_set.insert(node_idx.0);
        }
    }

    // 3. If no stale nodes, everything is unchanged
    if stale_set.is_empty() {
        let unchanged: Vec<String> = sorted
            .iter()
            .map(|idx| graph.nodes[idx.0].name.clone())
            .collect();
        return Ok(BuildOutcome {
            built: Vec::new(),
            unchanged,
            failed: Vec::new(),
        });
    }

    // 4. Build recipe options from sched options
    let recipe_opts = RecipeOptions {
        no_exec: opts.no_exec,
        explain: opts.explain,
        touch: opts.touch,
        silent: opts.silent,
    };

    let mut built: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    for &node_idx in &sorted {
        let node = &graph.nodes[node_idx.0];

        // Skip non-stale nodes (they're up to date)
        if !stale_set.contains(&node_idx.0) {
            unchanged.push(node.name.clone());
            continue;
        }

        // Virtual target with no recipe → mark as built (phony target)
        if node.flags.is_virtual() {
            let has_recipe = rules
                .get(&node.name)
                .map(|r| !r.recipe.is_empty())
                .unwrap_or(false);
            if !has_recipe {
                built.push(node.name.clone());
                graph.nodes[node_idx.0].flags.set(crate::graph::NodeFlags::MADE);
                continue;
            }
        }

        // Find the recipe for this target
        let rule = match rules.get(&node.name) {
            Some(r) => r,
            None => {
                // No rule for this target — it's a leaf/source file, skip
                unchanged.push(node.name.clone());
                continue;
            }
        };

        // Skip targets with empty recipes (leaf files that happen to need building)
        if rule.recipe.is_empty() {
            unchanged.push(node.name.clone());
            continue;
        }

        // Build the Recipe and execute
        let recipe = build_recipe(graph, node_idx, rule, working_dir, env);

        match run_recipe(&recipe, shell, &recipe_opts) {
            Ok(_result) => {
                built.push(node.name.clone());
                graph.nodes[node_idx.0].flags.set(crate::graph::NodeFlags::MADE);
            }
            Err(e) => {
                let msg = e.to_string();
                if opts.keep_going {
                    failed.push((node.name.clone(), msg));
                } else {
                    return Err(SchedError::BuildFailed);
                }
            }
        }
    }

    Ok(BuildOutcome {
        built,
        unchanged,
        failed,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Graph, build_graph};
    use crate::lex::{tokenize, ShellMode};
    use crate::parse;
    use crate::shell::ShellResult;
    use std::path::{Path, PathBuf};

    // ── Test shell ─────────────────────────────────────────────────────

    /// Test shell: succeeds for echo, fails for "exit 1".
    struct TestShell;

    impl Shell for TestShell {
        fn name(&self) -> &str {
            "test"
        }

        fn execute(
            &self,
            recipe: &str,
            _env: &HashMap<String, String>,
            _dir: &Path,
        ) -> Result<ShellResult, crate::error::ShellError> {
            // Note: recipe text here is after elide_first_char(), which
            // strips one char per line. The parser already strips the
            // indent, so elide_first_char strips the first content char.
            // "exit 1" becomes "xit 1". Match the elided form.
            if recipe.contains("xit 1") {
                Ok(ShellResult {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: "fail".into(),
                })
            } else {
                Ok(ShellResult {
                    exit_code: 0,
                    stdout: recipe.into(),
                    stderr: String::new(),
                })
            }
        }

        fn find_unescaped(&self, _input: &str, _ch: char) -> Vec<usize> {
            vec![]
        }

        fn quote(&self, token: &str) -> String {
            token.to_string()
        }
    }

    // ── Test helpers ───────────────────────────────────────────────────

    /// Parse a mkfile string and build a graph + rules map.
    fn build_from_mkfile(
        mkfile: &str,
        target: &str,
    ) -> (Graph, HashMap<String, ResolvedRule>) {
        let tokens = tokenize(mkfile, ShellMode::Sh).unwrap();
        let stmts = parse::parse(&tokens).unwrap();
        let graph = build_graph(&stmts, &[target.to_string()]).unwrap();

        let mut rules = HashMap::new();
        for stmt in &stmts {
            if let parse::Stmt::Rule(r) = &stmt {
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
        (graph, rules)
    }

    // ── execute() tests ────────────────────────────────────────────────

    #[test]
    fn execute_single_target() {
        let (mut graph, rules) =
            build_from_mkfile("hello:\n\techo hello\n", "hello");
        let shell = TestShell;
        let outcome = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &SchedOptions::default(),
        )
        .unwrap();
        assert_eq!(outcome.built, vec!["hello"]);
    }

    #[test]
    fn execute_no_exec() {
        let (mut graph, rules) =
            build_from_mkfile("target:\n\techo hello\n", "target");
        let shell = TestShell;
        let opts = SchedOptions {
            no_exec: true,
            ..Default::default()
        };
        let outcome = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &opts,
        )
        .unwrap();
        assert_eq!(outcome.built, vec!["target"]);
    }

    #[test]
    fn execute_keep_going() {
        // "all" depends on a and b; b's recipe fails
        let mkfile = "all:V: a b\n\techo all\na:\n\techo a\n\nb:\n\texit 1\n";
        let (mut graph, rules) = build_from_mkfile(mkfile, "all");
        let shell = TestShell;
        let opts = SchedOptions {
            keep_going: true,
            ..Default::default()
        };
        let outcome = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &opts,
        )
        .unwrap();
        assert!(outcome.built.contains(&"a".to_string()));
        assert!(outcome.failed.iter().any(|(t, _)| t == "b"));
    }

    #[test]
    fn topological_sort_leaves_first() {
        let mkfile = "target: a b\na: leaf1\nb: leaf2\n";
        let (graph, _rules) = build_from_mkfile(mkfile, "target");
        let sorted = topological_sort(&graph, &graph.targets);
        let names: Vec<&str> =
            sorted.iter().map(|i| graph.nodes[i.0].name.as_str()).collect();
        let target_pos = names.iter().position(|&n| n == "target").unwrap();
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        let leaf1_pos = names.iter().position(|&n| n == "leaf1").unwrap();
        assert!(leaf1_pos < a_pos);
        assert!(a_pos < target_pos);
    }

    #[test]
    fn virtual_target_built() {
        let mkfile = "all:V: prog\nprog:\n\techo building\n";
        let (mut graph, rules) = build_from_mkfile(mkfile, "all");
        let shell = TestShell;
        let outcome = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &SchedOptions::default(),
        )
        .unwrap();
        assert!(outcome.built.contains(&"all".to_string()));
        assert!(outcome.built.contains(&"prog".to_string()));
    }

    #[test]
    fn execute_without_keep_going_fails_fast() {
        let mkfile = "target: dep\n\techo target\ndep:\n\texit 1\n";
        let (mut graph, rules) = build_from_mkfile(mkfile, "target");
        let shell = TestShell;
        let result = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &SchedOptions::default(),
        );
        assert!(result.is_err());
    }

    // ── SchedOptions tests ─────────────────────────────────────────────

    #[test]
    fn sched_options_default_all_false() {
        let opts = SchedOptions::default();
        assert!(!opts.keep_going);
        assert!(!opts.no_exec);
        assert!(!opts.explain);
        assert!(!opts.touch);
        assert!(!opts.silent);
    }

    #[test]
    fn build_outcome_empty() {
        let outcome = BuildOutcome {
            built: vec![],
            unchanged: vec![],
            failed: vec![],
        };
        assert!(outcome.built.is_empty());
        assert!(outcome.unchanged.is_empty());
        assert!(outcome.failed.is_empty());
    }

    // ── build_recipe() tests ───────────────────────────────────────────

    #[test]
    fn build_recipe_populates_prereqs() {
        let mkfile = "target: a b\n\techo build\n";
        let (graph, rules) = build_from_mkfile(mkfile, "target");
        let target_idx = graph
            .targets
            .first()
            .copied()
            .unwrap();
        let rule = rules.get("target").unwrap();
        let recipe = build_recipe(
            &graph,
            target_idx,
            rule,
            &PathBuf::from("."),
            &HashMap::new(),
        );
        assert_eq!(recipe.target, "target");
        assert_eq!(recipe.prereqs, vec!["a", "b"]);
        assert_eq!(recipe.script, "echo build");
    }

    // ── topological_sort() edge cases ──────────────────────────────────

    #[test]
    fn topo_sort_single_node() {
        let mkfile = "target:\n";
        let (graph, _rules) = build_from_mkfile(mkfile, "target");
        let sorted = topological_sort(&graph, &graph.targets);
        assert_eq!(sorted.len(), 1);
        assert_eq!(graph.nodes[sorted[0].0].name, "target");
    }

    #[test]
    fn topo_sort_chain() {
        let mkfile = "a: b\nb: c\nc:\n";
        let (graph, _rules) = build_from_mkfile(mkfile, "a");
        let sorted = topological_sort(&graph, &graph.targets);
        let names: Vec<&str> =
            sorted.iter().map(|i| graph.nodes[i.0].name.as_str()).collect();
        // c should be first, then b, then a
        let c_pos = names.iter().position(|&n| n == "c").unwrap();
        let b_pos = names.iter().position(|&n| n == "b").unwrap();
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }

    #[test]
    fn topo_sort_diamond() {
        let mkfile = "a: b c\nb: d\nc: d\nd:\n";
        let (graph, _rules) = build_from_mkfile(mkfile, "a");
        let sorted = topological_sort(&graph, &graph.targets);
        let names: Vec<&str> =
            sorted.iter().map(|i| graph.nodes[i.0].name.as_str()).collect();
        let d_pos = names.iter().position(|&n| n == "d").unwrap();
        let b_pos = names.iter().position(|&n| n == "b").unwrap();
        let c_pos = names.iter().position(|&n| n == "c").unwrap();
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        // d must come before b and c
        assert!(d_pos < b_pos);
        assert!(d_pos < c_pos);
        // b and c must come before a
        assert!(b_pos < a_pos);
        assert!(c_pos < a_pos);
    }

    // ── NodeFlags after execution ──────────────────────────────────────

    #[test]
    fn node_marked_made_after_successful_build() {
        let (mut graph, rules) =
            build_from_mkfile("target:\n\techo ok\n", "target");
        let shell = TestShell;
        let _ = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &SchedOptions::default(),
        )
        .unwrap();
        let target_idx = graph.targets[0];
        assert!(graph.nodes[target_idx.0].flags.is_made());
    }

    #[test]
    fn node_not_marked_made_after_failure_with_keep_going() {
        let mkfile = "target:\n\texit 1\n";
        let (mut graph, rules) = build_from_mkfile(mkfile, "target");
        let shell = TestShell;
        let opts = SchedOptions {
            keep_going: true,
            ..Default::default()
        };
        let _ = execute(
            &mut graph,
            &rules,
            &shell,
            &PathBuf::from("."),
            &HashMap::new(),
            &opts,
        )
        .unwrap();
        let target_idx = graph.targets[0];
        assert!(!graph.nodes[target_idx.0].flags.is_made());
    }
}
