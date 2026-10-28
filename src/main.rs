//! `spatial new`, `check`, `build`, `run` and interactive design previews.
mod preview;
mod project;
mod simulator;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(
    version,
    about = "Compiler and RTL simulator for the PX8 spatial fabric"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Create a UART loopback project without overwriting any existing directory.
    New { path: PathBuf },
    /// Validate the nearest project or a standalone TOML source, without output.
    Check { source: Option<PathBuf> },
    /// Compile into target/spatial/artifacts; filenames are chosen for you.
    Build { source: Option<PathBuf> },
    /// Build the nearest project and run its bounded RTL experiment (default).
    Run,
    /// Preview the Klein bottle animation; no builds or file writes.
    Preview {
        /// Print a static frame without requiring an interactive terminal.
        #[arg(long)]
        snapshot: bool,
    },
}
fn run() -> AppResult<()> {
    match Cli::parse().command.unwrap_or(Command::Run) {
        Command::Preview { snapshot } => {
            if snapshot {
                println!("{}", preview::snapshot());
            } else {
                preview::run()?;
            }
        }
        Command::New { path } => {
            project::new(&path)?;
            println!(
                "Created {}\nNext: cd {:?}\n      spatial run",
                path.display(),
                path
            );
        }
        command => {
            let source = match &command {
                Command::Check { source } | Command::Build { source } => source.as_deref(),
                _ => None,
            };
            let project = project::Project::discover(source)?;
            let compiled = project.compile()?;
            if matches!(command, Command::Run) {
                project.validate_simulation(&compiled)?;
            }
            println!(
                "Checked: {} / 8 pins, {} / 4 timers, {} / 2 shifters; {} / 1024 bytes",
                compiled.report.pins.len(),
                compiled.report.timers.len(),
                compiled.report.shifter_fifo_channels.len(),
                compiled.image.len()
            );
            if !matches!(command, Command::Check { .. }) {
                project.publish(&compiled)?;
                println!(
                    "Built {}",
                    project.output().join("artifacts/image.bin").display()
                );
            }
            if matches!(command, Command::Run) {
                simulator::run(&project)?;
            }
        }
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
