"""Constraint witnesses and independent encoding checks, including rejection boundaries."""

from itertools import combinations, product
from pathlib import Path

import pytest

from spatial_compiler import CompileError, compile_program, parse
from spatial_compiler.backend import address_instruction, encode_image, instruction
from spatial_compiler.cli import main
from spatial_compiler.constraints import allocate_pins, allocate_units

ROOT = Path(__file__).resolve().parents[1]


def test_matching_against_exhaustive_oracle() -> None:
    subsets = [p for size in range(1, 4) for p in combinations(range(3), size)]
    for choices in product(subsets, repeat=3):
        candidates = dict(zip(("a", "b", "c"), choices, strict=True))
        possible = any(len(set(p)) == 3 for p in product(*choices))
        try:
            result = allocate_pins(candidates, capacity=3)
        except CompileError:
            assert not possible, candidates
        else:
            assert possible
            assert len(set(result.values())) == 3
            assert all(pad in candidates[name] for name, pad in result.items())


def test_pin_budget_and_greedy_trap() -> None:
    assert allocate_pins({"a": [0, 1], "b": [0]}) == {"b": 0, "a": 1}
    assert len(allocate_pins({f"p{i}": range(8) for i in range(8)})) == 8
    with pytest.raises(CompileError, match="pin-capacity"):
        allocate_pins({f"p{i}": range(8) for i in range(9)})
    with pytest.raises(CompileError, match="pin-pad"):
        allocate_pins({"invalid": [8]})


@pytest.mark.parametrize("capacity", [1, 2, 4, 8])
def test_generic_tile_limits(capacity: int) -> None:
    assert len(allocate_units([str(i) for i in range(capacity)], capacity, "test")) == capacity
    with pytest.raises(CompileError, match="tile-capacity"):
        allocate_units([str(i) for i in range(capacity + 1)], capacity, "test")


def test_fifo_width_and_signal_constraints() -> None:
    for direction in ("pull", "push"):
        for bits in (8, 9, 16):
            source = f'[[shifters]]\nname="s"\nbits={bits}\n{direction}=true\n'
            if direction == "push":
                source += 'sample="one"\nsource="rx.level"\n[[pins]]\nname="rx"\n'
            if bits > 8:
                with pytest.raises(CompileError, match="fifo-width"):
                    compile_program(parse(source))
            else:
                compile_program(parse(source))
    compile_program(parse('[[shifters]]\nname="s"\nbits=16\nvalue=65535'))
    with pytest.raises(CompileError, match="signal-class"):
        compile_program(parse('[[shifters]]\nname="s"\nsource="one"\nsample="one"'))
    with pytest.raises(CompileError, match="signal"):
        compile_program(parse('[[timers]]\nname="t"\nperiod_cycles=8\nstart="fire0"'))


@pytest.mark.parametrize(
    "source,code",
    [
        ("bogus=1", "schema"),
        ("clock_hz=true", "range"),
        ('[[pins]]\nname="ui_in"\npads=[8]', "range"),
        ('[[pins]]\nname="x"\nmode="push_pull"', "pin-source"),
        ('[[pins]]\nname="x"\nsource="one"', "pin-source"),
        ('[[pins]]\nname="x"\n[[pins]]\nname="x"', "name"),
        ('[[timers]]\nname="t"\nperiod_ns=21', "timing"),
        ('[[timers]]\nname="t"\nperiod_cycles=65537', "range"),
        ('[[timers]]\nname="t"\nperiod_cycles=0', "range"),
        ('[[timers]]\nname="t"\nperiod_cycles=4\nresync="one"', "signal-class"),
        ('[[shifters]]\nname="s"\nbits=17', "range"),
        ('[[shifters]]\nname="s"\npush=true', "rx-source"),
        ('[[shifters]]\nname="s"\npull=true\nvalue=1', "tx-source"),
        ('[[pins]]\nname="x"\nmode="push_pull"\nsource="missing.out"', "signal"),
        ('[[pins]]\nname="x"\nmode="push_pull"\nsource="x.rise"', "signal-class"),
        ("data=[256]", "range"),
    ],
)
def test_rejections(source: str, code: str) -> None:
    with pytest.raises(CompileError) as error:
        compile_program(parse(source))
    assert error.value.code == code


