mod inspect;
mod serve;

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
    /// Serve MCP on loopback so an agent can edit a project by conversation.
    Serve {
        #[arg(long, default_value_t = palmier_mcp::DEFAULT_PORT)]
        port: u16,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { path } => ExitCode::from(inspect::run(&path) as u8),
        Command::Serve { port } => {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("palmier: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(serve::run(port)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("palmier: {error:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
