"""Strict TOML parser: reject unknown fields and lossy integer conversions."""

import re
import tomllib
from collections.abc import Mapping

from .model import CompileError, Pin, Program, Shifter, Timer


def table(value: object, where: str) -> dict[str, object]:
    if not isinstance(value, dict) or not all(isinstance(k, str) for k in value):
        raise CompileError("schema", f"{where} must be a table")
    return {str(k): v for k, v in value.items()}


def keys(t: Mapping[str, object], allowed: str, where: str) -> None:
    unknown = sorted(set(t) - set(allowed.split()))
    if unknown:
        raise CompileError("schema", f"{where}: unsupported fields {unknown}")


def integer(value: object, low: int, high: int, where: str) -> int:
    if type(value) is not int or not low <= value <= high:
        raise CompileError("range", f"{where} must be an integer in {low}..{high}; got {value!r}")
    return value


def text(value: object, where: str) -> str:
    if not isinstance(value, str) or not value:
        raise CompileError("schema", f"{where} must be a nonempty string")
    return value


def boolean(value: object, where: str) -> bool:
    if not isinstance(value, bool):
        raise CompileError("schema", f"{where} must be true or false")
    return value


def optional_text(t: Mapping[str, object], key: str) -> str | None:
    return text(t[key], key) if key in t else None


def rows(root: Mapping[str, object], key: str) -> list[dict[str, object]]:
    value = root.get(key, [])
    if not isinstance(value, list):
        raise CompileError("schema", f"{key} must be an array of tables")
    return [table(item, key) for item in value]


def parse(source: str) -> Program:
    try:
        root = table(tomllib.loads(source), "program")
    except tomllib.TOMLDecodeError as error:
        raise CompileError("syntax", str(error)) from error
    keys(root, "clock_hz pins timers shifters data", "program")
    clock = integer(root.get("clock_hz", 50_000_000), 1, 1_000_000_000, "clock_hz")
    names: set[str] = {"zero", "one", "fire0", "fire1"}

    def name(t: Mapping[str, object]) -> str:
        n = text(t.get("name"), "name")
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", n) or n in names:
            raise CompileError("name", f"duplicate, reserved, or invalid resource name {n!r}")
        names.add(n)
        return n

    pins = []
    for t in rows(root, "pins"):
        keys(t, "name mode pads source", "pin")
        n = name(t)
        mode = text(t.get("mode", "input"), f"{n}.mode")
        if mode not in ("input", "push_pull", "open_drain"):
            raise CompileError("pin-mode", f"{n}: unsupported mode {mode!r}")
        pads = t.get("pads", list(range(8)))
        if not isinstance(pads, list) or not pads:
            raise CompileError("pin-pad", f"{n}: pads must be a nonempty list of uio indices")
        candidates = tuple(sorted({integer(v, 0, 7, f"{n}.pads") for v in pads}))
        src = optional_text(t, "source")
        if (mode == "input") != (src is None):
            raise CompileError(
                "pin-source", f"{n}: input pins cannot drive; driven pins need source"
            )
        pins.append(Pin(n, mode, candidates, src))
    timers = []
    for t in rows(root, "timers"):
        keys(
            t,
            "name period_cycles period_ns phase_cycles start stop idle pulse resync hold hold_invert",
            "timer",
        )
        n = name(t)
        if ("period_cycles" in t) == ("period_ns" in t):
            raise CompileError("timing", f"{n}: specify exactly one of period_cycles or period_ns")
        if "period_ns" in t:
            ns = integer(t["period_ns"], 1, 10**15, f"{n}.period_ns")
            cycles, remainder = divmod(clock * ns, 1_000_000_000)
            if remainder:
                raise CompileError(
                    "timing", f"{n}: {ns} ns is not an integer clock interval at {clock} Hz"
                )
        else:
            cycles = integer(t["period_cycles"], 1, 65536, f"{n}.period_cycles")
        period = integer(cycles, 1, 65536, f"{n}.period")
        timers.append(
            Timer(
                n,
                period,
                integer(t.get("phase_cycles", period - 1), 0, 65535, f"{n}.phase_cycles"),
                text(t.get("start", "one"), "start"),
                text(t.get("stop", "zero"), "stop"),
                integer(t.get("idle", 0), 0, 1, "idle"),
                boolean(t.get("pulse", False), "pulse"),
                optional_text(t, "resync"),
                optional_text(t, "hold"),
                boolean(t.get("hold_invert", False), "hold_invert"),
            )
        )
    shifters = []
    for t in rows(root, "shifters"):
        keys(
            t,
            "name bits pre pre_value post post_value parity msb lead end_on_sample idle shift sample start source pull push value",
            "shifter",
        )
        n = name(t)
        bits = integer(t.get("bits", 8), 1, 16, "bits")
        pull = boolean(t.get("pull", False), "pull")
        push = boolean(t.get("push", False), "push")
        src = optional_text(t, "source")
        sample = text(t.get("sample", "zero"), "sample")
        if bits > 8 and (pull or push):
            raise CompileError(
                "fifo-width", f"{n}: automatic FIFO paths carry only 8 bits, requested {bits}"
            )
        if (push or sample != "zero") and src is None:
            raise CompileError("rx-source", f"{n}: sampled RX requires an explicit level source")
        if push and sample == "zero":
            raise CompileError("rx-sample", f"{n}: automatic RX requires a sample signal")
        if pull and "value" in t:
            raise CompileError("tx-source", f"{n}: choose FIFO pull or a register value")
        shifters.append(
            Shifter(
                n,
                bits,
                integer(t.get("pre", 0), 0, 3, "pre"),
                integer(t.get("pre_value", 0), 0, 1, "pre_value"),
                integer(t.get("post", 0), 0, 3, "post"),
                integer(t.get("post_value", 0), 0, 1, "post_value"),
                integer(t.get("parity", 0), 0, 3, "parity"),
                boolean(t.get("msb", False), "msb"),
                boolean(t.get("lead", False), "lead"),
                boolean(t.get("end_on_sample", False), "end_on_sample"),
                integer(t.get("idle", 0), 0, 1, "idle"),
                text(t.get("shift", "zero"), "shift"),
                sample,
                text(t.get("start", "zero"), "start"),
                src,
                pull,
                push,
                integer(t.get("value", 0), 0, (1 << bits) - 1, "value"),
            )
        )
    data = root.get("data", [])
    if not isinstance(data, list):
        raise CompileError("schema", "data must be an array of bytes")
    return Program(
        clock,
        tuple(pins),
        tuple(timers),
        tuple(shifters),
        bytes(integer(v, 0, 255, "data byte") for v in data),
    )
