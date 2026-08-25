//! Ports `Deslop.hs` — command orchestration.
//!
//! Mirrors doWork's three commands over the ported plumbing: per-file lint
//! with fix splicing, both graph lints (cycles and rulebooks) over the module
//! graph, and the baseline round trip.

use std::time::Instant;

use rayon::prelude::*;

use crate::baseline::Baseline;
use crate::code_graph::build_module_graph;
use crate::effects::{cli, report_problem::ProblemSink};
use crate::git_ignore::GitIgnore;
use crate::lint::cycle_detection::no_import_cycles;
use crate::params::{Command, Params};
use crate::problem::Problem;
use crate::problem_shrinker::compact_problems;
use crate::rulebook::{load_rulebook, Rulebook};
use crate::rule_enforcer::enforce_rulebooks;
use crate::ts::{config::TsConfig, iterator::get_ts_files};
use crate::types::{DeslopError, ModuleCount, ProblemCounts, RuleCount, RunReport, RunSummary, Verdict};
use crate::ui;
use crate::utils::pluralise;

/// No relative imports and no import cycles.
const BUILTIN_RULE_COUNT: usize = 2;

pub fn run_deslop(dto: crate::params::ParamsDto) -> std::process::ExitCode {
    let params = match Params::from_dto(&dto) {
        Ok(p) => p,
        Err(e) => return fail_with(&ui::human_readable(&e)),
    };
    let start = Instant::now();
    cli::blue_bold(&format!(
        "🚀 {} project: {}",
        command_title(params.command),
        params.project_path
    ));
    if params.command == Command::Fix {
        cli::plain("Changelog:");
    }

    let report = match do_work(&params) {
        Ok(r) => r,
        Err(e) => return fail_with(&ui::human_readable(&e)),
    };

    cli::plain(&ui::summary_line(&report.summary, start.elapsed()));
    match report.verdict {
        Verdict::Clean => std::process::ExitCode::SUCCESS,
        Verdict::ProblemsFound(counts) => fail_with(&ui::problems_found_text(&counts)),
    }
}

fn fail_with(msg: &str) -> std::process::ExitCode {
    cli::error(&format!("❌ Error: {msg}"));
    std::process::ExitCode::from(1)
}

fn command_title(c: Command) -> &'static str {
    match c {
        Command::Check => "Checking",
        Command::Fix => "Fixing",
        Command::Baseline => "Baselining",
    }
}

fn do_work(params: &Params) -> Result<RunReport, DeslopError> {
    match params.command {
        Command::Fix => {
            let baseline = Baseline::load(params.project_path.as_std_path());
            let (summary, problems) = deslop_project(params, &baseline)?;
            let problems = compact_problems(baseline.apply(problems));
            log_fix_summary(problems.iter().filter(|p| p.is_auto_fixable()).count());
            Ok(RunReport { summary, verdict: Verdict::Clean })
        }
        Command::Check => {
            let baseline = Baseline::load(params.project_path.as_std_path());
            let (summary, problems) = deslop_project(params, &baseline)?;
            let problems = compact_problems(baseline.apply(problems));
            let verdict = if problems.is_empty() {
                cli::green("✅ Success: No problems found.");
                Verdict::Clean
            } else {
                log_problems(&problems);
                Verdict::ProblemsFound(ProblemCounts {
                    total: problems.len(),
                    auto_fixable: problems.iter().filter(|p| p.is_auto_fixable()).count(),
                })
            };
            Ok(RunReport { summary, verdict })
        }
        Command::Baseline => {
            let (summary, problems) = deslop_project(params, &Baseline::empty())?;
            let problems = compact_problems(problems);
            Baseline::save(params.project_path.as_std_path(), &problems)
                .map_err(|e| DeslopError::Rulebook(e.to_string()))?;
            cli::green(&format!(
                "✅ Success: Baseline generated with {}.",
                pluralise(problems.len(), "problem")
            ));
            Ok(RunReport { summary, verdict: Verdict::Clean })
        }
    }
}

fn log_problems(problems: &[Problem]) {
    cli::error(&format!("Found {}:", pluralise(problems.len(), "problem")));
    cli::error(cli::DIVIDER);
    cli::error(&ui::problems_log_text(problems));
    cli::error(cli::DIVIDER);
}

