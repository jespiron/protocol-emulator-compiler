# Ethernet and Classical CAN: proposed language support

Status: design sketches, not executable programs. The `.spt` blocks below mix
existing syntax with explicitly proposed syntax. They are outside `examples/`
so CI does not imply they compile. No RTL change is implemented by this PR.

The compiler pins `protocol-emulator` at
`58da19f41c4fc91d68bd052c0105c45efbcef8ee`. Its Python protocol library already
implements 10BASE-T transmit/receive and standard-identifier CAN transmit and
receive-with-ACK demonstrations. “Needs RTL support” therefore depends on the
scope: reproducing these demos mainly requires compiler work; delivering an
interoperable MAC or a complete CAN controller is a larger task.

## 10BASE-T

First milestone: port the existing separate TX and RX demonstrations, with the
same external line driver, magnetics and comparator assumptions. This is raw
Manchester signalling on GPIO, not MII/RMII to an external digital PHY.

A proposed transmit program should still expose how the hardware is wired:

```spatial
clock 40 MHz
// Existing clock/CRC primitives; phases must be verified against pin traces.
timer half_bit { every: 2 cycles, idle: 1, run: 1 }
// PROPOSED: byte burst ends at an explicit packet tag, not FIFO starvation.
shifter serial {
    frame: [data(8)], shift: half_bit.fall,
    burst: until packet_end,
}
// PROPOSED: packet-stage events distinguish preamble, payload and FCS.
crc fcs {
    width: 32, poly: 0x04C11DB7, check: 0xC704DD7B,
    absorb: half_bit.fall, data: serial.out,
    absorb_while: payload_phase, emit: half_bit.fall,
    emit_while: fcs_phase, restart: frame_begin,
}
// PROPOSED library component, expanded into registered logic and a counter;
// not an Ethernet hardware opcode. Produces 32 complemented FCS bits.
let wire_bit = select(serial.busy, serial.out, fcs.out)
reg delayed_half = half_bit.out
reg manchester = wire_bit == delayed_half
// PROPOSED paired pin contexts choose link-pulse versus frame sources atomically.
context link { pin td_p: out = link_pulse.out; pin td_n: out = 0 }
context frame { pin td_p: out = positive; pin td_n: out = negative }
// PROPOSED API: pads/preamble, stream the body, append FCS, enforce IPG;
// service normal link pulses independently. Not implemented by this sketch.
export fn transmit(frame: bytes via serial) { /* frame sequencing */ }
```

`payload_phase`, `fcs_phase`, `frame_begin`, `positive`, `negative`, and
`packet_end` are design placeholders, not undeclared names the compiler accepts.
The real TX port must reproduce the Python program's phase and TP_IDL logic;
CRC coverage excludes preamble/SFD and includes padding. Packet end must be
explicit so a delayed host cannot silently truncate a frame.

| Gap | Where to implement | Acceptance evidence |
| --- | --- | --- |
| Two contexts and atomic pin-source selection | Compiler: expose existing config addresses `0x34/0x35` | Link/frame switch has no extra differential edge |
| Tagged bytes, burst boundaries, packet-stage events | Compiler: expose existing shifter burst registers `0x32/0x33`, byte tiles `0x60–0x7F`, host EOP/ERR tags | Back-to-back frames, zero bytes available mid-frame, exact EOP placement |
| Frames stored in SRAM | Compiler: data tables and bounded SRAM-to-TX streaming already possible in PX8 | Match Python frame/FCS tests at 40 MHz |
| Sustained maximum-size frame traffic | Host transport and buffering design; generic FIFO/DMA RTL only if measured throughput requires it | No RX overrun at one byte per 32 core clocks; full frames despite shared 1 KB SRAM |
| RX timing robustness | Validation first; generic timer/edge-capture RTL if phase/frequency sweeps fail | Independent sender phase, rate offset, jitter, bad Manchester/FCS/truncation |
| Half-duplex MAC behavior, if claimed | Design carrier/collision detection, jam/backoff and retry sequencing; determine generic resource budget before RTL changes | Two transmitters collide, back off and retry successfully |
| Electrical interoperability | Board design and measurements, not compiler syntax | Waveform/link testing with an actual link partner |

Do not add an Ethernet-specific RTL block merely to expose a feature that
already exists. Simultaneous TX/RX is a separate resource-budget experiment:
the existing separate programs are not evidence that both fit at once.

## Classical CAN

