//! Ports `Deslop/AST.hs`.

use crate::ts::cst::{TsNode, TsProgram};

/// Ports `Deslop.AST.ImportNode`: one import statement extracted from a
/// module, kept as its raw text so problems can quote it verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportNode {
    pub target: String,
    pub raw_statement: String,
}

/// Ports `Deslop.AST.AstModule`.
#[derive(Debug, Clone)]
pub struct AstModule {
    /// Module identity — relative path with `/`, extension dropped later by
    /// the resolver; see `module_id`.
    pub id: String,
    pub nodes: Vec<ImportNode>,
}

impl AstModule {
    pub fn module_id(&self) -> &str {
        &self.id
    }
}

/// Ports `Deslop.AST.parseAst`'s node mapping: every structured import node
/// becomes an edge candidate carrying its raw statement; source trivia drops
/// out.
///
/// TODO(port): the module-id half of parseAst — `reverseResolve` of the file's
/// own path through the tsconfig alias mapping, falling back to the raw
/// extension-dropped path — once `TypeScript.ModuleResolver` lands.
pub fn parse_ast(id: String, prog: &TsProgram) -> AstModule {
    let nodes = prog
        .cst
        .iter()
        .filter_map(|node| match node {
            TsNode::Import { prefix, target, suffix } => Some(ImportNode {
                target: target.clone(),
                raw_statement: format!("{prefix}{target}{suffix}"),
            }),
            TsNode::Source { .. } => None,
        })
        .collect();
    AstModule { id, nodes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ts::cst::parse_ts;

    #[test]
    fn extracts_import_nodes_with_raw_statements() {
        let src = "import x from './b';\nimport y from '@/c';\n// from './not-real'\n";
        let m = parse_ast("src/a.ts".into(), &parse_ts("src/a.ts", src));
        let targets: Vec<_> = m.nodes.iter().map(|n| n.target.clone()).collect();
        assert_eq!(targets, ["./b", "@/c"]);
        assert_eq!(m.nodes[0].raw_statement, "import x from './b';");
        // Commented import is not an edge.
        assert!(m.nodes.iter().all(|n| n.target != "./not-real"));
    }
}
