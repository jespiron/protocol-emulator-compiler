# What `spatial run` means

Build the nearest project's source, then run a **bounded experiment on the actual
pinned Verilog RTL in Verilator**. The simulation is a convenience harness, not a
new hardware model and not a universal protocol checker.

The project's `Spatial.toml` separates host stimulus from chip source:

```toml
source = "src/main.toml"

[simulation]
cycles = 4096
idle_inputs = 255

[[simulation.tx]]
shifter = "transmitter"
bytes = [85, 163, 0]

[[simulation.loopback]]
output = "tx"
input = "rx"

[[simulation.expect_rx]]
shifter = "receiver"
bytes = [85, 163, 0]
```

Names refer to resources in the compiled source, not guessed pin/FIFO numbers.
Unknown fields, absent resources, wrong directions, duplicate shifter entries and
multiple drivers of one input fail before simulation setup. TX preload is limited
to four bytes per channel because the real host FIFO holds four. This first harness
does not stream arbitrarily long host input or implement a USB host.

The source clock determines the simulation clock. Its period must be an exact,
even number of picoseconds so both half-periods fit the RTL's 1 ps precision.
For example 50 MHz and 25 MHz work; 30 MHz is rejected by `run` rather than rounded.
The compiler itself still accepts structurally valid source with that clock.

## Exactly what is observed

1. Reset via top-level pins; load the generated image and preload TX FIFOs.
2. Initialize protocol inputs from `idle_inputs` (default all high).
3. Begin the observation window just before sending the host start command.
   `cycles` (16..1000000) includes controller startup and fabric configuration.
4. Follow each push-pull output to its input at the next falling clock edge.
   If output enable is off, retain that input's configured idle level. Open-drain
   loopbacks are rejected; we do not pretend a copied bit models a pull-up bus.
5. A single host reader services enabled RX shifters in declaration/channel order,
   round-robin. Select/status/read commands consume real clock cycles. Only host
   reads completed within the window appear in `rx.json`.
6. At the final observation edge, stop the reader. Queued bytes and reads still
   in progress are not drained afterward. Longer observation may be needed for
   a final packet; this window is not a hardware packet boundary.
7. Compare each requested expected RX list exactly, including order and extra
   observed bytes. Fail on mismatch or an observed RX error tag. Without expected
   lists, report **observation complete**, not protocol correctness.

`trace.csv` records changes of `uio_in`, `uio_out` and `uio_oe`, labeled by cycle
within this window. `simulation.log` contains tool output. A fresh temporary results
file is required every run; no tests, skipped/failed/error tests, missing or malformed
results fail the command. Subprocess timeouts terminate their process groups.

The host reader can itself be too slow for some designs. A matching expected prefix
does not prove no overflow or no pending data. This harness provides finite test
observations; FIFO capacity/lifetime proofs and protocol-specific monitors remain
separate work. Our independent UART TX/RX tests also check timing/data to avoid
relying solely on TX/RX loopback agreement.

## Setup and storage

The installed binary embeds `run.py`, `experiment.py`, the runner's dependency
manifest and uv lockfile. It fetches public RTL commit
`58da19f41c4fc91d68bd052c0105c45efbcef8ee` into `target/spatial/hardware` and verifies
that revision and clean source/test files on every run. It never checks out or
modifies a teammate's worktree. Python dependencies live in `target/spatial/runner`.

The actual Verilator build uses a fresh temporary copy of the sources and the small simulation Python environment. This avoids
the upstream Makefile's unquoted-path issues when your project path has spaces.
It also means Verilator currently recompiles on each run; caching that build is a
future speed improvement. Tool versions are diagnosed, not installed through a
system package manager. First setup needs network access.
