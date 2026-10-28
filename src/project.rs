//! Find projects, validate experiment inputs and publish paired build artifacts.
use crate::AppResult;
use serde::{Deserialize, Serialize};
use spatial_compiler::{Compilation, compile};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    source: PathBuf,
    #[serde(default)]
    simulation: Simulation,
}
fn cycles() -> u32 {
    4096
}
fn idle() -> u8 {
    255
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Simulation {
    #[serde(default = "cycles")]
    pub cycles: u32,
    #[serde(default = "idle")]
    pub idle_inputs: u8,
    #[serde(default)]
    pub tx: Vec<Bytes>,
    #[serde(default)]
    pub loopback: Vec<Loopback>,
    #[serde(default)]
    pub expect_rx: Vec<Bytes>,
}
impl Default for Simulation {
    fn default() -> Self {
        Self {
            cycles: cycles(),
            idle_inputs: idle(),
            tx: vec![],
            loopback: vec![],
            expect_rx: vec![],
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bytes {
    pub shifter: String,
    pub bytes: Vec<u8>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Loopback {
    pub output: String,
    pub input: String,
}

pub(crate) struct Project {
    pub root: PathBuf,
    pub source: PathBuf,
    pub simulation: Simulation,
}
impl Project {
    pub fn discover(source: Option<&Path>) -> AppResult<Self> {
        let cwd = std::env::current_dir()?;
        if let Some(source) = source {
            return Ok(Self {
                root: cwd.clone(),
                source: cwd.join(source).canonicalize()?,
                simulation: Simulation::default(),
            });
        }
        for root in cwd.ancestors() {
            let path = root.join("Spatial.toml");
            if path.exists() {
                let manifest: Manifest = toml::from_str(&fs::read_to_string(&path)?)?;
                return Ok(Self {
                    root: root.to_owned(),
                    source: root.join(manifest.source).canonicalize()?,
                    simulation: manifest.simulation,
                });
            }
        }
        Err(
            "No Spatial.toml found. Start a project with `spatial new hello`, then `cd hello`."
                .into(),
        )
    }
    pub fn output(&self) -> PathBuf {
        self.root.join("target/spatial")
    }
    pub fn compile(&self) -> AppResult<Compilation> {
        let output = self.output();
        reject_symlink_components(&output)?;
        if self.source.starts_with(&output) {
            return Err(
                "source must live outside target/spatial; that directory is generated".into(),
            );
        }
        Ok(compile(&fs::read_to_string(&self.source)?)?)
    }
    pub fn validate_simulation(&self, c: &Compilation) -> AppResult<()> {
        let s = &self.simulation;
        if !(16..=1_000_000).contains(&s.cycles) {
            return Err("simulation.cycles must be 16..1000000".into());
        }
        if !1_000_000_000_000_u64.is_multiple_of(c.report.clock_hz)
            || !(1_000_000_000_000_u64 / c.report.clock_hz).is_multiple_of(2)
        {
            return Err("simulation clock must have an exact even period in picoseconds (1 ps RTL precision)".into());
        }
        for (list, allowed, label) in [
            (&s.tx, &c.report.tx_shifters, "tx"),
            (&s.expect_rx, &c.report.rx_shifters, "expect_rx"),
        ] {
            let mut names = BTreeSet::new();
            for bytes in list {
                if !allowed.contains(&bytes.shifter) || !names.insert(&bytes.shifter) {
                    return Err(format!(
                        "simulation.{label}: unknown, wrong-direction or duplicate shifter {:?}",
                        bytes.shifter
                    )
                    .into());
                }
                if label == "tx" && bytes.bytes.len() > 4 {
                    return Err("simulation.tx preloads at most 4 bytes per FIFO; streaming host input is not implemented".into());
                }
            }
        }
        let mut destinations = BTreeSet::new();
        for wire in &s.loopback {
            if c.report.pin_modes.get(&wire.output).map(String::as_str) != Some("push_pull")
                || c.report.pin_modes.get(&wire.input).map(String::as_str) != Some("input")
            {
                return Err("simulation.loopback requires named push_pull output -> input pin; open-drain simulation is not implemented".into());
            }
            if !destinations.insert(&wire.input) {
                return Err("simulation.loopback destination has multiple drivers".into());
            }
        }
        Ok(())
    }
    pub fn publish(&self, c: &Compilation) -> AppResult<()> {
        let out = self.output();
        reject_symlink_components(&out)?;
        fs::create_dir_all(&out)?;
        let artifacts = out.join("artifacts");
        if artifacts.exists() {
            if artifacts.is_symlink() || !artifacts.is_dir() {
                return Err("artifacts must be an ordinary directory".into());
            }
            for entry in fs::read_dir(&artifacts)? {
                if entry?.file_type()?.is_symlink() {
                    return Err("refusing symlink in generated artifacts".into());
                }
            }
        }
        let stage = out.join("artifacts.next");
        let backup = out.join("artifacts.previous");
        if backup.exists() {
            return Err(
                "artifacts.previous exists; recover interrupted build before retrying".into(),
            );
        }
        fs::create_dir(&stage).map_err(|e| {
            format!("cannot stage build (another build or artifacts.next exists): {e}")
        })?;
        let result = (|| -> AppResult<()> {
            fs::write(stage.join("image.bin"), &c.image)?;
            fs::write(
                stage.join("report.json"),
                serde_json::to_vec_pretty(&c.report)?,
            )?;
            if artifacts.exists() {
                fs::rename(&artifacts, &backup)?;
            }
            if let Err(error) = fs::rename(&stage, &artifacts) {
                if backup.exists() {
                    fs::rename(&backup, &artifacts)?;
                }
                return Err(error.into());
            }
            if backup.exists() {
                fs::remove_dir_all(&backup)?;
            }
            Ok(())
        })();
        if stage.exists() {
            fs::remove_dir_all(stage)?;
        }
        result
    }
}
fn reject_symlink_components(path: &Path) -> AppResult<()> {
    for ancestor in path.ancestors() {
        if ancestor.is_symlink() {
            return Err(format!(
                "generated output path cannot use a symlink: {}",
                ancestor.display()
            )
            .into());
        }
    }
    Ok(())
}
pub(crate) fn new(path: &Path) -> AppResult<()> {
    // create_dir deliberately fails on any existing path: no overwrites.
    fs::create_dir(path)?;
    fs::create_dir(path.join("src"))?;
    fs::write(
        path.join("src/main.toml"),
        include_str!("../examples/uart_full_duplex.toml"),
    )?;
    fs::write(
        path.join("Spatial.toml"),
        include_str!("../Spatial.toml").replace("examples/uart_full_duplex.toml", "src/main.toml"),
    )?;
    fs::write(path.join(".gitignore"), "/target/\n")?;
    Ok(())
}
