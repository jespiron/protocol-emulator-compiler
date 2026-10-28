//! `spatial run`: type test statements at a running, simulated chip.
//!
//! Every line is compiled exactly as a line inside a `test` block, after the
//! `attach` and `wire` lines typed so far, so the console accepts the same
//! statements with the same checks and messages. The simulated chip keeps its
//! state between lines, and its clock stands still while you type.
use crate::AppResult;
use crate::project::Project;
use crate::simulator::Console;
use rustyline::{DefaultEditor, error::ReadlineError};
use serde_json::{Value as Json, json};

const HELP: &str = "\
  Type any statement a test can contain:
    transfer([0x01, 0x02])                 call an exported function, show its result
    send.write(\"hi\")   recv.read(2)        write to or read from a stream
    attach uart_device(from_chip: tx, to_chip: rx, bit: 64 cycles) as term
    term.write(\"ok\")   term.read(2)        talk to an attached device
    wire tx -> rx                          connect two pins
    wait 100 us                            let time pass
    expect recv.read(2) == \"ok\"            check, as in a test
  help shows this, exit (or Ctrl-D) quits.";

/// What the loaded program offers the host, for the welcome message.
fn offers(plan: &Json) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(functions) = plan["calls"]["functions"].as_object() {
        for (name, function) in functions {
            let params = function["params"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| p["name"].as_str().unwrap_or("").to_owned())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("{name}({params})"));
        }
    }
    if let Some(streams) = plan["streams"].as_object() {
        for (name, stream) in streams {
            let method = if stream["direction"] == "to_chip" {
                "write(bytes)"
            } else {
                "read(n)"
            };
            lines.push(format!("{name}.{method}"));
        }
    }
    lines
}

fn bytes(output: &Json) -> String {
    let values: Vec<u8> = output
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b.as_u64().map(|b| b as u8))
        .collect();
    let hex = values
        .iter()
        .map(|b| format!("0x{b:02X}"))
        .collect::<Vec<_>>()
        .join(", ");
    let text = String::from_utf8(values.clone())
        .ok()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_graphic() || c == ' '));
    match text {
        Some(text) => format!("[{hex}]  \"{text}\""),
        None => format!("[{hex}]"),
    }
}

pub(crate) fn run(project: &Project, build: &spatial::Build) -> AppResult<()> {
    println!("  starting the simulated chip (the first run also builds it)");
    let mut chip = Console::start(&project.target(), &build.plan)?;
    let clock_hz = build.plan["clock_hz"].as_f64().unwrap_or(1.0);
    println!("  ready. This program offers:");
    for offer in offers(&build.plan) {
        println!("    {offer}");
    }
    println!("  Type `help` for examples, `exit` to quit.\n");
    let mut editor = DefaultEditor::new()?;
    // `attach` and `wire` lines so far: later lines may refer to what they set up.
    let mut setup: Vec<String> = Vec::new();
    loop {
        let line = match editor.readline("> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(error) => return Err(error.into()),
        };
        let line = line.trim();
        match line {
            "" => continue,
            "exit" | "quit" => break,
            "help" => {
                println!("{HELP}");
                continue;
            }
            _ => {}
        }
        let _ = editor.add_history_entry(line);
        let prefix = format!(
            "{}\ntest \"console\" {{\n{}\n",
            project.text,
            setup.join("\n")
        );
        let source = format!("{prefix}{line}\n}}\n");
        let compiled = match spatial::compile(&source) {
            Ok(compiled) => compiled,
            Err(error) => {
                let start = error
                    .span
                    .start
                    .saturating_sub(prefix.len())
                    .min(line.len());
                let width = error.span.end.saturating_sub(error.span.start).max(1);
                println!(
                    "  error: {}\n  > {line}\n    {}{}",
                    error.message,
                    " ".repeat(start),
                    "^".repeat(width.min(line.len().saturating_sub(start).max(1)))
                );
                continue;
            }
        };
        let test = &compiled.plan["tests"]
            .as_array()
            .and_then(|tests| tests.last())
            .cloned()
            .unwrap_or_default();
        let reply = chip.send(&json!({
            "wires": test["wires"],
            "peers": test["peers"],
            "steps": test["steps"],
        }))?;
        let micros = reply["cycles"].as_f64().unwrap_or(0.0) * 1e6 / clock_hz;
        if let Some(error) = reply["error"].get("message").and_then(Json::as_str) {
            println!("  ✗ {error}   ({micros:.1} us)");
            continue;
        }
        if line.starts_with("attach ") || line.starts_with("wire ") {
            setup.push(line.to_owned());
        }
        let outputs: Vec<String> = reply["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| !o.is_null())
            .map(bytes)
            .collect();
        if outputs.is_empty() {
            println!("  ok   ({micros:.1} us)");
        } else {
            for output in outputs {
                println!("  {output}   ({micros:.1} us)");
            }
        }
    }
    Ok(())
}
