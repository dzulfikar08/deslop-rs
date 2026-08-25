//! Ports `Deslop/Rule/Lint/CycleDetection.hs` — built-in lint #2 — over the
//! `findCycles` walk from `Deslop/CodeGraph.hs`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::CodeGraph;
use crate::ast::AstModule;
use crate::problem::{LintRuleId, Problem};

pub const RULE_ID: &str = "no-import-cycles";

/// A circular import chain, listed in walk order and starting at the cycle's
/// canonical start. Every module appears exactly once — the closing edge from
/// the last module back to the start is implicit.
pub struct ModuleCycle {
    pub modules: Vec<AstModule>,
}

/// Ports `noImportCycles`.
pub fn no_import_cycles(
    asts: &[AstModule],
    graph: &CodeGraph,
    project_root: &Path,
) -> Vec<Problem> {
    find_cycles(asts, graph).iter().map(|cycle| import_cycle(project_root, cycle)).collect()
}

/// Reports the cycle against its start module, showing the loop it forms and
/// the import statement that enters it.
fn import_cycle(project_path: &Path, cycle: &ModuleCycle) -> Problem {
    let start = &cycle.modules[0];
    // a module that imports itself is its own next hop
    let next_hop = cycle.modules.get(1).unwrap_or(start);
    let entering_import = start
        .nodes
        .iter()
        .find(|node| node.target == next_hop.id)
        .map(|node| node.raw_statement.trim().to_string())
        .unwrap_or_else(|| next_hop.id.clone());
    Problem::Lint {
        lint_rule: LintRuleId(RULE_ID.into()),
        file: relative_to(project_path, &start.path),
        code: entering_import,
        description: format!(
            "Circular dependency (import cycle) detected: {}",
            render_loop(&cycle.modules)
        ),
        fix: Some(FIX_TEXT.into()),
        auto_fixable: false,
    }
}

/// The closing advice every cycle report carries, verbatim from the original.
const FIX_TEXT: &str = "Import cycles are not allowed. Break the loop by removing one of its imports - usually by extracting the shared code into a module that both sides can depend on.";

/// Renders the loop as a closed walk, repeating the start to show it closing.
fn render_loop(cycle: &[AstModule]) -> String {
    let mut ids: Vec<&str> = cycle.iter().map(|m| m.id.as_str()).collect();
    ids.push(cycle[0].id.as_str());
    ids.join(" → ")
}

