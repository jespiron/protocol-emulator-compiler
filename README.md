# Spatial

**Chip small. Compiler counts rocks.** You name hardware workers, connect them,
and ask Spatial to check whether the setup fits. It builds a startup program for
our existing PX8 chip. The workers then run in parallel.

Spatial is written in Rust. **Your source language is TOML**, not Rust.
Start with [the plain-words explanation](docs/plain-words.md): concrete examples,
what the compiler actually checks, what “spatial” means here, and what we have not
proved. Jess: this is deliberately rough, simple prose for you to edit.

## Install once; use short commands

With [Rust/Cargo installed](https://rustup.rs/):

```sh
cargo install --git https://github.com/jespiron/protocol-emulator-compiler --branch jess/spatial-compiler-rust --locked
spatial new hello
cd hello
spatial run
```

That last command compiles the project, fetches the pinned hardware, sets up the
locked Python simulation environment, and runs the actual RTL in Verilator.
The starter sends three UART bytes through a simulated wire and checks that the
receiver gets `[85, 163, 0]`. No binary name, dependency list or simulator command
to remember. This is **local simulation**, not uploading to a physical chip.

`build` and `check` need only the installed binary. `run` additionally needs
[uv](https://docs.astral.sh/uv/getting-started/installation/), Git, make, a C++
compiler and [Verilator >=5.036](https://verilator.org/guide/latest/install.html).
The runner diagnoses missing tools; it does not invoke sudo or your package manager.
The first run needs network access. Subsequent runs reuse downloaded RTL and Python
dependencies; the Verilator model currently rebuilds in a temporary directory.
Simulation is supported on macOS/Linux; native Windows support is not implemented.

```sh
spatial check      # Does the source fit? Write nothing.
spatial build      # Compile. Choose all output filenames for me.
spatial run        # Build, then execute the project's simulation experiment.
spatial preview    # Preview the chosen Klein bottle animation.
```

Commands find the nearest `Spatial.toml`, including from a subdirectory. Without
one, they tell you to create a project. `spatial build path/to/source.toml` also
works for a standalone source, with outputs beneath the current directory.

```text
hello/
  Spatial.toml             project source path + simulation experiment
  src/main.toml            workers and their wiring: edit this
  target/spatial/
    artifacts/image.bin    complete SRAM image
    artifacts/report.json  assignments, size and target revision
    trace.csv              observed pin changes during the latest simulation
    rx.json                received bytes during the observation window
    simulation.log         detailed setup/build/test output
```

The starter compiles to 66 bytes. It assigns RX to shifter/channel 0, TX to 1,
and pads 7/6. The report uses real allocated hardware indices. **Reset the chip
before loading the image**; the runner does that for you.

## Tiny source example

```toml
clock_hz = 50000000

[[timers]]
name = "beat"
period_cycles = 64

[[pins]]
name = "wire"
mode = "push_pull"
pads = [0]
source = "beat.out"
```

Every 64 clocks, `beat` changes its output. Pad 0 copies that output. A full
low/high cycle takes 128 clocks. These are hardware connections, not sequential
software statements. See the [language reference](docs/language.md) and
[UART examples](examples/uart_full_duplex.toml).

## What the checker promises

- At most **8 physical protocol pads**, 4 timers and 2 shifters; allowed pad
  lists must admit distinct assignments. The dedicated host pins are unavailable.
- Names exist, fields are recognized, and each signal fits its destination's
  selector. A pulse event is not interchangeable with a persistent level.
- Values fit hardware fields. FIFO paths carry at most 8 bits; register-mode
  shifters can hold 16. Nanosecond requests must be whole clock ticks.
- Final startup code + configuration + opaque data fit **1024 bytes**.

It does **not** prove that a protocol is correct, deadlines are met, buffers never
fill, or a requested clock is physically achievable. LUTs, counters, CRC blocks,
contexts, arbitrary controller code and USB transactions are not in the language
yet. [Constraint evidence](docs/constraints.md) explains why these particular
checks follow from the RTL.

## Rust modules and generated docs

One crate, one library, one executable. Parsing, resource assignment and encoding
are separate from project files, simulation and the visual preview. Rustdoc
comments describe the public API; examples in those comments are executable tests.

```sh
cargo doc --no-deps --open
cargo test --locked
```

[Implementation map](docs/implementation.md) explains the modules and how we kept
the Python work as a test reference instead of maintaining two user-facing tools.

## Develop and verify

```sh
cargo run --locked          # The repo itself is a starter project; build + simulate it.
make check                 # Rust checks + reference/differential/runner tests.
make rtl-test              # Independent UART TX and full-duplex RTL tests.
```

`make rtl-test` downloads the same pinned hardware if needed, compares Rust images
with the legacy assembler, and checks UART data and timing at the chip pins. The
bounded loopback experiment is convenient; these separate tests prevent a broken
TX and broken RX from merely agreeing with each other.

See [simulation semantics](docs/simulation.md), [visual previews](docs/ui-preview.md)
and [work status](docs/work-status.md). Nothing here claims USB transactions are
implemented on the chip.
