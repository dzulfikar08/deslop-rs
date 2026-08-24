//! Ports `UI.hs` — pure text helpers for run reporting.

use crate::problem::Problem;
use crate::problem_formatter::format_problem;
use crate::types::{DeslopError, ProblemCounts, RunSummary};
use crate::utils::pluralise;

/// The closing line of a run:
/// "⏱  Checked 412 modules enforcing 38 rules in 870ms".
pub fn summary_line(summary: &RunSummary, elapsed: std::time::Duration) -> String {
    format!("⏱  {} in {}", coverage(summary), duration(elapsed))
}

fn coverage(summary: &RunSummary) -> String {
    match summary {
        RunSummary::Checked(m, r) => {
            format!("Checked {} enforcing {}", modules(m.0), rules(r.0))
        }
        RunSummary::Baselined(m, r) => {
            format!("Baselined {} enforcing {}", modules(m.0), rules(r.0))
        }
        RunSummary::Scanned(m) => format!("Scanned {}", modules(m.0)),
    }
}

fn modules(n: usize) -> String {
    pluralise(n, "module")
}

fn rules(n: usize) -> String {
    pluralise(n, "rule")
}

/// Whole milliseconds below a second, seconds above it.
fn duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{secs:.2}s")
    }
}

pub fn human_readable(err: &DeslopError) -> String {
    err.to_string()
}

/// What the user can do about the Problems a check found.
pub fn problems_found_text(counts: &ProblemCounts) -> String {
    let mut lines = vec![match counts.auto_fixable {
        0 => format!(
            "Found {}, none auto-fixable.",
            pluralise(counts.total, "problem")
        ),
        n => format!(
            "Found {}, {} of them auto-fixable.",
            pluralise(counts.total, "problem"),
            n
        ),
    }];
    if counts.auto_fixable > 0 {
        lines.push(format!(
            "   Run `deslop fix` to fix the {}.",
            pluralise(counts.auto_fixable, "auto-fixable problem")
        ));
    }
    lines.push(format!(
        "   Run `deslop baseline` to silence all {}.",
        pluralise(counts.total, "problem")
    ));
    lines.join("\n")
}

pub fn problems_log_text(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(format_problem)
        .collect::<Vec<_>>()
        .join("\n---------\n\n")
}
