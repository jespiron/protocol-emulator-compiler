"""Run a compiled program's tests against the RTL, one fresh chip per test.

The plan comes from `spatial run --test`: the SRAM image, pin and stream
assignments, and each test's wires, peers and steps. Results go
to a JSON file that `spatial` prints. A failing test never stops the others.
"""

import json
import os
from pathlib import Path

import cocotb
from cocotb.clock import Clock
from cocotb.triggers import ClockCycles, FallingEdge, Lock, RisingEdge
from cocotb.utils import get_sim_time
from cocotb_tests.common import reset_top
from cocotb_tests.fabric import load_program, pop_rx_tagged, push_tx, select_channel, start_engine, tx_full

import peers

# A blocking step gives up after this many cycles.
STEP_TIMEOUT_CYCLES = 200_000


class Failure(Exception):
    """A failed expectation, reported against a source line."""


def fmt(data):
    return "[" + ", ".join(f"0x{b:02X}" for b in data) + "]"


class Bus:
    """The wires around the chip. Every pad is pulled up; the chip and the
    peers pull it low (a push-pull driver drives both ways, which a wired-AND
    models while only one side drives). `wire a -> b` makes pad b carry
    whatever pad a carries."""

    def __init__(self, dut, wires):
        self.dut = dut
        self.wires = {to: source for source, to in wires}
        self.drive = {}  # (pad, peer name) -> 0 or 1; absent means released

    def driven(self, pad):
        """Whether the chip or a peer drives the pad, rather than the pull-up."""
        pad = self.wires.get(pad, pad)
        return bool((int(self.dut.uio_oe.value) >> pad) & 1) or any(p == pad for p, _ in self.drive)

    def level(self, pad):
        pad = self.wires.get(pad, pad)
        chip = (int(self.dut.uio_out.value) >> pad) & 1 if (int(self.dut.uio_oe.value) >> pad) & 1 else 1
        return min([chip, *(level for (p, _), level in self.drive.items() if p == pad)])

    async def run(self):
        while True:
            await FallingEdge(self.dut.clk)
            self.dut.uio_in.value = sum(self.level(p) << p for p in range(8))


class Host:
    """The host's side of the host link, which carries one command at a time.
    Receive FIFOs hold only 4 bytes, so a background task keeps every
    receiving channel drained into a buffer of (byte, eop, err)."""

    def __init__(self, dut, channels):
        self.dut = dut
        self.link = Lock()
        self.received = {channel: [] for channel in sorted(channels)}

    async def write(self, channel, data):
        for byte in data:
            while True:
                async with self.link:
                    await select_channel(self.dut, channel)
                    if not await tx_full(self.dut):
                        await push_tx(self.dut, byte)
                        break
                await ClockCycles(self.dut.clk, 8)

    async def poll(self, channel):
        async with self.link:
            await select_channel(self.dut, channel)
            entry = await pop_rx_tagged(self.dut)
        if entry is not None:
            self.received[channel].append(entry)
        return entry

    async def drain(self):
        while True:
            for channel in self.received:
                await self.poll(channel)
            await ClockCycles(self.dut.clk, 4)


