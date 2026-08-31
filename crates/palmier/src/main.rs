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
        /// Speak MCP over stdin/stdout instead of HTTP. This is how a desktop client
        /// runs a local server: it spawns the process and talks over the pipes.
        #[arg(long)]
        stdio: bool,
    },
}

/// Double-clicking a console program on Windows runs it with no arguments and closes
/// the window the instant it exits, so the user sees nothing at all. Say what this is
/// and wait, rather than vanishing.
fn greet_and_wait() -> ExitCode {
    println!("palmier {}", env!("CARGO_PKG_VERSION"));
    println!("An AI-native video editor you drive by talking to your coding agent.\n");
    println!("This is a command-line program, not an installer. Run it from a terminal:\n");
    if cfg!(windows) {
        println!("    .\\palmier.exe serve        start the MCP server");
        println!("    .\\palmier.exe inspect <path>   describe a project\n");
    } else {
        println!("    palmier serve              start the MCP server");
        println!("    palmier inspect <path>     describe a project\n");
    }
    println!("Then point your agent at it:\n");
    println!("    claude mcp add --transport http palmier http://127.0.0.1:19789/mcp\n");

    match missing_tools().as_slice() {
        [] => println!("FFmpeg: found."),
        missing => {
            println!(
                "FFmpeg: NOT FOUND — {} missing from PATH.",
                missing.join(" and ")
            );
            println!("Rendering and media import will not work until it is installed.");
            if cfg!(windows) {
                println!("    winget install Gyan.FFmpeg");
                println!("    (then open a new terminal so PATH updates)");
            } else if cfg!(target_os = "macos") {
                println!("    brew install ffmpeg");
            } else {
                println!("    sudo apt install ffmpeg");
            }
        }
    }

    // Without this the console window closes before any of the above can be read.
    if cfg!(windows) {
        println!("\nPress Enter to close.");
        let mut discard = String::new();
        let _ = std::io::stdin().read_line(&mut discard);
    }
    ExitCode::SUCCESS
}

/// Which of the two binaries this project shells out to are absent.
pub fn missing_tools() -> Vec<&'static str> {
    ["ffmpeg", "ffprobe"]
        .into_iter()
        .filter(|tool| {
            !std::process::Command::new(tool)
                .arg("-version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
        .collect()
}

fn main() -> ExitCode {
    if std::env::args_os().count() == 1 {
        return greet_and_wait();
    }
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { path } => ExitCode::from(inspect::run(&path) as u8),
        Command::Serve { port, stdio } => {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("palmier: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let served = if stdio {
                runtime.block_on(serve::run_stdio())
            } else {
                runtime.block_on(serve::run(port))
            };
            match served {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("palmier: {error:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
