//! Ports `Deslop.hs` — command orchestration.
//!
//! Mirrors doWork's three commands over the ported plumbing. The per-file
//! parse/lint stage and graph rules are wired to the placeholder engines, so
//! output matches the original only after those land.

use std::time::Instant;

use rayon::prelude::*;

use crate::baseline::Baseline;
use crate::code_graph::build_module_graph;
use crate::effects::{cli, report_problem::ProblemSink};
use crate::git_ignore::GitIgnore;
use crate::lint::cycle_detection::no_import_cycles;
use crate::params::{Command, Params};
use crate::problem::Problem;
use crate::rulebook::load_rulebooks;
use crate::ts::{config::TsConfig, iterator::get_ts_files};
use crate::types::{ModuleCount, ProblemCounts, RuleCount, RunReport, RunSummary, Verdict};
use crate::ui;
use crate::utils::pluralise;

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
        Verdict::ProblemsFound(counts) => {
            fail_with(&ui::problems_found_text(&counts))
        }
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

fn do_work(params: &Params) -> Result<RunReport, crate::types::DeslopError> {
    match params.command {
        Command::Fix => {
            let baseline = Baseline::load(params.project_path.as_std_path());
            let summary = deslop_project(params, &baseline)?;
            let fixed = 0; // TODO(port): count applied fixes once CST splice lands
            log_fix_summary(fixed);
            Ok(RunReport { summary, verdict: Verdict::Clean })
        }
        Command::Check | Command::Baseline => {
            let baseline = if params.command == Command::Check {
                Baseline::load(params.project_path.as_std_path())
            } else {
                Baseline::empty()
            };
            let summary = deslop_project(params, &baseline)?;
            // TODO(port): collect per-file problems through the sink once the
            // parser lands; verdict/baseline-save below already handle both.
            let problems: Vec<Problem> = Vec::new();
            if params.command == Command::Baseline {
                Baseline::save(params.project_path.as_std_path(), &problems)
                    .map_err(|e| crate::types::DeslopError::Rulebook(e.to_string()))?;
                cli::green(&format!(
                    "✅ Success: Baseline generated with {}.",
                    pluralise(problems.len(), "problem")
                ));
                return Ok(RunReport { summary, verdict: Verdict::Clean });
            }
            let verdict = if problems.is_empty() {
                cli::green("✅ Success: No problems found.");
                Verdict::Clean
            } else {
                Verdict::ProblemsFound(ProblemCounts {
                    total: problems.len(),
                    auto_fixable: problems.iter().filter(|p| p.is_auto_fixable()).count(),
                })
            };
            Ok(RunReport { summary, verdict })
        }
    }
}

fn deslop_project(
    params: &Params,
    baseline: &Baseline,
) -> Result<RunSummary, crate::types::DeslopError> {
    let _ = baseline;
    let rulebooks = load_rulebooks(params.project_path.as_std_path())
        .map_err(crate::types::DeslopError::Rulebook)?;
    log_rulebooks(params.command, &rulebooks);

    let cfg_path = params.project_path.join("tsconfig.json");
    if !cfg_path.is_file() {
        return Err(crate::types::DeslopError::TsConfigNotFound(cfg_path));
    }
    let cfg = TsConfig::load(cfg_path.as_std_path())?;

    let git_ignore = GitIgnore::load(params.project_path.as_std_path());
    let files = get_ts_files(&git_ignore, params.project_path.as_std_path());

    let sink = ProblemSink::new();
    let asts: Vec<_> = files
        .par_iter()
        .filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
            let rel = path
                .strip_prefix(&params.project_path)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            // TODO(port): route `rel` through the module resolver's
            // reverseResolve so ids match alias-mapped import targets; the
            // raw extension-dropped path is the POSIX-shaped fallback.
            let prog = crate::ts::cst::parse_ts(&rel, &content);
            Some(crate::ast::parse_ast(rel, &prog))
        })
        .collect();

    let _ = &cfg;
    let mg = build_module_graph(&asts);
    let cycles = no_import_cycles(&mg);
    let violations = crate::rulebook::enforce(&mg, &rulebooks);
    for v in violations {
        sink.report(v);
    }
    let _ = cycles; // TODO(port): emit cycle problems in report shape

    let summary = match params.command {
        Command::Check => RunSummary::Checked(ModuleCount(asts.len()), RuleCount(rule_count(&rulebooks))),
        Command::Baseline => {
            RunSummary::Baselined(ModuleCount(asts.len()), RuleCount(rule_count(&rulebooks)))
        }
        Command::Fix => RunSummary::Scanned(ModuleCount(asts.len())),
    };
    Ok(summary)
}

fn rule_count(rulebooks: &[crate::rulebook::CompiledRulebook]) -> usize {
    let rb: usize = rulebooks.iter().map(|r| r.rule_count).sum();
    rb + BUILTIN_RULE_COUNT
}

fn log_rulebooks(c: Command, rulebooks: &[crate::rulebook::CompiledRulebook]) {
    if c == Command::Fix || rulebooks.is_empty() {
        return;
    }
    let total: usize = rule_count(rulebooks);
    cli::plain(&format!(
        "📚 Loaded {}, {}",
        pluralise(rulebooks.len(), "rulebook"),
        pluralise(total.saturating_sub(BUILTIN_RULE_COUNT), "rule")
    ));
    if total == BUILTIN_RULE_COUNT {
        cli::yellow_bold(
            "WARNING: No architecture rules loaded. Deslop is only running its built-in checks.\n\
             Define your own rules in deslop/rules/*.yaml - see https://deslop.dev",
        );
    }
}

fn log_fix_summary(fixed: usize) {
    cli::plain(cli::DIVIDER);
    cli::green(&match fixed {
        0 => "✨ The project is already clean!".to_string(),
        n => format!("✨ Fixed {}!", pluralise(n, "problem")),
    });
    cli::plain(cli::DIVIDER);
}
