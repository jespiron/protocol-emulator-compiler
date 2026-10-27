"""Checked lowering to the pinned spatial fabric ABI and PX8 SRAM image."""

from .constraints import allocate_pins, allocate_units
from .frontend import integer
from .model import Compilation, CompileError, Program


def instruction(opcode: int, operand: int) -> bytes:
    integer(opcode, 0, 15, "opcode")
    integer(operand, 0, 65535, "operand")
    digits = [int(c, 16) for c in f"{operand:x}"]
    return bytes([*digits[:-1], opcode << 4 | digits[-1]])


def address_instruction(opcode: int, address: int) -> bytes:
    integer(address, 0, 1023, "SRAM address")
    integer(opcode, 0, 15, "opcode")
    return bytes([address >> 8, address >> 4 & 15, opcode << 4 | address & 15])


def encode_image(config: tuple[tuple[int, int], ...], data: bytes = b"") -> tuple[bytes, int]:
    if not config or config[-1] != (0x31, 1):
        raise CompileError("arm-order", "last configuration write must arm the fabric")
    for address, value in config:
        integer(address, 0, 255, "configuration address")
        integer(value, 0, 65535, "configuration value")
    body = instruction(7, len(config)) + instruction(14, 9)
    idle = 3 + len(body)
    code_size = idle + 3
    code = address_instruction(2, code_size) + body + address_instruction(4, idle)
    table = bytes(b for addr, value in config for b in (addr, value & 255, value >> 8))
    offset = len(code) + len(table)
    image = code + table + data
    if len(image) > 1024:
        raise CompileError(
            "sram-capacity",
            f"image needs {len(image)} bytes: code={len(code)}, config={len(table)}, data={len(data)}; available=1024",
        )
    return image, offset


def compile_program(program: Program) -> Compilation:
    """Lower a program returned by parse(); no implicit protocol timing claim."""
    pins = allocate_pins({p.name: p.pads for p in program.pins})
    timers = allocate_units([t.name for t in program.timers], 4, "timers")
    shifters = allocate_units([s.name for s in program.shifters], 2, "shifters")
    signals = {"zero": 0, "one": 1}
    for name, k in pins.items():
        signals.update({f"{name}.rise": 16 + k, f"{name}.fall": 24 + k, f"{name}.level": 32 + k})
    for name, k in timers.items():
        signals.update(
            {f"{name}.rise": 4 + 2 * k, f"{name}.fall": 5 + 2 * k, f"{name}.out": 40 + k}
        )
    for name, k in shifters.items():
        signals.update(
            {
                f"{name}.word": 12 + 2 * k,
                f"{name}.stop": 13 + 2 * k,
                f"{name}.out": 44 + k,
                f"{name}.active": 46 + k,
            }
        )

    def signal(name: str, *, level: bool = False) -> int:
        if name not in signals:
            raise CompileError("signal", f"unknown or unsupported signal {name!r}")
        n = signals[name]
        if level and not 32 <= n < 64:
            raise CompileError("signal-class", f"{name}: selector requires a level line (32..63)")
        return n - 32 if level else n

    config: list[tuple[int, int]] = []
    for t in program.timers:
        k = timers[t.name]
        sync = 0
        if t.resync is not None:
            sync |= signal(t.resync, level=True) | 1 << 5
        if t.hold is not None:
            sync |= signal(t.hold, level=True) << 6 | 1 << 11 | int(t.hold_invert) << 12
        config.extend(
            (
                (4 * k, t.period - 1),
                (4 * k + 1, t.phase),
                (4 * k + 3, sync),
                (
                    4 * k + 2,
                    signal(t.start) | signal(t.stop) << 6 | t.idle << 12 | int(t.pulse) << 13,
                ),
            )
        )
    for s in program.shifters:
        base = 0x10 + 4 * shifters[s.name]
        frame = (
            s.bits - 1
            | s.pre << 4
            | s.pre_value << 6
            | s.post << 7
            | s.post_value << 9
            | s.parity << 10
            | int(s.msb) << 12
            | int(s.lead) << 13
            | int(s.end_on_sample) << 14
            | s.idle << 15
        )
        data = 0 if s.source is None else signal(s.source, level=True)
        config.extend(
            (
                (base, s.value),
                (base + 1, frame),
                (base + 2, signal(s.shift) | signal(s.sample) << 6),
                (base + 3, signal(s.start) | data << 6 | int(s.pull) << 11 | int(s.push) << 12),
            )
        )
    for p in program.pins:
        mode = {"input": 0, "push_pull": 1, "open_drain": 2}[p.mode]
        if p.source is None:
            value = 0
        elif p.source in ("zero", "one"):
            value = (int(p.source == "one") << 4) | (1 << 5) | (mode << 6)
        else:
            value = signal(p.source, level=True) | (mode << 6)
        config.append((0x18 + pins[p.name], value))
    config.append((0x31, 1))
    frozen = tuple(config)
    image, offset = encode_image(frozen, program.data)
    return Compilation(image, pins, timers, shifters, frozen, offset)
