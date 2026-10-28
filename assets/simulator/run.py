"""Pinned real-RTL runner, embedded in the installed Rust binary.

Only generated directories are written. Tool invocations use argv, never shell
strings. Simulation builds use a no-space temporary path because upstream make
files do not quote source paths. Cached RTL and the uv environment are reusable.
"""

import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path


def command(argv, *, timeout=600, **kwargs):
    """Bound subprocess trees, including make's compiler/simulator descendants."""
    with subprocess.Popen(argv, start_new_session=True, **kwargs) as process:
        try:
            stdout, _ = process.communicate(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise
        if process.returncode:
            raise RuntimeError(f"command failed ({process.returncode}): {argv[0]}")
        return stdout


def verify_results(path):
    """A newly created results file must contain passed tests, not skipped tests."""
    cases = list(ET.parse(path).iter("testcase"))
    if not cases or any(list(case) for case in cases):
        raise RuntimeError("simulation produced no tests, skipped tests, or failures")


def prerequisites():
    for tool in ("git", "make", "c++", "verilator"):
        if shutil.which(tool) is None:
            raise RuntimeError(
                f"Missing {tool}. Simulation needs Git, make, a C++ compiler and "
                "Verilator >=5.036. See https://verilator.org/guide/latest/install.html"
            )
    version = command(["verilator", "--version"], stdout=subprocess.PIPE, text=True)
    match = re.search(r"Verilator (\d+)\.(\d+)", version)
    if not match or tuple(map(int, match.groups())) < (5, 36):
        raise RuntimeError(f"Need Verilator >=5.036; found {version.strip()}")


def hardware_checkout(out, revision, log):
    hardware = out / "hardware"
    if not hardware.exists():
        print("Fetching pinned hardware...", flush=True)
        with tempfile.TemporaryDirectory(prefix="hardware-download-", dir=out) as temporary:
            checkout = Path(temporary) / "checkout"
            command(["git", "init", "--quiet", str(checkout)], stdout=log, stderr=log)
            command(
                [
                    "git",
                    "-C",
                    str(checkout),
                    "fetch",
                    "--quiet",
                    "--depth=1",
                    "https://github.com/WilliamZhang20/protocol-emulator.git",
                    revision,
                ],
                stdout=log,
                stderr=log,
            )
            command(
                ["git", "-C", str(checkout), "checkout", "--quiet", "--detach", "FETCH_HEAD"],
                stdout=log,
                stderr=log,
            )
            checkout.rename(hardware)
    head = command(
        ["git", "-C", str(hardware), "rev-parse", "HEAD"], stdout=subprocess.PIPE, text=True
    ).strip()
    dirty = command(
        [
            "git",
            "-C",
            str(hardware),
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "src",
            "test",
        ],
        stdout=subprocess.PIPE,
    )
    if head != revision or dirty:
        raise RuntimeError(f"cached RTL must be clean at {revision}; inspect {hardware}")
    return hardware


def main():
    out = Path(sys.argv[1]).resolve()
    revision = sys.argv[2]
    runner = Path(__file__).resolve().parent
    prerequisites()
    for generated in ("rx.json", "trace.csv", "results.xml"):
        (out / generated).unlink(missing_ok=True)
    with (out / "simulation.log").open("w") as log:
        hardware = hardware_checkout(out, revision, log)
        # Fresh results workspace prevents stale PASS from a previous invocation.
        with tempfile.TemporaryDirectory(prefix="spatial-rtl-") as temporary:
            work = Path(temporary)
            shutil.copytree(hardware / "src", work / "src")
            shutil.copytree(hardware / "test", work / "test")
            # Access the managed environment through a no-space copy too.
            # cocotb emits its own install paths into Makefile includes.
            shutil.copytree(sys.prefix, work / "env", symlinks=True)
            config = f"{work / 'env/bin/python'} -m cocotb_tools.config"
            results = work / "results.xml"
            env = dict(os.environ)
            env.update(
                {
                    "PYTHONPATH": os.pathsep.join((str(runner), str(work / "test"))),
                    "SPATIAL_OUTPUT": str(out),
                    "SPATIAL_EXPERIMENT": str(runner / "experiment.json"),
                    "COCOTB_RESULTS_FILE": str(results),
                }
            )
            # Do not let an inherited test filter silently skip this experiment.
            env.pop("COCOTB_TEST_FILTER", None)
            env.pop("COCOTB_TESTCASE", None)
            print("Building Verilator model and running experiment...", flush=True)
            try:
                command(
                    [
                        "make",
                        "-C",
                        str(work / "test"),
                        "SIM=verilator",
                        "COCOTB_TEST_MODULES=experiment",
                        f"COCOTB_CONFIG={config}",
                        f"PYTHON_BIN={work / 'env/bin/python'}",
                    ],
                    env=env,
                    stdout=log,
                    stderr=log,
                )
                verify_results(results)
            except RuntimeError as error:
                if (out / "rx.json").exists():
                    received = json.loads((out / "rx.json").read_text())
                    scenario = json.loads((runner / "experiment.json").read_text())
                    for expected in scenario["expect_rx"]:
                        actual = received.get(expected["shifter"])
                        if actual != expected["bytes"]:
                            raise RuntimeError(
                                f"{expected['shifter']}: expected {expected['bytes']}, observed {actual}"
                            ) from error
                raise
            finally:
                if results.exists():
                    shutil.copyfile(results, out / "results.xml")
    received = json.loads((out / "rx.json").read_text())
    print(f"Received within observation window: {received}")
    scenario = json.loads((runner / "experiment.json").read_text())
    print(
        "RX expectations passed."
        if scenario["expect_rx"]
        else "Observation complete (no RX correctness expectations supplied)."
    )
    print(f"Trace: {out / 'trace.csv'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, ET.ParseError, subprocess.TimeoutExpired) as error:
        print(f"simulation error: {error}", file=sys.stderr)
        sys.exit(1)
