"""Bounded top-pin experiment, not an inferred protocol-compliance test.

Cycles include boot/config, beginning just before the start host command.
Inputs follow driven outputs at the following falling edge (half-cycle wire
model). Only completed host RX reads inside the window count as observed bytes.
Queued or in-progress bytes at the boundary are deliberately not drained later.
"""

import csv
import json
import os
from pathlib import Path

import cocotb
from cocotb.clock import Clock
from cocotb.triggers import ClockCycles, FallingEdge, ReadOnly, RisingEdge
from cocotb_tests.common import reset_top
from cocotb_tests.fabric import load_program, pop_rx_tagged, push_tx, select_channel, start_engine


@cocotb.test()
async def experiment(dut):
    out = Path(os.environ["SPATIAL_OUTPUT"])
    scenario = json.loads(Path(os.environ["SPATIAL_EXPERIMENT"]).read_text())
    report = json.loads((out / "artifacts/report.json").read_text())
    period_ps = 1_000_000_000_000 // report["clock_hz"]
    clock = cocotb.start_soon(Clock(dut.clk, period_ps, unit="ps").start())
    await reset_top(dut)
    dut.uio_in.value = scenario["idle_inputs"]
    await load_program(dut, list((out / "artifacts/image.bin").read_bytes()))
    channels = report["shifter_fifo_channels"]
    for tx in scenario["tx"]:
        await select_channel(dut, channels[tx["shifter"]])
        for byte in tx["bytes"]:
            await push_tx(dut, byte)
    received = {name: [] for name in report["rx_shifters"]}
    tasks = []

    async def host():
        await start_engine(dut)
        while True:
            if not received:
                await ClockCycles(dut.clk, 1)
            for name in received:
                await select_channel(dut, channels[name])
                entry = await pop_rx_tagged(dut)
                if entry is not None:
                    byte, _eop, error = entry
                    assert not error, f"RX error tag on {name}"
                    received[name].append(byte)

    async def wires():
        while True:
            await FallingEdge(dut.clk)
            value = scenario["idle_inputs"]
            for wire in scenario["loopback"]:
                source = report["pins"][wire["output"]]
                dest = report["pins"][wire["input"]]
                if (int(dut.uio_oe.value) >> source) & 1:
                    level = (int(dut.uio_out.value) >> source) & 1
                    value = (value & ~(1 << dest)) | (level << dest)
            dut.uio_in.value = value

    tasks.extend([cocotb.start_soon(host()), cocotb.start_soon(wires())])
    try:
        with (out / "trace.csv").open("w", newline="") as stream:
            writer = csv.writer(stream)
            writer.writerow(["cycle", "uio_in", "uio_out", "uio_oe"])
            previous = None
            for cycle in range(scenario["cycles"]):
                await RisingEdge(dut.clk)
                await ReadOnly()
                state = tuple(
                    int(getattr(dut, field).value) for field in ("uio_in", "uio_out", "uio_oe")
                )
                if state != previous:
                    writer.writerow([cycle, *state])
                    previous = state
    finally:
        for task in tasks:
            task.cancel()
        clock.cancel()
        (out / "rx.json").write_text(json.dumps(received, indent=2) + "\n")
    for expected in scenario["expect_rx"]:
        assert received[expected["shifter"]] == expected["bytes"], (
            f"{expected['shifter']}: expected {expected['bytes']}, observed {received[expected['shifter']]}"
        )
