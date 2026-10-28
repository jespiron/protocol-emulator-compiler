# Small modules, one compiler

| File | Job |
|---|---|
| `src/lib.rs` | Public `compile(&str)` API, target identity, errors and report; tested Rustdoc example |
| `src/frontend.rs` | Deserialize strict TOML and validate field combinations/ranges |
| `src/constraints.rs` | Assign finite pools; find distinct pads from allowed lists |
| `src/backend.rs` | Resolve signals, encode registers, emit startup code and check image size |
| `src/project.rs` | Discover project, validate experiment references, publish paired artifacts |
| `src/simulator.rs` | Unpack embedded simulation assets and start the managed runner |
| `src/main.rs` | Short CLI commands and human-readable status |
| `src/preview.rs` | Ratatui Klein bottle preview; sample status text only |
| `assets/simulator/` | Python/cocotb testbench and tool orchestration, locked with uv |

The compiler library performs no filesystem or process operations. Its public
entry point accepts source text, so callers cannot bypass checks by constructing
an invalid intermediate structure. Implementation structs stay private. There is
one crate because separate crates would not yet buy us useful isolation.

`///` comments on the public Rust API generate `cargo doc --no-deps` output.
`//!` comments explain module responsibilities. `missing_docs` and unsafe code
are denied; `cargo test` executes the API example. More comments do not substitute
for straightforward types, names and module boundaries.

## Keeping the Python work

`tests/reference/python/` preserves the original implementation, tests and examples.
It is a migration oracle, not a second supported CLI. `tests/differential.py`
compares Rust/Python images, allocations and acceptance for 126 deterministic
sources, including random allowed-pad graphs. Rust's own tests independently
check capacities, instruction decoding, exact SRAM boundaries and all 343
three-pin/three-pad candidate graphs against a brute-force feasibility oracle.

One intentional semantic tightening: `hold_invert=true` without `hold` now fails
instead of silently doing nothing. TOML deserialization diagnostics may use
`schema` where Python used `range`; rejection remains mandatory. The output report
adds configuration writes and resource modes used by simulation.

Python remains only for test infrastructure: cocotb is the bridge into Verilator.
An installed Rust compiler can `check` and `build` without Python, uv or this repo.
`run` embeds its own runner and lockfile, provisions dependencies through uv, and
fetches an exact RTL commit. It does not import the Python reference compiler.

## What keeps builds reviewable

We stage the binary and report in a fresh directory before publishing them together.
Invalid input does not overwrite previous output. Source paths under the generated
directory and symlinked output paths are rejected. Interrupted publication leaves
an explicit recovery directory rather than silently mixing generations. This is
not a crash-durable database or a multi-process build scheduler.

The preview currently has no production progress wiring. Its proposed statuses are
mock, clearly labeled and separate from compiler decisions. Jess chose the lavender/indigo palette and a rotating Klein bottle. Real
build/setup/run phase events can feed the renderer in the next UI integration. Plain
text remains the default for scripts/CI. A separate branded installer is future
work; Cargo controls `cargo install` output.
