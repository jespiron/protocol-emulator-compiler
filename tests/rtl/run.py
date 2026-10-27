"""Run compiler differential and black-box RTL tests against a pinned hardware checkout."""

import os
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from spatial_compiler import compile_program, parse
from spatial_compiler.cli import main
from spatial_compiler.model import HARDWARE_REVISION

root = Path(__file__).resolve().parents[2]
hardware = Path(sys.argv[1]).resolve()
revision = subprocess.check_output(
    ["git", "-C", str(hardware), "rev-parse", "HEAD"], text=True
).strip()
if revision != HARDWARE_REVISION:
    raise SystemExit(f"expected RTL {HARDWARE_REVISION}, got {revision}")
if subprocess.check_output(["git", "-C", str(hardware), "diff", "HEAD", "--", "src", "test"]):
    raise SystemExit("hardware source/test checkout must be clean")
sys.path.insert(0, str(hardware / "test"))
from cocotb_tests.fabric import Px8  # noqa: E402

for example in ("uart_tx", "uart_full_duplex"):
    folder = root / "build" / example
    folder.mkdir(parents=True, exist_ok=True)
    source = root / "examples" / f"{example}.toml"
    result = compile_program(parse(source.read_text()))
    reference = Px8()
    for address, value in result.config:
        reference.cfg(address, value)
    reference.idle_forever()
    assert bytes(reference.assemble()) == result.image, "legacy assembler differs"
    image, report = folder / "image.bin", folder / "report.json"
    if main([str(source), "-o", str(image), "--report", str(report)]):
        raise SystemExit("compilation failed")
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(
        (str(root / "tests/rtl"), str(hardware / "test"), str(root / "src"))
    )
    env["COMPILER_IMAGE"], env["COMPILER_REPORT"] = str(image), str(report)
    results = folder / "results.xml"
    env["COCOTB_RESULTS_FILE"] = str(results)
    # Build/results are external to hardware checkout; pin-level harness only.
    subprocess.run(
        [
            "make",
            "-C",
            str(hardware / "test"),
            "SIM=verilator",
            f"SIM_BUILD={root / 'build/rtl'}",
            "COCOTB_TEST_MODULES=compiler_uart",
        ],
        env=env,
        check=True,
    )
    cases = list(ET.parse(results).iter("testcase"))
    if not cases or any(list(c) for c in cases):
        raise SystemExit(f"RTL test failed or skipped: {results}")
    print(f"{example}: byte-for-byte legacy encoding and RTL behavior passed")
