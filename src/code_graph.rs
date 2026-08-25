//! Ports `Deslop/CodeGraph.hs` — the import graph over resolved module ids.
//!
//! Parsed files and unparsed third-party dependencies are both vertices: a
//! target no file owns (an npm package, an unresolved specifier) becomes an
//! edgeless key, so reachability and existence queries see it.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::AstModule;

pub type CodeGraph = HashMap<String, Vec<String>>;

/// Builds module → imported-module-id edges. Import specifiers are kept as
/// written: a file whose own id is its alias-mapped form meets its importers
/// because they spell that alias; a relative specifier stays raw and dangles
/// as an external vertex, which is what the relative-imports lint is for.
pub fn build_module_graph(asts: &[AstModule]) -> CodeGraph {
    let mut graph: CodeGraph = HashMap::new();
    for m in asts {
        let edges = graph.entry(m.module_id().to_string()).or_default();
        for n in &m.nodes {
            edges.push(n.target.clone());
        }
    }
    // External targets become edgeless vertices, as in the original.
    for m in asts {
        for n in &m.nodes {
            graph.entry(n.target.clone()).or_default();
        }
    }
    graph
}

/// Whether any module or import names this id.
pub fn module_exists(graph: &CodeGraph, id: &str) -> bool {
    graph.contains_key(id)
}

/// Every module id reachable from `from`, itself included, sorted so runs are
/// deterministic. Ports `reachableFrom`.
pub fn reachable_from(graph: &CodeGraph, from: &str) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::from([from]);
    let mut stack = vec![from];
    while let Some(current) = stack.pop() {
        if let Some(targets) = graph.get(current) {
            for next in targets {
                if seen.insert(next.as_str()) {
                    stack.push(next.as_str());
                }
            }
        }
    }
    let mut out: Vec<String> = seen.into_iter().map(str::to_string).collect();
    out.sort();
    out
}

/// The shortest dependency path from `from` to `to`, both ends included.
/// `None` when no path exists — the enforcer only asks after reachability has
/// established one. Ports `findKnownPath`.
pub fn find_known_path(graph: &CodeGraph, from: &str, to: &str) -> Option<Vec<String>> {
    let mut parents: HashMap<&str, &str> = HashMap::new();
    let mut visited: HashSet<&str> = HashSet::from([from]);
    let mut queue: VecDeque<&str> = VecDeque::from([from]);
    while let Some(current) = queue.pop_front() {
        if current == to {
            // Walk parent links back to `from`, then speak the path forward.
            let mut walk: Vec<&str> = vec![current];
            while let Some(&parent) = parents.get(walk.last().copied().unwrap()) {
                walk.push(parent);
            }
            walk.reverse();
            return Some(walk.into_iter().map(str::to_string).collect());
        }
        if let Some(targets) = graph.get(current) {
            for next in targets {
                if visited.insert(next.as_str()) {
                    parents.insert(next.as_str(), current);
                    queue.push_back(next.as_str());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstModule, ImportNode};

    fn module(id: &str, targets: &[&str]) -> AstModule {
        AstModule {
            id: id.to_string(),
            path: format!("/{id}.ts"),
            nodes: targets
                .iter()
                .map(|t| ImportNode { target: (*t).to_string(), raw_statement: String::new() })
                .collect(),
        }
    }

    #[test]
    fn external_targets_become_edgeless_vertices() {
        let g = build_module_graph(&[module("@/a", &["react", "@/b"])]);
        assert!(module_exists(&g, "react"));
        assert_eq!(g["react"], Vec::<String>::new());
        assert!(module_exists(&g, "@/b"));
    }

    #[test]
    fn reachable_includes_self_and_transitive_closure() {
        let g = build_module_graph(&[
            module("a", &["b", "react"]),
            module("b", &["c"]),
            module("c", &[]),
        ]);
        assert_eq!(reachable_from(&g, "a"), ["a", "b", "c", "react"]);
        assert_eq!(reachable_from(&g, "c"), ["c"]);
    }

    #[test]
    fn known_path_is_shortest_with_both_ends() {
        let g = build_module_graph(&[
            module("a", &["b", "c"]),
            module("b", &["d"]),
            module("c", &["d"]),
            module("d", &[]),
        ]);
        assert_eq!(find_known_path(&g, "a", "d"), Some(vec!["a".into(), "b".into(), "d".into()]));
        assert_eq!(find_known_path(&g, "a", "a"), Some(vec!["a".into()]));
        assert_eq!(find_known_path(&g, "d", "a"), None);
    }
}
