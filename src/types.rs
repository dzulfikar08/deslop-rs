//! Ports `Types.hs` — error type and per-run report shapes.

use camino::Utf8PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DeslopError {
    #[error("tsconfig.json not found in '{0}'")]
    TsConfigNotFound(Utf8PathBuf),
    #[error("Could not parse TS config, check: '{0}'")]
    TsConfigParse(String),
    #[error("{0}")]
    Rulebook(String),
    #[error("Invalid rule configuration: {0}")]
    InvalidRuleConfig(String),
}

/// How many modules a run went through.
#[derive(Debug)]
pub struct ModuleCount(pub usize);

/// How many Rulebook Rules a run enforced.
#[derive(Debug)]
pub struct RuleCount(pub usize);

/// What a run covered, per command. `fix` enforces no Rulebook rules, so it
/// cannot claim any.
#[derive(Debug)]
pub enum RunSummary {
    Checked(ModuleCount, RuleCount),
    Baselined(ModuleCount, RuleCount),
    Scanned(ModuleCount),
}

#[derive(Debug)]
pub struct ProblemCounts {
    pub total: usize,
    pub auto_fixable: usize,
}

/// Whether a run found anything the user must act on.
#[derive(Debug)]
pub enum Verdict {
    Clean,
    ProblemsFound(ProblemCounts),
}

/// What a run that reached the end has to say for itself.
#[derive(Debug)]
pub struct RunReport {
    pub summary: RunSummary,
    pub verdict: Verdict,
}
