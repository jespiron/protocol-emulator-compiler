//! The `spatial` command: create, check, build and test programs.
mod console;
mod preview;
mod project;
mod simulator;

use clap::{Parser, Subcommand};
use project::Project;
use std::{
    fs,
    io::{self, IsTerminal},
    path::PathBuf,
};

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
    /// Show the Klein bottle animation.
    Preview {
        /// Print one frame instead of animating.
        #[arg(long)]
        snapshot: bool,
    },
}

/// A compile error already rendered with its source lines; printed as is.
#[derive(Debug)]
struct Reported(String);

impl std::fmt::Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Reported {}

/// Whether to color what goes to a stream: only a terminal, and never under
/// `NO_COLOR` (<https://no-color.org>).
fn color(terminal: bool) -> bool {
    terminal && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn paint(on: bool, code: &str, text: &str) -> String {
    if on && !text.is_empty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// How full each kind of hardware is, as a bar, then which pad each pin got:
///
/// ```text
///   pins       ██░░░░░░   2 / 8
///   timers     ██░░       2 / 4
///   shifters   ██         2 / 2   full
///   memory     ▍░░░░░░░   57 / 1024 bytes
///
///   Pins
///     tx    uio0
///     rx    uio1
/// ```
fn summary(build: &spatial::Build, on: bool) -> String {
    const WIDTH: usize = 8;
    const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let rows: Vec<_> = build
        .usage
        .iter()
        .filter(|(_, used, _)| *used > 0)
        .collect();
    let label = rows.iter().map(|(name, ..)| name.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for &&(name, used, total) in &rows {
        // One cell per unit, or a scaled bar when there are too many units.
        let (filled, cells) = if total <= WIDTH {
            ("█".repeat(used), total)
        } else {
            let eighths = (used * WIDTH * 8).div_ceil(total).max(1);
            let partial = EIGHTHS[eighths % 8];
            let filled = "█".repeat(eighths / 8) + &partial.to_string();
            (filled.trim_end().to_owned(), WIDTH)
        };
        let empty = "░".repeat(cells.saturating_sub(filled.chars().count()));
        let pad = " ".repeat(WIDTH - cells);
        let unit = if name == "memory" { " bytes" } else { "" };
        let full = if used == total {
            paint(on, "33", "   full")
        } else {
            String::new()
        };
        out.push(format!(
            "  {name:<label$}   {}{}{pad}   {used} / {total}{unit}{full}",
            paint(on, "32", &filled),
            paint(on, "2", &empty),
        ));
    }
    if !build.pins.is_empty() {
        let mut pins: Vec<_> = build.pins.iter().collect();
        pins.sort_by_key(|(_, pad)| **pad);
        let width = pins.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        out.push(String::new());
        out.push(format!("  {}", paint(on, "1", "Pins")));
        for (name, pad) in pins {
            out.push(format!(
                "    {name:<width$}   {}",
                paint(on, "36", &format!("uio{pad}"))
            ));
        }
    }
    out.join("\n")
}

/// Compile and print the hardware summary.
fn build(project: &Project) -> AppResult<spatial::Build> {
    let build = spatial::compile(&project.text).map_err(|e| {
        let on = color(io::stderr().is_terminal());
        Reported(e.render(&project.display_path(), &project.text, on))
    })?;
    println!("{}", summary(&build, color(io::stdout().is_terminal())));
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
            println!();
            console::run(&project, &build)?;
        }
        Command::Run {
            test: Some(only),
            file,
        } => {
            let project = Project::find(file.as_deref())?;
            let build = build(&project)?;
            println!();
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
            let animation = preview::Animation::start("spatial run --test", "running on the RTL");
            let outcomes = simulator::run(&project.target(), &build.plan, only, &|s| {
                animation.status(s)
            });
            drop(animation);
            let outcomes = outcomes?;
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
        Command::Preview { snapshot } => {
            if snapshot {
                println!("{}", preview::snapshot());
            } else {
                preview::run()?;
            }
        }
    }
    Ok(true)
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            match error.downcast_ref::<Reported>() {
                Some(Reported(text)) => eprintln!("{text}"),
                None => {
                    let on = color(io::stderr().is_terminal());
                    eprintln!("{}: {error}", paint(on, "1;31", "error"));
                }
            }
            std::process::exit(1);
        }
    }
}
