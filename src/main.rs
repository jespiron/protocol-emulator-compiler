//! The `spatial` command: create, check, build and test programs.
mod console;
mod project;
mod simulator;

use clap::{Parser, Subcommand};
use project::Project;
use std::{fs, path::PathBuf};

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(
    version,
    about = "Compile and test programs for the spatial protocol chip"
)]
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
    /// Build, then talk to the program on a simulated chip; `--test` runs its tests instead.
    Run {
        /// Run the tests; `--test=NAME` runs only that one.
        #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "")]
        test: Option<String>,
        file: Option<PathBuf>,
    },
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

fn run() -> AppResult<bool> {
    match Cli::parse().command {
        Command::New { name } => {
            project::new(&name)?;
            println!(
                "Created {}/{}\n\n  cd {}\n  spatial run --test",
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
        Command::Run { test: None, file } => {
            let project = Project::find(file.as_deref())?;
            let build = build(&project)?;
            console::run(&project, &build)?;
        }
        Command::Run {
            test: Some(only),
            file,
        } => {
            let project = Project::find(file.as_deref())?;
            let build = build(&project)?;
            let only = Some(only.as_str()).filter(|name| !name.is_empty());
            if let Some(name) = only.filter(|name| !build.tests.iter().any(|t| t == name)) {
                return Err(format!(
                    "no test named \"{name}\"; tests: {}",
                    build.tests.join(", ")
                )
                .into());
            }
            if build.tests.is_empty() {
                return Err("this program has no tests; add `test \"name\" { ... }`".into());
            }
            let outcomes = simulator::run(&project.target(), &build.plan, only)?;
            let micros =
                |cycles: u64| cycles as f64 * 1e6 / build.plan["clock_hz"].as_f64().unwrap_or(1.0);
            for outcome in &outcomes {
                match &outcome.error {
                    None => println!("  ✓ {}   {:.1} us", outcome.name, micros(outcome.cycles)),
                    Some(error) => println!(
                        "  ✗ {}\n      {}:{}: {}",
                        outcome.name,
                        project.display_path(),
                        error.line,
                        error.message
                    ),
                }
            }
            return Ok(outcomes.iter().all(|o| o.error.is_none()));
        }
    }
    Ok(true)
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
