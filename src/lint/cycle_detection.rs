//! Ports `Deslop/Lint/CycleDetection.hs` — built-in lint #2.

use std::collections::{HashMap, HashSet};

use super::CodeGraph;

/// Reports each import cycle once per strongly-connected component.
/// TODO(port): match the Haskell report shape (chain of ModuleIds per problem)
/// rather than bare cycles.
pub fn no_import_cycles(graph: &CodeGraph) -> Vec<Vec<String>> {
    let sccs = strongly_connected_components(graph);
    sccs
        .into_iter()
        .filter(|scc| scc.len() > 1 || is_self_loop(graph, &scc[0]))
        .collect()
}

fn is_self_loop(g: &CodeGraph, m: &str) -> bool {
    g.get(m).map_or(false, |outs| outs.iter().any(|x| x == m))
}

/// Iterative Tarjan SCC over the import graph, returning sorted components.
fn strongly_connected_components(g: &CodeGraph) -> Vec<Vec<String>> {
    #[derive(Clone)]
    enum Frame {
        Enter(String),
        Visit { module: String, next_edge: usize },
    }

    let mut index = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut on_stack: HashSet<String> = HashSet::new();
    let mut idx: HashMap<String, usize> = HashMap::new();
    let mut low: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Vec<String>> = Vec::new();

    for root in g.keys() {
        if idx.contains_key(root) {
            continue;
        }
        let mut frames = vec![Frame::Enter(root.clone())];
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Enter(module) => {
                    idx.insert(module.clone(), index);
                    low.insert(module.clone(), index);
                    index += 1;
                    stack.push(module.clone());
                    on_stack.insert(module.clone());
                    frames.push(Frame::Visit { module, next_edge: 0 });
                }
                Frame::Visit { module, next_edge } => {
                    let outs = g.get(&module).cloned().unwrap_or_default();
                    if let Some(dep) = outs.get(next_edge) {
                        frames.push(Frame::Visit {
                            module: module.clone(),
                            next_edge: next_edge + 1,
                        });
                        if !idx.contains_key(dep.as_str()) {
                            frames.push(Frame::Enter(dep.clone()));
                        } else if on_stack.contains(dep) {
                            let dlow = low[dep];
                            let cur = low[&module];
                            low.insert(module.clone(), cur.min(dlow));
                        }
                    } else {
                        if low[&module] == idx[&module] {
                            let mut scc: Vec<String> = Vec::new();
                            while let Some(n) = stack.pop() {
                                let done = n == module;
                                on_stack.remove(&n);
                                scc.push(n);
                                if done {
                                    break;
                                }
                            }
                            scc.sort();
                            out.push(scc);
                        }
                        if let Some(Frame::Visit { module: parent, .. }) =
                            frames.last().cloned()
                        {
                            let plow = low[&parent];
                            let mlow = low[&module];
                            low.insert(parent, plow.min(mlow));
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[String])]) -> CodeGraph {
        edges
            .iter()
            .map(|(k, deps)| ((*k).to_string(), deps.to_vec()))
            .collect()
    }

    fn deps(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn finds_simple_cycle() {
        let g = graph(&[
            ("a", &deps(&["b"])),
            ("b", &deps(&["c"])),
            ("c", &deps(&["a"])),
        ]);
        let cycles = no_import_cycles(&g);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], ["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn acyclic_graph_has_no_cycles() {
        let g = graph(&[("a", &deps(&["b"])), ("b", &deps(&["c"]))]);
        assert!(no_import_cycles(&g).is_empty());
    }
}
