"""End-to-end project runner checks, usable with a separately installed binary."""

import json
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix="spatial-workflow-") as temporary:
        root = Path(temporary)
        subprocess.run([binary, "new", "project with spaces"], cwd=root, check=True)
        project = root / "project with spaces"
        output = project / "target/spatial"
        source = project / "src/main.toml"
        manifest = project / "Spatial.toml"
        original = manifest.read_text()

        def run(success=True):
            result = subprocess.run([binary, "run"], cwd=project, check=False)
            assert (result.returncode == 0) == success
            return json.loads((output / "rx.json").read_text())

        assert run() == {"receiver": [85, 163, 0]}
        case = next(ET.parse(output / "results.xml").iter("testcase"))
        time_50mhz = float(case.attrib["sim_time_ns"])
        source.write_text(source.read_text().replace("50000000", "25000000"))
        assert run() == {"receiver": [85, 163, 0]}
        case = next(ET.parse(output / "results.xml").iter("testcase"))
        assert float(case.attrib["sim_time_ns"]) == 2 * time_50mhz

        # An expected prefix is not enough: extra observed bytes must fail.
        manifest.write_text(
            original.replace(
                '[[simulation.expect_rx]]\nshifter = "receiver"\nbytes = [85, 163, 0]',
                '[[simulation.expect_rx]]\nshifter = "receiver"\nbytes = [85]',
            )
        )
        assert run(success=False) == {"receiver": [85, 163, 0]}
        manifest.write_text(original.replace("cycles = 4096", "cycles = 16"))
        assert run(success=False) == {"receiver": []}

        # Two active RX channels, two independent loopback wires, no UART assumption.
        source.write_text("""
[[timers]]
name = "beat"
period_cycles = 64
pulse = true
[[pins]]
name = "high"
mode = "push_pull"
source = "one"
[[pins]]
name = "low"
mode = "push_pull"
source = "zero"
[[pins]]
name = "a"
[[pins]]
name = "b"
[[shifters]]
name = "receive_a"
start = "one"
shift = "beat.rise"
sample = "beat.rise"
source = "a.level"
push = true
[[shifters]]
name = "receive_b"
start = "one"
shift = "beat.rise"
sample = "beat.rise"
source = "b.level"
push = true
""")
        manifest.write_text("""source = "src/main.toml"
[simulation]
cycles = 4096
[[simulation.loopback]]
output = "high"
input = "a"
[[simulation.loopback]]
output = "low"
input = "b"
""")
        received = run()
        assert len(received["receive_a"]) >= 2 and set(received["receive_a"]) == {255}
        assert len(received["receive_b"]) >= 2 and set(received["receive_b"]) == {0}
    print(
        "Workflow passed: paths with spaces, 50/25MHz, exact RX comparison, window boundary, two RX channels/wires"
    )


if __name__ == "__main__":
    main()
