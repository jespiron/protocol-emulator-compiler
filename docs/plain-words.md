# Chip small. Compiler counts rocks.

We have a chip with a fixed box of hardware workers. A timer counts clock ticks.
A shifter turns a byte into a line of bits, or bits back into a byte. A pin is a
physical wire leaving the chip. These workers can run at the same time.

Our compiler lets us give workers names and connect them to [fill this in ? connec to what?] by configuring a TOML file.

Our compiler emits a small program for the chip's PX8 controller. This program sets up the workers, enables them, then stays in a loop while they work.

## Language today: describe the wiring

This is a hardware configuration language. It has named resources and connections.
It does not yet have functions, loops, protocol declarations, or a USB keyword.
Order of declarations is not order of execution. A reference can name a resource
that appears later in the file. Timer/shifter declaration order chooses their
hardware slots; pin placement uses the allowed-pad lists.

Small working example: make a wire alternate low and high.

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

`beat` counts 64 chip clocks, then changes its output level. `wire` copies that
level onto physical pad 0. Low-to-high-to-low takes 128 clocks. No CPU loop has
to toggle the wire one bit at a time. The boot code only sets things up.

A name like `beat.out` means “the current level of beat's output.” A name like
`beat.rise` means “the one-clock event when beat changes low to high.” A pulse is
a doorbell; a level is whether the door is currently open. Some hardware sockets
accept either; others can read only levels. The compiler checks the actual socket.

## Compiler rejects program when...

**You ask for more physical wires than we own.** In this example, two different wires ask for pad 0 and thus get rejected.

```toml
[[pins]]
name = "a"
pads = [0]

[[pins]]
name = "b"
pads = [0]
```

You can change `a` to `pads = [0, 1]` and the compiler will be happy since it can can put `a` on 1 and `b` on 0. 

**You ask for too many workers.** Suppose we have four timers and two shifters
If you ask for a fifth timer or a third shifter, the compiler rejects your program.

Currently we count all declared workers as a worker that is in use, even if you intended
to use some workers at different times.

PLANNED for future is ability for compiler to have more granular way of keeping track of which workers are not in use, so that workers can be shared for multiple tasks.

**You connect a wire to a nonexistent field.** Like `source = "missing.out"` fails because no worker
named `missing` exists.

**You try to load data in a module that does not match its width.** Suppose a shifter can hold up to 16 bits, but its automatic FIFO path only carries eight bits. This fails:

```toml
[[shifters]]
name = "too_wide"
bits = 16
pull = true
```

This fits the register path instead:

```toml
[[shifters]]
name = "constant_word"
bits = 16
value = 65535
```

That second example only configures a word. It still needs start/shift signals
and an output pin to transmit anything. “Fits” does not mean “does useful work.”

**You ask for half a clock tick.** At 50 MHz, each clock tick is 20 ns.
`period_ns = 1280` becomes exactly 64 clocks. `period_ns = 21` is rejected.
We do not quietly round it. The compiler does not prove that the real chip can
run at every clock frequency you write down.

**Your backpack exceeds 1024 bytes.** Startup instructions, configuration values,
and optional `data = [...]` all live in the same 1 KiB SRAM. The compiler counts
the final encoded bytes, not an estimate. Those optional data bytes just sit in
memory; they do not automatically become bytes to transmit.

## What we have not proved

The compiler can say “the worker count fits and the connections can be encoded.”
It cannot yet say “your USB handshake happens before its deadline,” “your host
reads fast enough to prevent overflow,” or “this design obeys every protocol
rule.” Those need scheduling/lifetime constraints and independent wire tests.
LUTs, CRC blocks, counters, context switching and arbitrary controller programs
exist in the hardware but are not exposed by this first language version.

Separately, the USB work fixed a packet receiver that said “good packet” after
losing bytes when its output buffer filled. We also wrote a reference model for
transaction rules such as retries and duplicate suppression. That model is a
specimen to compare against, not a USB transaction engine running on the chip.
