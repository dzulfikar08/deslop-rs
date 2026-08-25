//! Ports `Deslop/Rule/Book.hs` — the domain model of a rulebook: valid,
//! compiled, and ready for the hot path.
//!
//! Everything here has already been through `compiler`. A value of these
//! types cannot carry a pattern that failed to compile, a clause naming a
//! variable its target never binds, or a clause whose polarity was chosen by
//! its caller — which is what lets the enforcer read it without a single
//! check of its own.

use crate::glob_plus::{CompiledClausePattern, CompiledExcludePattern, CompiledTargetPattern};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rulebook {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub target: CompiledTargetPattern,
    pub exclude: Vec<CompiledExcludePattern>,
    pub forbids: Vec<ForbidsClause>,
    pub allows: Vec<AllowsClause>,
    pub uses: Vec<UsesClause>,
    pub exists: Vec<ExistsClause>,
    pub example: Option<String>,
    pub fix: String,
}

/// A clause's pattern plus whether it governs direct imports only or the whole
/// reachable closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbidsClause {
    pub target: CompiledClausePattern,
    pub transitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowsClause {
    pub target: CompiledClausePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsesClause {
    pub target: CompiledClausePattern,
    pub transitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistsClause {
    pub target: CompiledClausePattern,
}
