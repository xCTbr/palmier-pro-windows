mod inspect;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "palmier",
    version,
    about = "AI-native video editing, driven over MCP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Describe a project: timelines, durations, and tracks.
    Inspect {
        /// A `.palmier` folder or a `project.json`.
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Inspect { path } => inspect::run(&path),
    };
    ExitCode::from(code as u8)
}
