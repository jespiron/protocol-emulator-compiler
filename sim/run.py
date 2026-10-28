"""Fetch the pinned RTL, build it with Verilator once, and run a test plan.

Usage: run.py <target dir> <hardware revision> <test name or "">
With SPATIAL_CONSOLE set to two pipe paths, the harness serves the console instead.

The RTL checkout and the compiled simulator are cached per revision in
~/.cache/spatial, shared by every project. Upstream makefiles do not quote
paths, so everything the build touches lives under that cache directory.
"""

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HARDWARE = "https://github.com/WilliamZhang20/protocol-emulator.git"


def command(argv, log, **kwargs):
    result = subprocess.run(argv, stdout=log, stderr=log, **kwargs)
    if result.returncode:
        raise RuntimeError(f"`{' '.join(map(str, argv[:3]))} ...` failed")


def check_tools():
    for tool in ("git", "make", "c++", "verilator"):
        if shutil.which(tool) is None:
            raise RuntimeError(f"{tool} is not installed; simulation needs git, make, a C++ compiler and Verilator >= 5.036")
    version = subprocess.run(["verilator", "--version"], capture_output=True, text=True).stdout
    found = re.search(r"Verilator (\d+)\.(\d+)", version)
    if not found or tuple(map(int, found.groups())) < (5, 36):
        raise RuntimeError(f"Verilator >= 5.036 is needed; found {version.strip() or 'none'}")


def hardware(cache, revision, log):
    checkout = cache / "hardware"
    if not checkout.exists():
        print("  fetching the chip's RTL (first run only)", flush=True)
        partial = cache / "hardware.partial"
        shutil.rmtree(partial, ignore_errors=True)
        command(["git", "init", "--quiet", str(partial)], log)
        command(["git", "-C", str(partial), "fetch", "--quiet", "--depth=1", HARDWARE, revision], log)
        command(["git", "-C", str(partial), "checkout", "--quiet", "--detach", "FETCH_HEAD"], log)
        partial.rename(checkout)
    return checkout


def main():
    target, revision, only = Path(sys.argv[1]).resolve(), sys.argv[2], sys.argv[3]
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "spatial" / revision
    if " " in str(cache):
        raise RuntimeError(f"the simulator cache path cannot contain spaces: {cache}")
    cache.mkdir(parents=True, exist_ok=True)
    check_tools()
    # The console talks to the simulation through two named pipes.
    for pipe in filter(None, os.environ.get("SPATIAL_CONSOLE", "").split(os.pathsep)):
        Path(pipe).unlink(missing_ok=True)
        os.mkfifo(pipe)
    with (target / "simulation.log").open("w") as log:
        rtl = hardware(cache, revision, log) / "test"
        env = dict(os.environ)
        env.pop("COCOTB_TEST_FILTER", None)
        env.update(
            PYTHONPATH=os.pathsep.join([str(Path(__file__).parent), str(rtl)]),
            SPATIAL_PLAN=str(target / "plan.json"),
            SPATIAL_RESULTS=str(target / "results.json"),
            SPATIAL_ONLY=only,
        )
        if not (rtl / "sim_build").exists():
            print("  building the RTL simulator (first run only)", flush=True)
        command(
            [
                "make",
                "-C",
                str(rtl),
                "SIM=verilator",
                "COCOTB_TEST_MODULES=harness",
                f"PYTHON_BIN={sys.executable}",
                f"COCOTB_CONFIG={sys.executable} -m cocotb_tools.config",
                f"COCOTB_RESULTS_FILE={cache / 'results.xml'}",
            ],
            log,
            env=env,
        )


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
