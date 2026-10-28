//! The `spatial` command: create, check and build programs.
mod project;

use clap::{Parser, Subcommand};
use project::Project;
use std::{fs, path::PathBuf};

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(version, about = "Compile programs for the spatial protocol chip")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project directory with a starter `main.spt`.
    New { name: PathBuf },
    /// Check that the program fits the chip, without writing anything.
    Check { file: Option<PathBuf> },
    /// Compile into `target/image.bin`.
    Build { file: Option<PathBuf> },
}

/// Compile and print the hardware summary.
fn build(project: &Project) -> AppResult<spatial::Build> {
    let build = spatial::compile(&project.text)
        .map_err(|e| e.render(&project.display_path(), &project.text))?;
    let used = build
        .usage
        .iter()
        .filter(|(_, used, _)| *used > 0)
        .map(|(name, used, total)| format!("{name} {used}/{total}"))
        .collect::<Vec<_>>()
        .join("  ");
    let pins = build
        .pins
        .iter()
        .map(|(name, pad)| format!("{name}→uio{pad}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("  fits   {used}");
    if !pins.is_empty() {
        println!("  pins   {pins}");
    }
    Ok(build)
}

fn run() -> AppResult<()> {
    match Cli::parse().command {
        Command::New { name } => {
            project::new(&name)?;
            println!(
                "Created {}/{}\n\n  cd {}\n  spatial build",
                name.display(),
                project::SOURCE,
                name.display()
            );
        }
        Command::Check { file } => {
            build(&Project::find(file.as_deref())?)?;
        }
        Command::Build { file } => {
            let project = Project::find(file.as_deref())?;
            let build = build(&project)?;
            fs::create_dir_all(project.target())?;
            let image = project.target().join("image.bin");
            fs::write(&image, &build.image)?;
            println!("  wrote  {}", image.display());
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
