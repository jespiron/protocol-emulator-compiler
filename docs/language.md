# The language: a small wiring description

Rust implements the compiler. TOML describes the chip configuration. There is no
protocol-specific keyword: `serial` is just a name you chose, not a UART opcode.
The language currently has three resource kinds: pins, timers and shifters.
They can refer forward to each other. Declaration order does not mean execution
order, though timer/shifter order assigns their numbered hardware slots.

All declarations are simultaneously allocated. No dead-code removal, aliases,
time-sharing, functions, loops, imports or raw-register escape hatch exist yet.
Unknown keys are errors. Names use ASCII letters/digits/underscores, cannot start
with a digit, and are globally unique. `zero`, `one`, `fire0`, `fire1` are reserved.
FIRE events are not exposed in this boot-only language.

## Top level

| Field | Meaning | Default / check |
|---|---|---|
| `clock_hz` | Assumed input clock | 50000000; integer 1..1000000000; not physical signoff |
| `pins`, `timers`, `shifters` | Arrays of tables (`[[pins]]` etc.) | Empty |
| `data` | Opaque bytes appended to the image | Empty; each 0..255; not automatically sent |

## Pins

| Field | Meaning | Default / check |
|---|---|---|
| `name` | Name used in connections | Required |
| `mode` | `input`, `push_pull`, `open_drain` | `input` |
| `pads` | Allowed physical `uio` pads | All 0..7; nonempty; duplicates harmless |
| `source` | Level to drive, or `zero`/`one` | Required for driven pins; forbidden for input |

`pads=[0,1]` means “either pad is okay,” not “use two pads.” An augmenting-path
matching algorithm finds a distinct pad for each name. A single open-drain pin
can represent a bidirectional wire; declaring separate logical RX/TX pins does
not share a pad. This version conservatively forbids such aliases.

## Timers

| Field | Meaning | Default / check |
|---|---|---|
| `name` | Worker name | Required |
| `period_cycles` | Clock ticks per timer interval | Exactly one of this or `period_ns`; 1..65536 |
| `period_ns` | Same interval in integer ns | Exact conversion only; no rounding |
| `phase_cycles` | Initial phase register | period minus one; 0..65535 |
| `start`, `stop` | Event/level selector | `one`, `zero` |
| `idle` | Output when idle | 0; only 0/1 |
| `pulse` | Pulse rather than toggle output | false |
| `resync` | Resynchronization level selector | Absent |
| `hold` | Hold level selector | Absent |
| `hold_invert` | Invert the hold condition | false; true requires `hold` |

Tick interval is not automatically a protocol bit time. Toggle output takes two
intervals for a full cycle. `phase_cycles` is a hardware setting, not a promise
about first pin-edge latency. Our UART examples use phase 61 for TX and 30 for RX
with 64-cycle bits to account for the fabric/synchronizer pipeline.

## Shifters

| Field | Meaning | Default / check |
|---|---|---|
| `name` | Worker name | Required |
| `bits` | Word width | 8; 1..16 |
| `pre`, `post` | Framing bit counts | 0; 0..3 |
| `pre_value`, `post_value` | Framing levels | 0; 0/1 |
| `parity` | RTL parity encoding | 0 none; 1 even; 2/3 odd |
| `msb` | Most-significant bit first | false |
| `lead`, `end_on_sample` | RTL framing controls | false; see pinned `fabric_shifter.v` |
| `idle` | Output idle level | 0; 0/1 |
| `shift`, `sample`, `start` | Event/level selectors | `zero` |
| `source` | RX level selector | Required when sampling or pushing |
| `pull` | Read TX FIFO automatically | false; width must be <=8 |
| `push` | Write RX FIFO automatically | false; width <=8 and sample must not be `zero` |
| `value` | Register-mode constant TX word | 0; fits `bits`; incompatible with `pull` |

These settings configure hardware. Setting `value=42` alone does not start a
transmission. FIFO channel number equals the allocated shifter number.

## Signals

| Signal | Category |
|---|---|
| `zero`, `one` | Constant selector; separate special encoding on output pins |
| `name.rise`, `name.fall` on pin/timer | One-clock event |
| `name.level` on pin | Level |
| `name.out` on timer/shifter | Level |
| `name.active` on shifter | Level |
| `name.word`, `name.stop` on shifter | Event |

Timer start/stop and shifter start/shift/sample accept either event or level
selectors. Timer resync/hold and shifter RX source accept only level lines;
even `one` is not encodable in those restricted sockets. Output pins accept level
lines plus their dedicated constant encoding. The compiler never silently truncates
a selector to force it to fit.

## A concrete UART fragment

```toml
[[timers]]
name = "bit_clock"
period_cycles = 64
phase_cycles = 61
pulse = true
start = "sender.active"
stop = "sender.stop"

[[shifters]]
name = "sender"
bits = 8
pre = 1
post = 1
post_value = 1
idle = 1
shift = "bit_clock.rise"
pull = true

[[pins]]
name = "tx"
mode = "push_pull"
source = "sender.out"
```

The host puts bytes into the sender's FIFO. The sender emits one low start bit,
eight data bits, and one high stop bit. The timer tells it when to shift.
The output pin copies the sender. No UART opcode is involved.
At the default 50 MHz clock this example uses 781250 baud, not a standard 115200
baud example. Exact hardware-cycle timing matters; we do not silently approximate.
