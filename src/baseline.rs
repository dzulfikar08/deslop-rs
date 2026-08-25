//! Ports `Deslop/Baseline.hs` — the set of problems users have accepted.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::problem::{Problem, ProblemId};

const BASELINE_PATH: &str = "deslop/baseline.yaml";

#[derive(Debug, Default, Clone)]
pub struct Baseline(HashSet<ProblemId>);

#[derive(Serialize, Deserialize)]
struct BaselineFile(Vec<String>);

impl Baseline {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Test seam: a baseline holding exactly these ids.
    pub fn from_ids(ids: Vec<String>) -> Self {
        Baseline(ids.into_iter().map(ProblemId).collect())
    }

    pub fn contains(&self, id: &ProblemId) -> bool {
        self.0.contains(id)
    }

    pub fn apply(&self, problems: Vec<Problem>) -> Vec<Problem> {
        problems.into_iter().filter(|p| !self.contains(&p.id())).collect()
    }

    /// Missing file → empty baseline; malformed file → empty baseline
    /// (matches the Haskell `fromRight emptyBaseline`).
    pub fn load_from_file(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_yaml::from_str::<BaselineFile>(&s).ok())
            .map(|f| Baseline(f.0.into_iter().map(|s| ProblemId(s.trim().to_string())).collect()))
            .unwrap_or_default()
    }

    pub fn load(project_path: &Path) -> Self {
        Self::load_from_file(&project_path.join(BASELINE_PATH))
    }

    /// Sorted unique ids, matching the Haskell save format so baselines are
    /// diff-stable.
    pub fn save(project_path: &Path, problems: &[Problem]) -> std::io::Result<()> {
        let mut ids: Vec<String> =
            problems.iter().map(|p| p.id().0).collect();
        ids.sort();
        ids.dedup();
        let dir = project_path.join("deslop");
        fs::create_dir_all(&dir)?;
        let yaml = serde_yaml::to_string(&BaselineFile(ids)).expect("serialize baseline");
        fs::write(project_path.join(BASELINE_PATH), yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::problem::LintRuleId;

    fn lint(id: &str) -> Problem {
        Problem::Lint {
            lint_rule: LintRuleId(id.into()),
            file: "src/a.ts".into(),
            code: String::new(),
            description: String::new(),
            fix: None,
            auto_fixable: false,
        }
    }

    #[test]
    fn round_trip_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let ps = vec![lint("no-relative-imports"), lint("other-rule")];
        Baseline::save(dir.path(), &ps).unwrap();

        let b = Baseline::load(dir.path());
        assert_eq!(b.apply(ps.clone()).len(), 0);

        let extra = vec![lint("third-rule")];
        assert_eq!(b.apply(extra).len(), 1);
    }

    #[test]
    fn missing_file_is_empty_baseline() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Baseline::load(dir.path()).apply(vec![lint("x")]).len(), 1);
    }
}
