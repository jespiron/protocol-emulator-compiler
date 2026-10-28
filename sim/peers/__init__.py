"""Built-in peers: simulated devices on the other end of the chip's wires.

A peer is written from the protocol's specification, independently of any
Spatial program, so it can referee the chip: a test fails when the chip
breaks the protocol's rules, not only when the data is wrong.

A peer class takes (bus, dut, name, **args) and provides:
  tasks()       coroutines that watch and drive its pins, run for the test
  write(data)   queue bytes for the peer to send to the chip
  received      bytes it has received from the chip, oldest first
  problems      rules the chip broke, as messages
"""

import inspect

from peers.spi import SpiDevice
from peers.uart import UartDevice

KINDS = {"spi_device": SpiDevice, "uart_device": UartDevice}


def attach(spec, bus, dut):
    """Construct a peer from its plan entry, or return an error message."""
    kind = KINDS.get(spec["kind"])
    if kind is None:
        return f"there is no built-in peer `{spec['kind']}`; there are: {', '.join(KINDS)}"
    params = {name: p for name, p in inspect.signature(kind).parameters.items() if name not in ("bus", "dut", "name")}
    args = {key: value["pin"] if isinstance(value, dict) else value for key, value in spec["args"].items()}
    required = {name for name, p in params.items() if p.default is inspect.Parameter.empty}
    if not required <= set(args) <= set(params):
        return f"`{spec['kind']}` takes {', '.join(params)}" + (f"; it needs {', '.join(sorted(required - set(args)))}" if required - set(args) else "")
    return kind(bus=bus, dut=dut, name=spec["name"], **args)
