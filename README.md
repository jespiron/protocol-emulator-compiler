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

Commands find the nearest `main.spt`, or take a file: `spatial run --test examples/uart.spt`.
Running tests also needs [uv](https://docs.astral.sh/uv/), git, make, a C++
compiler and Verilator 5.036 or newer. The first run fetches and builds the RTL
once into `~/.cache/spatial`.

## Examples

- [`examples/loopback.spt`](examples/loopback.spt): the starter project.
- [`examples/uart.spt`](examples/uart.spt): full-duplex UART, tested against a terminal.

The language reference is the crate documentation: `cargo doc --open`.

## Layout

- `src/`: the compiler and the `spatial` command.
- `sim/`: the simulator harness and built-in peers (simulated devices), embedded into the binary.
- `examples/`: example programs, which are also the end-to-end tests.
