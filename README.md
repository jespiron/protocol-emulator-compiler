# Spatial

A language, compiler and test runner for the spatial protocol chip. You wire
the chip's hardware tiles (timers, shifters, counters, logic cells, pins) into a
protocol, export what the host may call, and test it against simulated devices
on the real RTL.

```sh
cargo install --locked --path .   # from a checkout of this repository
spatial new hello && cd hello
spatial run --test
```

| Command | What it does |
| --- | --- |
| `spatial new NAME` | Create `NAME/main.spt` |
| `spatial check` | Check the program fits the chip |
| `spatial build` | Write `target/image.bin` |
| `spatial run --test` | Build, then run every `test` in RTL simulation (`--test=NAME` for one) |
| `spatial run` | Build, then type test statements at the program on a simulated chip |

Commands find the nearest `main.spt`, or take a file: `spatial run --test examples/spi.spt`.
Running tests also needs [uv](https://docs.astral.sh/uv/), git, make, a C++
compiler and Verilator 5.036 or newer. The first run fetches and builds the RTL
once into `~/.cache/spatial`.

## Examples

- [`examples/loopback.spt`](examples/loopback.spt): the starter project.
- [`examples/uart.spt`](examples/uart.spt): full-duplex UART, tested against a terminal.
- [`examples/spi.spt`](examples/spi.spt): an SPI controller with a host-callable `transfer`.
- [`examples/i2c.spt`](examples/i2c.spt): an I2C controller with `write_read`, NACK reporting and clock stretching.
- [`examples/usb.spt`](examples/usb.spt): a low-speed USB packet transmitter: NRZI, bit stuffing, CRC16 and end of packet in the fabric.

The language reference is the crate documentation: `cargo doc --open`.

Editor highlighting for VS Code, Vim, Neovim, Emacs, Helix and Zed: [`editors/`](editors/README.md).

- [`examples/jtag.spt`](examples/jtag.spt): JTAG scans against a 16-state TAP, including IDCODE and IR/DR register access.

- [`examples/swd.spt`](examples/swd.spt): raw SWD host transfers, line reset, DP reads/writes, ACK/parity and bus turnaround checks.

- [`examples/ps2.spt`](examples/ps2.spt): PS/2 keyboard reception with odd parity and stop checking; host-to-device commands are outside this example.

## Layout

- `src/`: the compiler and the `spatial` command.
- `sim/`: the simulator harness and built-in peers (simulated devices), embedded into the binary.
- `examples/`: example programs, which are also the end-to-end tests.
- `editors/`: syntax highlighting for `.spt` in common editors.
