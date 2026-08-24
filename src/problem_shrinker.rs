//! Ports `Deslop/ProblemShrinker.hs` — deduplicates transitive-violation
//! chains into one representative problem plus an "also reached" list.
//!
//! TODO(port): exact compaction algorithm (which chain survives, how
//! alsoReached is populated) once ViolationKind lands in Problem.

use crate::problem::Problem;

pub fn compact_problems(problems: Vec<Problem>) -> Vec<Problem> {
    let mut seen = std::collections::HashSet::new();
    problems
        .into_iter()
        .filter(|p| seen.insert(p.id()))
        .collect()
}
