//! Ports `Deslop/ProblemFormatter.hs` — one-line rendering of a Problem.

use crate::problem::Problem;

// TODO(port): match the exact Haskell output strings; goldens depend on them.
pub fn format_problem(p: &Problem) -> String {
    match p {
        Problem::Lint { file, code, description, fix: Some(fix), .. } => {
            format!("{file}: {description}\n  {code}\n  fix: {fix}")
        }
        Problem::Lint { file, code, description, fix: None, .. } => {
            format!("{file}: {description}\n  {code}")
        }
        Problem::Rule { bad_module, prose, .. } => {
            format!("{bad_module}: {prose}")
        }
    }
}
