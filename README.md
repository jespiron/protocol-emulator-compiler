# Spatial

A language and compiler for the spatial protocol chip. You wire the chip's
hardware tiles (timers, shifters, counters, logic cells, pins) into a protocol,
and the compiler checks it fits the chip and writes the image the chip loads.

```sh
cargo install --locked --path .   # from a checkout of this repository
spatial new hello && cd hello
spatial build
```

| Command | What it does |
| --- | --- |
| `spatial new NAME` | Create `NAME/main.spt` |
| `spatial check` | Check the program fits the chip |
| `spatial build` | Write `target/image.bin` |

Commands find the nearest `main.spt`, or take a file: `spatial check examples/uart.spt`.

## Examples

- [`examples/loopback.spt`](examples/loopback.spt): the starter project.
- [`examples/uart.spt`](examples/uart.spt): full-duplex UART.

The language reference is the crate documentation: `cargo doc --open`.

## Layout

- `src/`: the compiler and the `spatial` command.
- `examples/`: example programs.