fn deslop_project(
    params: &Params,
    baseline: &Baseline,
) -> Result<(RunSummary, Vec<Problem>), DeslopError> {
    let rulebooks =
        load_rulebook(params.project_path.as_std_path()).map_err(DeslopError::Rulebook)?;
    log_rulebooks(params.command, &rulebooks);

    let cfg_path = params.project_path.join("tsconfig.json");
    if !cfg_path.is_file() {
        return Err(DeslopError::TsConfigNotFound(cfg_path));
    }
    let cfg = TsConfig::load(cfg_path.as_std_path())?;

    let git_ignore = GitIgnore::load(params.project_path.as_std_path());
    let files = get_ts_files(&git_ignore, params.project_path.as_std_path());

    // Ports `deslopFile`: lint (and, for fix, rewrite) each file, then lower
    // it to an AstModule under its alias-mapped module id.
    let sink = ProblemSink::new();
    let asts: Vec<_> = files
        .par_iter()
        .filter_map(|path| {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    cli::error(&format!("❌ Error: {}: {e}", path.display()));
                    return None;
                }
            };
            let prog = crate::ts::cst::parse_ts(&path.to_string_lossy(), &content);
            let (fixed, problems) =
                crate::lint::relative_imports::no_relative_imports(&prog, &cfg, baseline);
            for problem in problems {
                sink.report(problem);
            }
            let rendered = fixed.render();
            if params.command == Command::Fix && rendered != content {
                match std::fs::write(path, &rendered) {
                    Ok(()) => cli::cyan_bold(&format!("  modified  {}", path.display())),
                    Err(e) => cli::error(&format!("❌ Error: {}: {e}", path.display())),
                }
            }
            let id = crate::ts::module_resolver::program_module_id(&cfg, path);
            Some(crate::ast::parse_ast(id.0, &fixed))
        })
        .collect();

    let mut problems = sink.take();
    // `fix` enforces no rulebook rules and walks no graph.
    if params.command != Command::Fix {
        let graph = build_module_graph(&asts);
        for m in &asts {
            problems.extend(enforce_rulebooks(m, &graph, &rulebooks)?);
        }
        problems.extend(no_import_cycles(&asts, &graph, &cfg.base_url));
    }

    let summary = match params.command {
        Command::Check => {
            RunSummary::Checked(ModuleCount(asts.len()), RuleCount(rule_count(&rulebooks)))
        }
        Command::Baseline => {
            RunSummary::Baselined(ModuleCount(asts.len()), RuleCount(rule_count(&rulebooks)))
        }
        Command::Fix => RunSummary::Scanned(ModuleCount(asts.len())),
    };
    Ok((summary, problems))
}

/// Every rule the run enforced: the rulebooks' own, plus the built-in ones
/// that hold with no rulebook at all.
fn rule_count(rulebooks: &[Rulebook]) -> usize {
    rulebooks.iter().map(|rb| rb.rules.len()).sum::<usize>() + BUILTIN_RULE_COUNT
}

/// Reports what the rulebooks contribute. Silent for `fix`, which never
/// enforces rulebook rules.
fn log_rulebooks(c: Command, rulebooks: &[Rulebook]) {
    if c == Command::Fix {
        return;
    }
    let total: usize = rulebooks.iter().map(|rb| rb.rules.len()).sum();
    if !rulebooks.is_empty() {
        cli::plain(&format!(
            "📚 Loaded {}, {}",
            pluralise(rulebooks.len(), "rulebook"),
            pluralise(total, "rule")
        ));
    }
    if total == 0 {
        cli::yellow_bold(
            "WARNING: No architecture rules loaded. Deslop is only running its built-in checks.\n\
             Define your own rules in deslop/rules/*.yaml - see https://deslop.dev",
        );
    }
}

/// Reports how many auto-fixable problems `deslop fix` resolved.
fn log_fix_summary(fixed: usize) {
    cli::plain(cli::DIVIDER);
    cli::green(&match fixed {
        0 => "✨ The project is already clean!".to_string(),
        n => format!("✨ Fixed {}!", pluralise(n, "problem")),
    });
    cli::plain(cli::DIVIDER);
}