async def wait_for(dut, ready, describe, check):
    """Wait until `ready()` is true, or fail after STEP_TIMEOUT_CYCLES."""
    for _ in range(STEP_TIMEOUT_CYCLES // 16):
        check()
        if ready():
            return
        await ClockCycles(dut.clk, 16)
    raise Failure(f"{describe()} after {STEP_TIMEOUT_CYCLES} cycles")


class Invariants:
    """Check every `always` condition on every clock cycle."""

    def __init__(self, dut, bus, invariants):
        self.dut, self.bus, self.invariants = dut, bus, invariants
        self.problems = []

    def holds(self, node, now, before):
        if "const" in node:
            return bool(node["const"])
        if "edge" in node:
            # A line going from floating to driven is not an edge.
            (a, was_driven), (b, is_driven) = before[node["pin"]], now[node["pin"]]
            if not (was_driven and is_driven):
                return False
            return {"changed": a != b, "rise": not a and b, "fall": a and not b}[node["edge"]]
        if "pin" in node:
            return bool(now[node["pin"]][0])
        x = self.holds(node["a"], now, before)
        if node["op"] == "not":
            return not x
        y = self.holds(node["b"], now, before)
        return {"&&": x and y, "||": x or y, "==": x == y, "!=": x != y, "implies": not x or y}[node["op"]]

    async def run(self):
        before, cycle = None, 0
        while True:
            await RisingEdge(self.dut.clk)
            now = [(self.bus.level(p), self.bus.driven(p)) for p in range(8)]
            for invariant in self.invariants:
                if before is not None and not self.holds(invariant["check"], now, before):
                    self.problems.append((invariant, cycle))
            before, cycle = now, cycle + 1


async def run_test(dut, plan, test, period_ps):
    await reset_top(dut)
    await load_program(dut, plan["image"])
    channels = {s["channel"] for s in plan["streams"].values() if s["direction"] == "from_chip"}
    bus = Bus(dut, test["wires"])
    host = Host(dut, channels)
    invariants = Invariants(dut, bus, plan["invariants"])
    attached = {}
    for spec in test["peers"]:
        peer = peers.attach(spec, bus, dut)
        if isinstance(peer, str):
            return {"line": spec["line"], "message": peer}, 0
        attached[spec["name"]] = peer
    coroutines = [bus.run(), host.drain()] + [c for peer in attached.values() for c in peer.tasks()]
    tasks = [cocotb.start_soon(c) for c in coroutines]

    def check():
        for name, peer in attached.items():
            if peer.problems:
                raise Failure(f"`{name}`: {peer.problems[0]}")
        for channel, buffer in host.received.items():
            if any(err for _, _, err in buffer):
                raise Failure(f"channel {channel} received a byte marked as an error (wrong stop or parity bit)")
        if invariants.problems:
            invariant, cycle = invariants.problems[0]
            raise Failure(f"always \"{invariant['name']}\" (line {invariant['line']}) broke at cycle {cycle}")

    async with host.link:
        await start_engine(dut)
    tasks.append(cocotb.start_soon(invariants.run()))
    started = get_sim_time("ps")
    line = test["line"]
    try:
        for step in test["steps"]:
            line = step["line"]
            target = step.get("target", {})
            peer = attached.get(target.get("peer"))
            if step["op"] == "write" and peer:
                peer.write(step["bytes"])
            elif step["op"] == "write":
                await host.write(plan["streams"][target["stream"]]["channel"], step["bytes"])
            elif step["op"] == "read":
                name = target.get("peer") or target["stream"]
                count = len(step["expect"])
                if peer:
                    buffer = peer.received
                    values = lambda: list(buffer)
                else:
                    buffer = host.received[plan["streams"][name]["channel"]]
                    values = lambda: [b for b, _, _ in buffer]
                await wait_for(dut, lambda: len(buffer) >= count, lambda: f"`{name}` received {fmt(values())}, then nothing", check)
                got = values()[:count]
                del buffer[:count]
                if got != step["expect"]:
                    raise Failure(f"expected {fmt(step['expect'])}, got {fmt(got)}")
            elif step["op"] == "wait":
                await ClockCycles(dut.clk, step["cycles"])
            check()
        error = None
    except Failure as failure:
        error = {"line": line, "message": str(failure)}
    finally:
        for task in tasks:
            task.cancel()
    return error, int(get_sim_time("ps") - started) // period_ps


@cocotb.test()
async def spatial(dut):
    plan = json.loads(Path(os.environ["SPATIAL_PLAN"]).read_text())
    only = os.environ.get("SPATIAL_ONLY")
    period_ps = 10**12 // plan["clock_hz"]
    cocotb.start_soon(Clock(dut.clk, period_ps, unit="ps").start())
    results = []
    for test in plan["tests"]:
        if not only or test["name"] == only:
            error, cycles = await run_test(dut, plan, test, period_ps)
            results.append({"name": test["name"], "error": error, "cycles": cycles})
    Path(os.environ["SPATIAL_RESULTS"]).write_text(json.dumps(results))
