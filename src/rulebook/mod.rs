//! Port target: `Deslop/Rulebook.hs` + `Rulebook/Dto.hs` + `Rulebook/Compiler.hs`
//! + `Deslop/RuleEnforcer.hs`.
//!
//! Rulebooks are YAML files under `deslop/rules/*.yaml` with forbids / allows /
//! uses / exists clauses. The compiler validates and lowers them into rules the
//! enforcer checks against the module graph. TODO(port).

use serde::Deserialize;

use crate::code_graph::CodeGraph;
use crate::problem::Problem;

#[derive(Debug, Deserialize)]
pub struct RulebookDto {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<RuleDto>,
}

#[derive(Debug, Deserialize)]
pub struct RuleDto {
    pub id: String,
    pub severity: Option<String>,
    #[serde(flatten)]
    pub clause: ClauseDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClauseDto {
    Forbids(Vec<String>),
    Allows(Vec<String>),
    Uses(Vec<String>),
    Exists(Vec<String>),
}

pub struct CompiledRulebook {
    pub name: String,
    pub rule_count: usize,
}

/// Placeholder loader: parses YAML but enforces nothing yet.
pub fn load_rulebooks(project_root: &std::path::Path) -> Result<Vec<CompiledRulebook>, String> {
    let dir = project_root.join("deslop").join("rules");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut names: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yaml") | Some("yml")))
            .collect();
        names.sort();
        for p in names {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let dto: RulebookDto =
                serde_yaml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
            out.push(CompiledRulebook { name: dto.name, rule_count: dto.rules.len() });
        }
    }
    Ok(out)
}

/// TODO(port): enforceRulebooks — check every module's imports against every
/// compiled rule, emitting Rule violations with ViolationKind payloads.
pub fn enforce(_graph: &CodeGraph, _rulebooks: &[CompiledRulebook]) -> Vec<Problem> {
    Vec::new()
}
