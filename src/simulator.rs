//! Run a program's tests against the real RTL.
//!
//! The Python simulator in `sim/` is embedded in the binary and unpacked into
//! `target/sim`, so an installed `spatial` works anywhere. `uv` provides its
//! locked Python environment; Verilator simulates the pinned RTL.
use crate::AppResult;
use serde::Deserialize;
use spatial::HARDWARE_REVISION;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const FILES: [(&str, &str); 6] = [
    ("pyproject.toml", include_str!("../sim/pyproject.toml")),
    ("uv.lock", include_str!("../sim/uv.lock")),
    ("run.py", include_str!("../sim/run.py")),
    ("harness.py", include_str!("../sim/harness.py")),
    (
        "peers/__init__.py",
        include_str!("../sim/peers/__init__.py"),
    ),
    ("peers/uart.py", include_str!("../sim/peers/uart.py")),
];

/// One test's outcome, as written by `sim/harness.py`.
#[derive(Deserialize)]
pub(crate) struct Outcome {
    pub name: String,
    pub error: Option<Failure>,
    pub cycles: u64,
}

/// Why a test failed, and the source line it failed on.
#[derive(Deserialize)]
pub(crate) struct Failure {
    pub line: usize,
    pub message: String,
}

/// Simulate the plan's tests (those named `only`, if given).
pub(crate) fn run(
    target: &Path,
    plan: &serde_json::Value,
    only: Option<&str>,
) -> AppResult<Vec<Outcome>> {
    let sim = target.join("sim");
    for (name, content) in FILES {
        let path = sim.join(name);
        fs::create_dir_all(path.parent().expect("files live in a directory"))?;
        fs::write(path, content)?;
    }
    fs::write(target.join("plan.json"), serde_json::to_vec(plan)?)?;
    let results = target.join("results.json");
    let _ = fs::remove_file(&results);
    // Shared by every project; upstream makefiles break on paths with spaces,
    // which a project directory may have.
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("cannot find a cache directory; set HOME")?
        .join("spatial");
    let status = Command::new("uv")
        .env_remove("VIRTUAL_ENV")
        .env("UV_PROJECT_ENVIRONMENT", cache.join("python"))
        .args([
            "run",
            "--quiet",
            "--locked",
            "--python",
            "3.12",
            "--project",
        ])
        .arg(&sim)
        .arg("python")
        .arg(sim.join("run.py"))
        .arg(target)
        .arg(HARDWARE_REVISION)
        .arg(only.unwrap_or(""))
        .status()
        .map_err(|e| {
            format!("cannot start uv ({e}); install it from https://docs.astral.sh/uv/")
        })?;
    if !status.success() || !results.exists() {
        return Err(format!(
            "the simulator did not finish; see {}",
            target.join("simulation.log").display()
        )
        .into());
    }
    Ok(serde_json::from_slice(&fs::read(results)?)?)
}
