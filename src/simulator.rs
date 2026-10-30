//! Run a program on the real RTL: its tests, or an interactive console.
//!
//! The Python simulator in `sim/` is embedded in the binary and unpacked into
//! `target/sim`, so an installed `spatial` works anywhere. `uv` provides its
//! locked Python environment; Verilator simulates the pinned RTL.
use crate::AppResult;
use serde::Deserialize;
use serde_json::Value as Json;
use spatial::HARDWARE_REVISION;
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

const FILES: [(&str, &str); 12] = [
    ("pyproject.toml", include_str!("../sim/pyproject.toml")),
    ("uv.lock", include_str!("../sim/uv.lock")),
    ("run.py", include_str!("../sim/run.py")),
    ("harness.py", include_str!("../sim/harness.py")),
    (
        "peers/__init__.py",
        include_str!("../sim/peers/__init__.py"),
    ),
    ("peers/ps2.py", include_str!("../sim/peers/ps2.py")),
    ("peers/swd.py", include_str!("../sim/peers/swd.py")),
    ("peers/jtag.py", include_str!("../sim/peers/jtag.py")),
    ("peers/i2c.py", include_str!("../sim/peers/i2c.py")),
    ("peers/spi.py", include_str!("../sim/peers/spi.py")),
    ("peers/uart.py", include_str!("../sim/peers/uart.py")),
    ("peers/usb.py", include_str!("../sim/peers/usb.py")),
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

/// Shared by every project; upstream makefiles break on paths with spaces,
/// which a project directory may have.
fn cache() -> AppResult<PathBuf> {
    Ok(std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("cannot find a cache directory; set HOME")?
        .join("spatial"))
}

/// Unpack the simulator and the plan, and return the command that runs them.
fn simulation(target: &Path, plan: &Json, only: Option<&str>) -> AppResult<Command> {
    let sim = target.join("sim");
    for (name, content) in FILES {
        let path = sim.join(name);
        fs::create_dir_all(path.parent().expect("files live in a directory"))?;
        fs::write(path, content)?;
    }
    fs::write(target.join("plan.json"), serde_json::to_vec(plan)?)?;
    let mut command = Command::new("uv");
    command
        .env_remove("VIRTUAL_ENV")
        .env("UV_PROJECT_ENVIRONMENT", cache()?.join("python"))
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
        .arg(only.unwrap_or(""));
    Ok(command)
}

fn no_uv(error: std::io::Error) -> String {
    format!("cannot start uv ({error}); install it from https://docs.astral.sh/uv/")
}

fn unfinished(target: &Path) -> String {
    format!(
        "the simulator did not finish; see {}",
        target.join("simulation.log").display()
    )
}

/// Start the simulator with its progress lines and its errors captured, so
/// that they never land in the middle of the animation.
fn spawn(command: &mut Command) -> AppResult<(Child, mpsc::Receiver<String>)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(no_uv)?;
    let progress = progress(child.stdout.take().expect("piped"));
    Ok((child, progress))
}

/// Progress lines, read on their own thread. Once nobody listens, they are
/// printed instead.
fn progress(stdout: ChildStdout) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let line = line.trim().to_owned();
            if let Err(mpsc::SendError(line)) = sender.send(line) {
                println!("  {line}");
            }
        }
    });
    receiver
}

/// What the simulator said went wrong, or where to look if it said nothing.
fn failure(target: &Path, child: &mut Child) -> String {
    let mut errors = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut errors);
    }
    match errors.trim() {
        "" => unfinished(target),
        errors => errors.strip_prefix("error: ").unwrap_or(errors).to_owned(),
    }
}

/// Simulate the plan's tests (those named `only`, if given), reporting the
/// simulator's progress to `status`.
pub(crate) fn run(
    target: &Path,
    plan: &Json,
    only: Option<&str>,
    status: &dyn Fn(&str),
) -> AppResult<Vec<Outcome>> {
    let results = target.join("results.json");
    let _ = fs::remove_file(&results);
    let (mut child, progress) = spawn(&mut simulation(target, plan, only)?)?;
    // The stderr pipe stays unread until the end; the simulator writes to it
    // only just before it exits, far less than a pipe holds.
    for line in progress {
        status(&line);
    }
    if !child.wait()?.success() || !results.exists() {
        return Err(failure(target, &mut child).into());
    }
    Ok(serde_json::from_slice(&fs::read(results)?)?)
}

/// A running simulation that executes requests one at a time.
pub(crate) struct Console {
    target: PathBuf,
    child: Child,
    errors: Option<thread::JoinHandle<()>>,
    requests: Option<File>,
    replies: BufReader<File>,
}

impl Console {
    /// Start the simulation and wait until the chip is running, reporting the
    /// simulator's progress to `status`.
    pub(crate) fn start(target: &Path, plan: &Json, status: &dyn Fn(&str)) -> AppResult<Self> {
        let cache = cache()?;
        fs::create_dir_all(&cache)?;
        let id = std::process::id();
        let paths = [
            cache.join(format!("console-{id}.requests")),
            cache.join(format!("console-{id}.replies")),
        ];
        let joined = std::env::join_paths(&paths)?;
        let (mut child, progress) =
            spawn(simulation(target, plan, None)?.env("SPATIAL_CONSOLE", joined))?;
        // Opening a pipe blocks until the simulator opens the other end, which
        // never happens if it fails to start; so open it on another thread.
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let opened = (|| {
                while !paths.iter().all(|p| p.exists()) {
                    thread::sleep(Duration::from_millis(50));
                }
                let requests = File::options().write(true).open(&paths[0])?;
                let replies = File::open(&paths[1])?;
                Ok::<_, std::io::Error>((requests, replies))
            })();
            let _ = sender.send(opened);
        });
        let (requests, replies) = loop {
            for line in progress.try_iter() {
                status(&line);
            }
            if let Ok(opened) = receiver.recv_timeout(Duration::from_millis(200)) {
                break opened?;
            }
            if child.try_wait()?.is_some() {
                return Err(failure(target, &mut child).into());
            }
        };
        // From here on the simulator's errors go straight to the terminal.
        let mut stderr = child.stderr.take().expect("piped");
        let errors = thread::spawn(move || {
            let _ = io::copy(&mut stderr, &mut io::stderr());
        });
        let mut console = Self {
            target: target.to_owned(),
            child,
            errors: Some(errors),
            requests: Some(requests),
            replies: BufReader::new(replies),
        };
        console.receive()?;
        Ok(console)
    }

    fn receive(&mut self) -> AppResult<Json> {
        let mut line = String::new();
        if self.replies.read_line(&mut line)? == 0 {
            return Err(unfinished(&self.target).into());
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// Run one request (`{"wires", "peers", "steps"}`) and return the reply.
    pub(crate) fn send(&mut self, request: &Json) -> AppResult<Json> {
        let requests = self.requests.as_mut().expect("open until dropped");
        writeln!(requests, "{request}")?;
        requests.flush()?;
        self.receive()
    }
}

impl Drop for Console {
    /// Closing the request pipe ends the simulation.
    fn drop(&mut self) {
        self.requests = None;
        let _ = self.child.wait();
        if let Some(errors) = self.errors.take() {
            let _ = errors.join();
        }
    }
}