fn relative_to(base: &Path, target: &str) -> String {
    Path::new(target)
        .strip_prefix(base)
        .unwrap_or(Path::new(target))
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// findCycles, and the walk it is made of
// ---------------------------------------------------------------------------

/// Ports `findCycles`: one cycle per strongly-connected component. A component
/// with more than one module is always cyclic; a lone module is cyclic only
/// when it imports itself.
pub fn find_cycles(asts: &[AstModule], graph: &CodeGraph) -> Vec<ModuleCycle> {
    let by_id: HashMap<&str, &AstModule> = asts.iter().map(|m| (m.id.as_str(), m)).collect();
    strongly_connected_components(graph)
        .into_iter()
        .filter_map(|component| cycle_of(graph, &by_id, &component))
        .collect()
}

/// Reduces a strongly-connected component to the shortest cycle through its
/// canonical start. External modules cannot occur here — they are built
/// without outgoing edges — so a component holding one is not a cycle.
fn cycle_of(
    graph: &CodeGraph,
    by_id: &HashMap<&str, &AstModule>,
    component: &[String],
) -> Option<ModuleCycle> {
    if component.iter().any(|id| !by_id.contains_key(id.as_str())) {
        return None;
    }
    let start = component.iter().min()?;
    let members: HashSet<&str> = component.iter().map(String::as_str).collect();
    let cycle = shortest_loop(graph, &members, start)?;
    Some(ModuleCycle {
        modules: cycle.into_iter().map(|id| by_id[id.as_str()].clone()).collect(),
    })
}

/// Breadth-first search for the shortest walk leading from `start` back to
/// itself within `component`. Neighbours are visited in module-id order so
/// that ties between equally short cycles resolve deterministically.
fn shortest_loop(graph: &CodeGraph, component: &HashSet<&str>, start: &str) -> Option<Vec<String>> {
    let mut visited: HashSet<&str> = HashSet::from([start]);
    let mut queue: VecDeque<(&str, Vec<String>)> = VecDeque::from([(start, vec![start.to_string()])]);
    while let Some((current, walk)) = queue.pop_front() {
        let mut neighbors: Vec<&str> = graph
            .get(current)
            .map(|targets| targets.iter().map(String::as_str).collect())
            .unwrap_or_default();
        neighbors.sort_unstable();
        neighbors.retain(|n| component.contains(n));
        if neighbors.contains(&start) {
            return Some(walk);
        }
        for neighbor in neighbors {
            if visited.insert(neighbor) {
                let mut longer = walk.clone();
                longer.push(neighbor.to_string());
                queue.push_back((neighbor, longer));
            }
        }
    }
    None
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
                        frames.push(Frame::Visit { module: module.clone(), next_edge: next_edge + 1 });
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
                        if let Some(Frame::Visit { module: parent, .. }) = frames.last().cloned() {
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
    use crate::ast::ImportNode;
    use crate::code_graph::build_module_graph;

    fn module(id: &str, targets: &[&str], raw_of: &str) -> AstModule {
        AstModule {
            id: id.to_string(),
            path: format!("/repo/{id}.ts"),
            nodes: targets
                .iter()
                .map(|t| ImportNode { target: (*t).to_string(), raw_statement: raw_of.to_string() })
                .collect(),
        }
    }

    fn graph(asts: &[AstModule]) -> CodeGraph {
        build_module_graph(asts)
    }

    #[test]
    fn reports_the_closed_walk_and_its_entering_import() {
        let asts = vec![
            module("a", &["b"], "import { b } from './b';"),
            module("b", &["c"], "import { c } from './c';"),
            module("c", &["a"], "import { a } from './a';"),
        ];
        let problems = no_import_cycles(&asts, &graph(&asts), Path::new("/repo"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::Lint { lint_rule, file, code, description, auto_fixable, .. } = &problems[0]
        else {
            panic!("expected lint problem");
        };
        assert_eq!(lint_rule.0, RULE_ID);
        assert_eq!(file, "a.ts");
        assert_eq!(code, "import { b } from './b';");
        assert_eq!(description, "Circular dependency (import cycle) detected: a → b → c → a");
        assert!(!auto_fixable);
    }

    #[test]
    fn acyclic_graph_reports_nothing() {
        let asts = vec![module("a", &["b"], "i"), module("b", &["c"], "i"), module("c", &[], "i")];
        assert!(no_import_cycles(&asts, &graph(&asts), Path::new("/repo")).is_empty());
    }

    #[test]
    fn a_self_importing_module_is_its_own_cycle() {
        let asts = vec![module("a", &["a"], "import { a } from './a';")];
        let problems = no_import_cycles(&asts, &graph(&asts), Path::new("/repo"));
        assert_eq!(problems.len(), 1);
        let Problem::Lint { description, code, .. } = &problems[0] else {
            panic!("expected lint problem");
        };
        assert_eq!(description, "Circular dependency (import cycle) detected: a → a");
        assert_eq!(code, "import { a } from './a';");
    }

    #[test]
    fn the_cycle_starts_at_its_canonical_start_and_takes_the_shortest_loop() {
        // b is the smallest id in the component, so it starts the cycle, and
        // its single edge to d closes the loop at one hop — even though the
        // component also holds the longer c → e → d → c loops.
        let asts = vec![
            module("d", &["b", "c"], "i"),
            module("b", &["d"], "i"),
            module("c", &["d", "e"], "i"),
            module("e", &["d"], "i"),
        ];
        let cycles = find_cycles(&asts, &graph(&asts));
        assert_eq!(cycles.len(), 1);
        let ids: Vec<&str> = cycles[0].modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["b", "d"]);
    }

    #[test]
    fn external_targets_never_form_cycles() {
        // react is an edgeless vertex; the component holding it is not a cycle.
        let asts = vec![module("a", &["react"], "i")];
        assert!(no_import_cycles(&asts, &graph(&asts), Path::new("/repo")).is_empty());
    }
}
