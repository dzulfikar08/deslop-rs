//! Ports `Deslop/CodeGraph.hs` — adjacency map over resolved imports.

use std::collections::HashMap;

use crate::ast::AstModule;

pub type CodeGraph = HashMap<String, Vec<String>>;

/// Builds module → imported-module-ids edges. TODO(port): resolution through
/// TypeScript.ModuleResolver so edges use canonical module ids, not raw
/// specifiers.
pub fn build_module_graph(asts: &[AstModule]) -> CodeGraph {
    asts.iter()
        .map(|m| {
            (
                m.module_id().to_string(),
                m.imports.iter().map(|i| i.specifier.clone()).collect(),
            )
        })
        .collect()
}
