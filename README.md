# Spatial compiler

A typed Python compiler for the PX8 spatial fabric. TOML descriptions name pins,
timers and shifters; the compiler binds resources, checks constraints, and emits
an executable SRAM image. It does not generate protocol-specific RTL.

```sh
uv sync --locked
make check
uv run --locked spatialc examples/uart_full_duplex.toml -o /tmp/duplex.bin --report /tmp/duplex.json
```

The image configures the fabric, arms it last, then loops. **Reset the chip before
loading.** HALT would disable the fabric, so the boot program does not halt.
Automatic shifter `k` uses FIFO channel `k`; the report gives the assignments.
The examples use 64 clocks per UART bit at 50 MHz (781250 baud). Full duplex
intentionally puts RX on channel 0/pad 7 and TX on channel 1/pad 6.

## What is checked

- Eight **physical** `uio` pads shared by input/output/open-drain signals.
  `ui_in` and `uo_out` are reserved for the host. Distinct logical pins never alias.
- Allowed-pad matching, globally unique names, at most four timers/two shifters.
- Signal existence and selector compatibility; integer field widths; no silent wrapping.
- Byte-only automatic FIFOs: widths above eight are rejected with pull/push.
  Register-mode shifters support up to 16 bits with a constant `value`.
- Exact conversion of integral nanoseconds to clock cycles; no rounding.
- The complete code + configuration + opaque data image fits 1024 bytes.

Diagnostics identify the violated constraint and offending resource. For example,
a ninth distinct pin is rejected even if some pins are inputs and others outputs.

## Source contract

See the runnable examples and [constraints](docs/constraints.md). Unknown keys are
errors. `pins`, `timers`, and `shifters` are arrays of tables. `clock_hz` defaults
to 50 MHz; optional top-level `data` is a byte array stored after configuration,
**not automatically transmitted**.

Pin `mode` is `input`, `push_pull`, or `open_drain`. `pads` lists allowed `uio`
indices (default 0..7). Driven pins require a `source`: `one`, `zero`, or a named
level signal. Input pins cannot have an output source; all modes can be sampled.
Use one logical open-drain pin for a bidirectional wire, not separate RX/TX pads.

Timers require exactly one of `period_cycles` or `period_ns`. Period is the tick
interval, so a toggling timer's full output period is twice that interval.
`phase_cycles` is the first-tick configuration, not a pin-edge latency guarantee.
The UART examples explicitly account for fabric/synchronizer pipeline latency.
Optional fields: `start`, `stop`, `idle`, `pulse`, `resync`, `hold`, `hold_invert`.

Shifter fields: `bits`, `pre`, `pre_value`, `post`, `post_value`, `parity` (0 none,
1 even, 2/3 odd, matching RTL), `msb`, `lead`, `end_on_sample`, `idle`, `shift`,
`sample`, `start`, `source`, `pull`, `push`, and register-mode `value`.
RX requires an explicit level source and sampling event. Refer to the pinned RTL
for framing semantics; these are fabric operations rather than a UART-only DSL.

Signals are `zero`, `one`, `pin.level/rise/fall`, `timer.out/rise/fall`, and
`shifter.out/active/word/stop`. A resource name replaces `pin`, `timer`, or
`shifter`. FIRE is not supported by this boot-only frontend.

## Verification

`make check` runs Ruff lint/format, mypy strict, and pytest. Allocation is compared
with a brute-force feasibility oracle on all 343 three-pin candidate graphs.
Tests cover boundaries, invalid fields, selector mismatches, bytecode decoding,
relocation and exact SRAM capacity, plus CLI failure behavior.

The RTL smoke tests compare emitted bytes with the existing assembler and run
TX plus simultaneous TX/RX through the chip's public pins:

```sh
git clone https://github.com/WilliamZhang20/protocol-emulator.git /tmp/spatial-rtl
git -C /tmp/spatial-rtl checkout 58da19f41c4fc91d68bd052c0105c45efbcef8ee
make rtl-test HW_ROOT=/tmp/spatial-rtl
```

Requires make, a C++ compiler, and Verilator >=5.036 (locally checked with 5.050).
The checked-in SRAM models are used. Python dependencies, including cocotb, are
locked. Generated compiler/RTL build files go under `build/`; the hardware sources
must match the pinned revision and remain unchanged.

This first slice does **not** lower LUTs, counters, CRCs, byte tiles, contexts,
controller programs, or USB transactions. It does not prove protocol correctness,
FIFO scheduling, external clock accuracy, or analog/physical timing compliance.
Those require independent protocol contracts and hardware tests. Requested
`clock_hz` is an external assumption, not a fresh timing-signoff result.
