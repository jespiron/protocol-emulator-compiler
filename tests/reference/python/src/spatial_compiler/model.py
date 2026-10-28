"""Immutable, validated input structures and target identity."""

from dataclasses import dataclass

HARDWARE_REVISION = "58da19f41c4fc91d68bd052c0105c45efbcef8ee"
ABI = "px8-spatial-v1"


class CompileError(ValueError):
    def __init__(self, code: str, message: str) -> None:
        self.code = code
        super().__init__(f"{code}: {message}")


@dataclass(frozen=True)
class Pin:
    name: str
    mode: str
    pads: tuple[int, ...]
    source: str | None


@dataclass(frozen=True)
class Timer:
    name: str
    period: int
    phase: int
    start: str
    stop: str
    idle: int
    pulse: bool
    resync: str | None
    hold: str | None
    hold_invert: bool


@dataclass(frozen=True)
class Shifter:
    name: str
    bits: int
    pre: int
    pre_value: int
    post: int
    post_value: int
    parity: int
    msb: bool
    lead: bool
    end_on_sample: bool
    idle: int
    shift: str
    sample: str
    start: str
    source: str | None
    pull: bool
    push: bool
    value: int


@dataclass(frozen=True)
class Program:
    clock_hz: int
    pins: tuple[Pin, ...]
    timers: tuple[Timer, ...]
    shifters: tuple[Shifter, ...]
    data: bytes


@dataclass(frozen=True)
class Compilation:
    image: bytes
    pins: dict[str, int]
    timers: dict[str, int]
    shifters: dict[str, int]
    config: tuple[tuple[int, int], ...]
    data_offset: int
