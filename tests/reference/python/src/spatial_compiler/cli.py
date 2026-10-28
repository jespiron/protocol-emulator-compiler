"""Compile checked TOML descriptions into SRAM images and allocation reports."""

import argparse
import json
import sys
from pathlib import Path

from .backend import compile_program
from .frontend import parse
from .model import ABI, HARDWARE_REVISION, CompileError


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.source.resolve() == args.output.resolve() or (
            args.report is not None
            and args.report.resolve() in (args.source.resolve(), args.output.resolve())
        ):
            raise CompileError("output-path", "source, output, and report must be different files")
        program = parse(args.source.read_text())
        result = compile_program(program)
        report = {
            "abi": ABI,
            "hardware_revision": HARDWARE_REVISION,
            "clock_hz": program.clock_hz,
            "pins": result.pins,
            "timers": result.timers,
            "shifter_fifo_channels": result.shifters,
            "image_bytes": len(result.image),
            "capacity_bytes": 1024,
            "data_offset": result.data_offset,
        }
        args.output.write_bytes(result.image)
        if args.report is not None:
            args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    except (CompileError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
