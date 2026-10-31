"""Additional Ethernet RTL cases; only link-pulse gaps are accelerated.

Run after `cargo build`: python3 tests/ethernet_cases.py target/debug/spatial
The shipped .spt example separately checks real 16 ms timing.
"""
from pathlib import Path
import random
import re
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    source = (Path(__file__).resolve().parents[1] / "examples/ethernet_tx.spt").read_text()
    header = bytes.fromhex("ffffffffffff02123456789a0800")
    array = lambda data: "[" + ", ".join(f"0x{x:02X}" for x in data) + "]"
    cases = []
    for name, payload in [
        ("zeros46", bytes(46)),
        ("ones46", bytes([255]) * 46),
        ("alternating100", bytes([0xAA, 0x55]) * 50),
        ("random400", random.Random(2026).randbytes(400)),
    ]:
        frame = header + payload
        wire = bytes([0x55] * 7 + [0xD5]) + frame
        tick = max(4096, 1 << (((len(wire) + 4) * 32 + 64).bit_length()))
        text = re.sub(r"const FRAME = .*", "const FRAME = " + array(frame), source)
        text = re.sub(r"const WIRE_FRAME = .*", "const WIRE_FRAME = " + array(wire), text)
        text = accelerated(text, tick)
        cases.append((name, text, None))
    base = accelerated(source, 4096)
    cases += [
        ("wrong_crc", base.replace("poly: 0x04C11DB7", "poly: 0x04C11DB5"), "bad Ethernet FCS"),
        ("missing_pulses", base.replace("    fire link_pulse\n    wait pulse.fall\n", ""), "link pulses, got"),
        ("wrong_clock", base.replace("clock 40 MHz", "clock 50 MHz"), "40 MHz core clock"),
    ]
    with tempfile.TemporaryDirectory(prefix="spatial-ethernet-") as directory:
        for name, text, error in cases:
            path = Path(directory) / (name + ".spt")
            path.write_text(text)
            result = subprocess.run(
                [binary, "run", "--test=real-time link pulses and a stored frame", str(path)],
                capture_output=True, text=True,
            )
            output = result.stdout + result.stderr
            if error is None:
                if result.returncode:
                    raise RuntimeError(name + "\n" + output)
                print(name + ": pass", flush=True)
            else:
                if not result.returncode or error not in output:
                    raise RuntimeError(name + " was not rejected as expected\n" + output)
                print(name + ": correctly rejected", flush=True)


def accelerated(source, tick):
    return source.replace("NLP_TICK = 64000 cycles", f"NLP_TICK = {tick} cycles").replace(
        "pulses: 3)", f"pulses: 3, pulse_period: {tick * 10})"
    ).replace("pulses: 6)", f"pulses: 6, pulse_period: {tick * 10})")


if __name__ == "__main__":
    main()