First milestone: standard 11-bit identifiers, data frames with 0–8 payload
bytes, existing stuffing/CRC15/ACK behavior. Explicitly exclude CAN FD and do
not claim extended identifiers, remote frames or full controller conformance
until they have implementations and tests.

```spatial
clock 40 MHz
const BIT = 40 cycles
pin rx: in                       // external CAN transceiver RXD
pin tx: out = bus_bit             // external transceiver TXD, recessive = 1
reg dominant_edge = rx.fall
timer sample {
    every: BIT, first: 30 cycles, pulse,
    run: dominant_edge, resync: dominant_edge,
}
counter run { count: sample.tick, streak: equal(rx), limit: 5 }
reg consume = sample.tick && !run.at_least
crc check {
    width: 15, poly: 0x4599, check: 0,
    absorb: consume, data: rx, restart: frame_begin,
}
// PROPOSED packet fields/byte tiles: map received header, DLC and payload,
// mark an explicit end, and report stuffing/form/CRC errors separately.
packet received { source: destuffed; end: frame_end; error: receive_error }
// PROPOSED bounded timing correction, rather than restarting the sample
// timer unconditionally on every dominant edge.
sample.resync_limit = 4 cycles
// PROPOSED shared-bus safety: qualify idle, monitor arbitration and release
// the transmitter immediately when a recessive bit loses to a dominant bit.
reg arbitration_lost = arbitrating && intended_bit && !rx
on arbitration_lost { release transmitter; continue receiving }
export fn send(id: u11, payload: bytes) { /* queue, arbitrate, ACK, retry */ }
export stream frames: received -> host
```

Only the basic timer/counter/CRC declarations resemble today's accepted
syntax. `packet`, `u11`, `on`, `release transmitter`, `sample.resync_limit`, and
all frame-phase names above are proposals. A real receiver cannot simply feed
all bits into CRC: it must exclude stuff bits and manage header/data/CRC/tail
phases. The existing Python receiver is the baseline, including its specific
CRC-delimiter convention; this sketch does not replace it.

| Gap | Where to implement | Acceptance evidence |
| --- | --- | --- |
| Port current standalone TX and receive-with-ACK demos | Compiler: byte tiles, burst/abort configuration, packet metadata, controller field extraction | Independent transmitter/receiver, stuffing boundaries and corrupted CRC |
| Safe bus joining | Current RX uses all 8 logic cells, 4 timers, 2 shifters, 2 counters and CRC. First try a new mapping; if it cannot fit, add a generic idle/run qualifier or reusable predicate capacity | Arm mid-frame; no false ACK; wait for qualified recessive idle |
| Arbitration and bit monitoring | Generic transmit abort/release and latched cause, if existing burst-abort cannot meet the deadline | Two identifiers transmit together; loser releases within the bit and receives winner |
| Bounded synchronization | Extend generic timer resynchronization with a programmable correction bound, after checking existing behavior against CAN timing | Sweep oscillator offset/phase; no unrestricted phase jump or lost sample |
| Error signalling and fault confinement | Resource/architecture design for error frames and TEC/REC state; generic counters/status mechanisms preferred | Stuff/form/ACK/CRC/bit errors, error-passive and bus-off/recovery scenarios |
| Retransmission and TX buffering | Compiler/controller plus generic replay storage if 1 KB SRAM/FIFOs cannot retain a frame until ACK | ACK absent, arbitration lost and retry; no payload loss or duplication |
| Electrical bus | External CAN transceiver and termination | Wired-bus interoperability with independent nodes |

Arbitration, error signalling and error confinement are part of controller
behavior, not optional CRC decorations. See the manufacturer's
[CAN bit-stream processor description](https://docs.amd.com/r/en-US/pg096-can/Bit-Stream-Processor)
and [fault confinement documentation](https://onlinedocs.microchip.com/oxy/GUID-B822915F-C375-4172-91BD-AB6F326EB783-en-US-1/GUID-7D190839-A6DB-403E-9C9A-71C7F894A376.html).

## Implementation order

1. Expose existing contexts, byte tiles, burst/abort and EOP/ERR metadata with
   compiler validation and targeted encoding tests; no RTL modification yet.
2. Port and compare the existing Python programs cycle-for-cycle at their
   documented clocks, without silently expanding their interoperability claims.
3. Add independent phase/error/resource-pressure tests to identify actual
   deadlines or capacity failures.
4. Specify and verify the smallest generic RTL changes those failures require;
   rerun formal, synthesis, timing/area and physical signoff before updating the
   compiler's pinned hardware revision.
