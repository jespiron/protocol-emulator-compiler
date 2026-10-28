# What we are doing, and what is still unfinished

Updated 2026-10-27. This file keeps the work from drifting when visual ideas arrive.

## Original work

1. USB low-speed device transactions on existing spatial RTL first, enumeration
   later. Completed: packet receiver overflow verdict fix; independent transaction
   oracle/tests. Not completed: transaction engine executing on the chip. The
   current receiver's buffering limit is proven; architecture-wide impossibility
   is not. Branch: jess/spatial-usb-transactions in protocol-emulator.
2. First spatial compiler, originally typed Python. Completed on parent branch:
   resource-checked TOML -> startup image, 31 tests, generated UART TX/full-duplex
   RTL validation. Stack: compiler reset -> Python starter -> current Rust port.

## Current requested work, in order

- Explain the language, what the compiler enforces and what it does not, in plain
  caveman words with concrete source examples. Jess will edit the prose later.
- Port to Rust without discarding verified behavior. Keep Python as a migration
  test oracle, compare emitted bytes and rejection cases.
- Make install and project use short: spatial new / check / build / run. User
  confirmed run means local RTL simulation first, not a board uploader.
- Split parsing, constraints, encoding, project workflow and simulation into small
  modules. Generate API docs from Rust comments and test doc examples.
- Visual direction selected by Jess: palette #3 (indigo/lavender/ice blue), a
  large intricate slowly rotating Klein bottle, depth-shaded with · : + * #.
  Only operation/status text; no slogans, descriptions, bars or percentages.
  Delegate preview only; parent agent owns compiler/workflow and reviews preview.

The preview is additive. It does not replace compiler/docs work or mean USB is
finished. Cargo owns cargo-install's output; a branded installer is future work,
while the preview can show a first-launch welcome concept.

## Evidence required before calling this done

Rust fmt/clippy/tests/doctests/docs; Python differential checks; old independent
UART RTL tests driven by Rust images; installed binary outside the repo with
new/build/run and expected loopback bytes; meaningful failure-path tests. Preview
must restore the terminal on exit, be labeled mock data and change no files.
Record actual outcomes at the end of this file when verified.

## Verified compiler/workflow results

- Rust fmt, Clippy with warnings denied, unit/CLI tests, doctest and Rustdoc pass.
- Original 31 Python tests still pass; 126 valid/invalid cases agree between the
  Rust port and Python on images, allocations and rejection.
- Independent UART TX and remapped full-duplex tests pass on pinned RTL; emitted
  images also match the legacy assembler.
- Installed binary tested outside checkout, in a path with spaces. 50/25 MHz
  experiments return the right bytes and simulator times scale by exactly two.
- Extra received bytes and a too-short observation window correctly fail. Two
  simultaneous RX channels and two independent wires deliver distinct values.
- Four runner tests cover missing tools, fresh/valid results, failing commands
  and timeouts. Package contents exclude caches and virtual environments.

The Klein bottle preview is implemented and reviewed. Six preview tests and a
PTY smoke test verify geometry, shading, slow rotation, controls and terminal
restoration. It remains a preview; production commands still use plain text.
Remote CI has not yet been run for this port. USB transaction execution remains
unimplemented; this compiler work does not change that status.
