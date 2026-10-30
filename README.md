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

## Configuration contexts

`contexts(level)` groups both configurations in one place. The flag's
initializer makes the starting selection explicit:

```spatial
flag phase = 0
pin tx: out = sender.out
timer bit { every: 64 cycles, first: 62 cycles, pulse, run: sender.busy until sender.done }
contexts(phase) {
    context 0 {
        shifter sender { frame: [0, data(8), 1], idle: 1, shift: bit.tick }
    }
    context 1 {
        shifter sender { frame: [0, data(8), even, 1], idle: 1, shift: bit.tick }
    }
}
```

Both `context 0` and `context 1` are required, in either order. Context 0 can
declare the physical resources; context 1 configures those same resources
without allocating extra hardware. Declarations outside the block are shared
by both configurations. Flags accept `= 0` or `= 1`; existing declarations
without an initializer continue to start at zero. An external pin selector's
initial value comes from the external signal, not the flag initialization.

The selector may be a pin level, controller flag, or another fabric level. The
hardware changes contexts together, two clocks after the selecting level
changes. State (shifted bits, counters, timers) is shared: switching contexts
does not reset it. Switch framing between transactions unless the protocol is
designed for a mid-transfer change.

This first version permits overrides of existing **pins, timers, shifters and
counters**. Omitted tile fields inherit the shared declaration (or context 0 when the
resource is declared there). Pin overrides
specify the complete mode/source; pad assignments remain shared. CRC and byte
tiles have no hardware context banks. Logic-cell context overrides and
context-specific runtime writes are not exposed yet. Runtime `.set`, `.drive`
and `.release` operations update both banks; `.drive()` restores the shared pin
declaration (or its context 0 declaration) in both banks.

See [`examples/contexts.spt`](examples/contexts.spt) for 8N1/8E1 UART framing
and paired pin switching, and [`examples/contexts_initial_one.spt`](examples/contexts_initial_one.spt)
for starting directly in context 1. The UART test peer accepts optional `parity: "even"`
or `parity: "odd"`; its default remains 8N1. Run:

```sh
cargo test --locked
cargo run -q -- run --test examples/contexts.spt
```

## Setting flags

These are built-in controller operations; they need no `fn` definition:

```spatial
flag sending = 0, receiving = 0

export fn begin_sending() {
    set_flags(sending)             // sending=1, receiving=0
}
export fn enable_both() {
    set_flags(sending, receiving)  // sending=1, receiving=1
}
export fn stop() {
    clear_flags()                 // sending=0, receiving=0
}
```

`set_flags(...)` replaces the complete flag set: listed flags become 1 and
all unlisted flags become 0. It requires at least one flag. `clear_flags()`
takes no arguments and sets every flag to 0. The old `flags(...)` spelling
is rejected with migration guidance. Updating just one flag while preserving
the others is not exposed by this compiler yet.
