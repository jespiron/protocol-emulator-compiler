//! Package the simulator assets so cargo-installed binaries work outside this repo.
use crate::{AppResult, project::Project};
use spatial_compiler::HARDWARE_REVISION;
use std::{fs, process::Command};

pub(crate) fn run(project: &Project) -> AppResult<()> {
    let runner = project.output().join("runner");
    fs::create_dir_all(&runner)?;
    for (name, content) in [
        (
            "pyproject.toml",
            include_str!("../assets/simulator/pyproject.toml"),
        ),
        ("uv.lock", include_str!("../assets/simulator/uv.lock")),
        ("run.py", include_str!("../assets/simulator/run.py")),
        (
            "experiment.py",
            include_str!("../assets/simulator/experiment.py"),
        ),
    ] {
        fs::write(runner.join(name), content)?;
    }
    fs::write(
        runner.join("experiment.json"),
        serde_json::to_vec_pretty(&project.simulation)?,
    )?;
    println!(
        "Preparing RTL simulation (first run downloads pinned RTL and Python dependencies)..."
    );
    let status = Command::new("uv")
        .env_remove("VIRTUAL_ENV")
        .env_remove("UV_PROJECT_ENVIRONMENT").args(["run", "--locked", "--python", "3.12", "--project"])
        .arg(&runner).arg("python").arg(runner.join("run.py"))
        .arg(project.output()).arg(HARDWARE_REVISION).status()
        .map_err(|e| format!("Could not start uv: {e}. Install uv once: https://docs.astral.sh/uv/getting-started/installation/"))?;
    if !status.success() {
        return Err(
            "RTL simulation failed; see diagnostic above and target/spatial/simulation.log".into(),
        );
    }
    Ok(())
}
