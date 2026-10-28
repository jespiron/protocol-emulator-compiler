//! Exercise the installed-binary contract with isolated project directories.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spatial"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn new() -> TempDir {
    let root = TempDir::new().unwrap();
    let output = cli(root.path(), &["new", "demo space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    root
}
#[test]
fn new_build_nested_discovery_and_preserve_failure() {
    let root = new();
    let project = root.path().join("demo space");
    assert!(!cli(root.path(), &["new", "demo space"]).status.success());
    assert!(cli(&project.join("src"), &["build"]).status.success());
    let image = project.join("target/spatial/artifacts/image.bin");
    let report = project.join("target/spatial/artifacts/report.json");
    let old = fs::read(&image).unwrap();
    let old_report = fs::read(&report).unwrap();
    fs::write(project.join("src/main.toml"), "bogus=true").unwrap();
    assert!(!cli(&project, &["build"]).status.success());
    assert_eq!(fs::read(&image).unwrap(), old);
    assert_eq!(fs::read(&report).unwrap(), old_report);
    assert!(!cli(root.path(), &["check"]).status.success());
}
#[test]
fn simulation_invalid_before_dependency_setup() {
    let root = new();
    let project = root.path().join("demo space");
    let file = project.join("Spatial.toml");
    let initial = fs::read_to_string(&file).unwrap();
    for bad in [
        initial.replace("cycles = 4096", "cycles = 0"),
        initial.replace("bytes = [85, 163, 0]", "bytes = [1,2,3,4,5]"),
        initial.replace("output = \"tx\"", "output = \"rx\""),
        initial.replace("input = \"rx\"", "input = \"missing\""),
        initial.clone() + "\n[[simulation.loopback]]\noutput='tx'\ninput='rx'\n",
    ] {
        fs::write(&file, bad).unwrap();
        assert!(!cli(&project, &["run"]).status.success());
        assert!(!project.join("target").exists());
    }
    fs::write(&file, initial).unwrap();
    let source = project.join("src/main.toml");
    fs::write(
        &source,
        fs::read_to_string(&source)
            .unwrap()
            .replace("50000000", "30000000"),
    )
    .unwrap();
    assert!(!cli(&project, &["run"]).status.success());
    assert!(!project.join("target").exists());
}
#[test]
fn generated_source_collision_and_staging_failure() {
    let root = new();
    let project = root.path().join("demo space");
    assert!(cli(&project, &["build"]).status.success());
    let image = project.join("target/spatial/artifacts/image.bin");
    let original = fs::read(&image).unwrap();
    fs::write(
        project.join("Spatial.toml"),
        "source='target/spatial/artifacts/image.bin'",
    )
    .unwrap();
    assert!(!cli(&project, &["build"]).status.success());
    assert_eq!(fs::read(&image).unwrap(), original);
    fs::create_dir(project.join("target/spatial/artifacts.next")).unwrap();
    let output = cli(&project, &["build", "src/main.toml"]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&image).unwrap(), original);
}
#[cfg(unix)]
#[test]
fn symlinked_outputs_are_rejected() {
    let root = new();
    let project = root.path().join("demo space");
    std::os::unix::fs::symlink("src", project.join("target")).unwrap();
    assert!(!cli(&project, &["build"]).status.success());
    assert!(!project.join("src/spatial").exists());
}
