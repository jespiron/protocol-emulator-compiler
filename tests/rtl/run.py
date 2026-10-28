"""Run compiler differential and black-box RTL tests against a pinned hardware checkout."""

import json
import os
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

HARDWARE_REVISION = "58da19f41c4fc91d68bd052c0105c45efbcef8ee"

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
    subprocess.run([str(root / "target/debug/spatial"), "build", str(source)], cwd=root, check=True)
    artifacts = root / "target/spatial/artifacts"
    manifest = json.loads((artifacts / "report.json").read_text())
    compiled_image = (artifacts / "image.bin").read_bytes()
    reference = Px8()
    for address, value in manifest["config"]:
        reference.cfg(address, value)
    reference.idle_forever()
    assert bytes(reference.assemble()) == compiled_image, "legacy assembler differs"
    image, report = folder / "image.bin", folder / "report.json"
    image.write_bytes(compiled_image)
    report.write_text(json.dumps(manifest))
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(
        (str(root / "tests/rtl"), str(hardware / "test"), str(root / "src"))
    )
    env["COMPILER_IMAGE"], env["COMPILER_REPORT"] = str(image), str(report)
    results = folder / "results.xml"
    env["COCOTB_RESULTS_FILE"] = str(results)
    results.unlink(missing_ok=True)
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
