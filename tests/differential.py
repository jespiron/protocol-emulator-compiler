"""Frozen Python compiler vs Rust: compare images, assignments and rejection.

This reference is migration evidence, never a dependency of installed Spatial.
"""

import json
import random
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference/python/src"))
from spatial_compiler import CompileError, compile_program, parse  # noqa: E402


def main():
    binary = ROOT / "target/debug/spatial"
    sources = [p.read_text() for p in sorted((ROOT / "examples").glob("*.toml"))]
    sources += ["", "data=[1,2,255]", "[[pins]]\nname='x'\nmode='open_drain'\nsource='one'"]
    rng = random.Random(74)
    for i in range(100):
        count = rng.randrange(1, 9)
        source = "\n".join(
            f"[[pins]]\nname='p{k}'\npads={rng.sample(range(8), rng.randrange(1, 9))}\n"
            for k in range(count)
        )
        if i % 2:
            source += f"\n[[timers]]\nname='tick'\nperiod_cycles={rng.choice([1, 64, 65536])}\npulse=true\nresync='p0.level'\n"
        sources.append(source)
    for bits in (1, 8, 9, 16, 17):
        for direction in ("pull", "push"):
            sources.append(
                f"[[pins]]\nname='p'\n[[shifters]]\nname='s'\nbits={bits}\n{direction}=true\nsample='p.rise'\nsource='p.level'"
            )
    for value in (0, 1, 255, 65535):
        sources.append(
            f"[[shifters]]\nname='s'\nbits=16\nvalue={value}\nmsb=true\nlead=true\nend_on_sample=true\npre=3\npost=3\nparity=3"
        )
    sources += [
        "clock_hz=true",
        "bogus=1",
        "data=[256]",
        "[[timers]]\nname='t'\nperiod_ns=21",
        "[[pins]]\nname='zero'",
        "[[pins]]\nname='x'\npads=[8]",
        "[[shifters]]\nname='s'\npull=true\nvalue=1",
    ]
    with tempfile.TemporaryDirectory(prefix="spatial-diff-") as directory:
        folder = Path(directory)
        for source in sources:
            path = folder / "source.toml"
            path.write_text(source)
            try:
                expected = compile_program(parse(source))
            except CompileError:
                expected = None
            process = subprocess.run(
                [str(binary), "build", str(path)], cwd=folder, capture_output=True, text=True
            )
            if expected is None:
                assert process.returncode != 0, source
                continue
            assert process.returncode == 0, (source, process.stderr)
            output = folder / "target/spatial/artifacts"
            assert (output / "image.bin").read_bytes() == expected.image, source
            report = json.loads((output / "report.json").read_text())
            assert report["pins"] == expected.pins
            assert report["timers"] == expected.timers
            assert report["shifter_fifo_channels"] == expected.shifters
            assert report["config"] == [list(pair) for pair in expected.config]
            assert report["data_offset"] == expected.data_offset
    print(f"Python/Rust differential: {len(sources)} valid and invalid source cases passed")


if __name__ == "__main__":
    main()
