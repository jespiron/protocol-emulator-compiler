//! The `spatial` command: create, check and build programs.
mod project;

use clap::{Parser, Subcommand};
use project::Project;
use std::{
    fs,
    io::{self, IsTerminal},
    path::PathBuf,
};

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

/// `text` as one shell word: as is when that is safe, otherwise quoted.
fn shell_quote(text: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c);
    if !text.is_empty() && text.chars().all(safe) {
        text.to_owned()
    } else if !text.contains(['"', '\\', '$', '`', '!']) {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
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

fn run() -> AppResult<()> {
    match Cli::parse().command {
        Command::New { name } => {
            project::new(&name)?;
            println!(
                "Created {}/{}\n\n  cd {}\n  spatial build",
                name.display(),
                project::SOURCE,
                shell_quote(&name.display().to_string())
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
