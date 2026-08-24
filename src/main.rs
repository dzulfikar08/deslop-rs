use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use deslop_rs::params::{Command, ParamsDto};

/// Keeps the CLI surface identical to the Haskell original:
/// `deslop <check|fix|baseline> [PROJECT_DIR]`.
#[derive(Parser)]
#[command(
    name = "deslop",
    version = concat!("Deslop Version ", env!("CARGO_PKG_VERSION")),
    about = "Removes slop from TypeScript projects.",
    long_about = None,
    arg_required_else_help = true
)]
struct Cli {
    /// Command to run: check, fix, or baseline
    #[arg(value_name = "COMMAND")]
    command: CommandArg,

    /// Path to the TypeScript project
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    project_dir: PathBuf,
}

#[derive(clap::ValueEnum, Clone, Copy)]
enum CommandArg {
    Check,
    Fix,
    Baseline,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let dto = ParamsDto {
        command: match cli.command {
            CommandArg::Check => Command::Check,
            CommandArg::Fix => Command::Fix,
            CommandArg::Baseline => Command::Baseline,
        },
        project_dir: cli.project_dir.to_string_lossy().into_owned(),
    };
    deslop_rs::run_deslop(dto)
}
