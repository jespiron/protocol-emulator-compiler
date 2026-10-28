"""One developer command: reuse the runner's pinned checkout, then independent RTL tests."""

import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[2]
subprocess.run([str(root / "target/debug/spatial"), "run"], cwd=root, check=True)
subprocess.run(
    [sys.executable, str(root / "tests/rtl/run.py"), str(root / "target/spatial/hardware")],
    cwd=root,
    check=True,
)