def test_constants_and_input_share_one_physical_pad() -> None:
    source = '[[pins]]\nname="bus"\nmode="open_drain"\npads=[3]\nsource="one"'
    result = compile_program(parse(source))
    assert result.pins == {"bus": 3}
    assert (0x1B, 0xB0) in result.config  # released open-drain constant, not level index 1
    assert parse('[[timers]]\nname="t"\nperiod_ns=1280').timers[0].period == 64


def test_encoding_vectors_and_relocation() -> None:
    assert instruction(1, 0x1234) == bytes([1, 2, 3, 0x14])
    assert instruction(1, 0) == b"\x10"
    assert address_instruction(4, 1023) == bytes([3, 15, 0x4F])
    for bad in (-1, 1024):
        with pytest.raises(CompileError):
            address_instruction(4, bad)
        with pytest.raises(CompileError):
            address_instruction(2, bad)
    config = ((0x18, 0x70), (0x31, 1))
    image, data_offset = encode_image(config, b"\xaa")
    # Independent decode of the boot sequence, then table and idle target.
    decoded: list[tuple[int, int]] = []
    operand = 0
    at = 0
    while len(decoded) < 4:
        byte = image[at]
        at += 1
        operand |= byte & 15
        if byte >> 4 == 0:
            operand <<= 4
        else:
            decoded.append((byte >> 4, operand))
            operand = 0
    pointer = decoded[0][1]
    assert decoded == [(2, at), (7, len(config)), (14, 9), (4, at - 3)]
    actual = tuple(
        (image[i], image[i + 1] + 256 * image[i + 2]) for i in range(pointer, data_offset, 3)
    )
    assert actual == config
    assert image[data_offset:] == b"\xaa"


def test_exact_sram_boundary() -> None:
    base, _ = encode_image(((0x31, 1),))
    data = bytes(1024 - len(base))
    assert len(encode_image(((0x31, 1),), data)[0]) == 1024
    with pytest.raises(CompileError, match="sram-capacity"):
        encode_image(((0x31, 1),), data + b"\0")
    with pytest.raises(CompileError, match="arm-order"):
        encode_image(((0x18, 1),))


def test_examples() -> None:
    for path in sorted((ROOT / "examples").glob("*.toml")):
        result = compile_program(parse(path.read_text()))
        assert result.config[-1] == (0x31, 1)
        assert len(result.image) < 1024
    duplex = compile_program(parse((ROOT / "examples/uart_full_duplex.toml").read_text()))
    assert duplex.shifters == {"receiver": 0, "transmitter": 1}
    assert duplex.pins == {"tx": 6, "rx": 7}


def test_cli_invalid_input_preserves_output(tmp_path: Path) -> None:
    source, output = tmp_path / "input.toml", tmp_path / "image.bin"
    source.write_text('[[pins]]\nname="x"\npads=[9]')
    output.write_bytes(b"untouched")
    assert main([str(source), "-o", str(output)]) == 1
    assert output.read_bytes() == b"untouched"
    assert main([str(source), "-o", str(source)]) == 1


@pytest.mark.parametrize(
    "kind,capacity,fields", [("timers", 4, "period_cycles=64"), ("shifters", 2, "bits=8")]
)
def test_frontend_resource_capacity(kind: str, capacity: int, fields: str) -> None:
    source = "\n".join(f'[[{kind}]]\nname="r{i}"\n{fields}' for i in range(capacity))
    compile_program(parse(source))
    with pytest.raises(CompileError, match="tile-capacity"):
        compile_program(parse(source + f'\n[[{kind}]]\nname="extra"\n{fields}'))


def test_cli_success_report(tmp_path: Path) -> None:
    import json

    source = ROOT / "examples/uart_full_duplex.toml"
    image, report = tmp_path / "program.bin", tmp_path / "report.json"
    assert main([str(source), "-o", str(image), "--report", str(report)]) == 0
    manifest = json.loads(report.read_text())
    assert manifest["image_bytes"] == len(image.read_bytes())
    assert manifest["pins"] == {"tx": 6, "rx": 7}
    assert manifest["shifter_fifo_channels"] == {"receiver": 0, "transmitter": 1}
