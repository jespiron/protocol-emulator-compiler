# Why these constraints are sound

The compiler targets spatial RTL revision
`58da19f41c4fc91d68bd052c0105c45efbcef8ee`, ABI `px8-spatial-v1`.
The acceptance claim applies to TOML parsed by this frontend and lowered by this
backend. It is structural and encoding validity, not a universal proof of
program correctness. Compiler implementation defects remain possible; the tests
below provide evidence and counterexamples, not a formal verification claim.

| Constraint | RTL evidence | Compiler rule / evidence |
|---|---|---|
| Physical pins | `project.v` wires `ui_in`/`uo_out` to host; only `uio` to core | Injective assignment to 0..7; input and output are not separate budgets |
| Timer/shifter count | Core generates 4 timers and 2 shifters | Generic finite-pool allocator, actual config addresses derive from allocation |
| Selector types | Timer/shifter selectors vs `levels = lines[63:32]` | 6-bit selectors accept either class; restricted selectors accept 32..63 only |
| Pin constants | Core pin config bit 5 enables a constant | ZERO/ONE use that encoding; never truncate a pulse index into a level selector |
| Automatic word width | Shifter TX zero-extends FIFO byte; RX stores low 8 bits | Auto pull/push reject widths >8 |
| Register widths | Timer 16-bit reload/phase; shifter length-minus-one4 | Validate before encoding; no modulo conversion of invalid values |
| SRAM | 10-bit instruction and data addresses; 1024-byte macro | Validate relocated addresses and exact whole-image length |
| Startup | `live = enable && armed`; host disables on HALT | Configure everything before arm; end in idle loop; require hardware reset |

## Pin allocation argument

Pins and allowed physical pads form a bipartite graph. Every successful augmenting
path gives one previously unassigned logical pin a pad, displacing another owner
only when that owner can be reassigned. Therefore each completed iteration retains
an injective mapping and every assignment follows an allowed edge. The visited-pad
set bounds recursion. Failure to find an augmenting path means the existing maximum
matching cannot grow; no complete assignment exists for these candidate sets.

The tests compare feasible/infeasible decisions with exhaustive assignments for all
nonempty candidate subsets on three pads and three logical pins (7^3 cases). They
also test eight vs nine pins and a case where naive greedy allocation fails.
This argument relies on all declarations being simultaneously live. The compiler
conservatively forbids temporal reuse and electrically shared logical aliases;
it may reject designs a future phase-aware allocator could realize.

## Timing and scope

Integral nanosecond requests are converted with exact integer arithmetic. A nonzero
remainder is rejected; field limits are then checked. Tick interval is not a
protocol period unless the chosen wiring makes it one. Pipeline latency,
asynchronous skew, clock accuracy and FIFO service rates need separate analysis.
No generic baud-rate or response-deadline claim follows from a timer fitting.

Only timers/shifters/pins are currently exposed. Unsupported resource kinds and
unknown source keys fail explicitly. No raw-config escape hatch lets user source
bypass allocation. Data is opaque storage, not an IN retry buffer implementation.
Next extensions must add constraints together with their lowering, not just increase
resource counters. In particular USB needs packet ownership and retry lifetime,
not merely a sum of tile counts.
