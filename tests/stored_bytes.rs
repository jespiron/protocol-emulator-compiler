//! Stored-table streaming and long-protocol test-budget validation.
use spatial::compile;
fn reject(source: &str, message: &str) {
    let e = compile(source).expect_err("invalid stored transfer must fail");
    assert!(format!("{e:?}").contains(message), "{e:?}");
}
#[test]
fn stored_transfer_claims_fifo_and_stays_within_sram() {
    let b = compile(include_str!("../examples/stored_bytes.spt")).unwrap();
    assert_eq!(b.plan["calls"]["channel"], 1);
    assert!(b.image.len() < 1024);
}
#[test]
fn stored_tables_are_interned_across_functions() {
    let data = [13, 149, 77, 212, 83, 19, 238, 101];
    let b = compile("shifter s {} const DATA = [13,149,77,212,83,19,238,101] export fn a() { s.send(DATA) } export fn b() { s.send(DATA) }").unwrap();
    assert_eq!(
        b.image
            .windows(data.len())
            .filter(|window| *window == data)
            .count(),
        1
    );
}
#[test]
fn validates_stored_transfer_source_and_ownership() {
    reject("shifter s {} export fn a() { s.send([]) }", "1..1024 bytes");
    reject("shifter s {} export fn a() { s.send([256]) }", "0..255");
    reject(
        "shifter s {} export fn a(data: bytes) { s.send(data) }",
        "compile-time bytes",
    );
    reject(
        "shifter s {} export stream out: host -> s export fn a() { s.send([1]) }",
        "already exchanges bytes",
    );
    reject(
        "shifter s {} export fn a(n: u8) { repeat n { s.send([1]) } }",
        "loop inside a loop",
    );
}
#[test]
fn long_test_budget_is_explicit_and_bounded() {
    let b = compile("clock 40 MHz test \"long\" step_timeout 60 ms { wait 1 us }").unwrap();
    assert_eq!(b.plan["tests"][0]["step_timeout_cycles"], 2400000);
    reject(
        "test \"bad\" step_timeout 0 cycles {}",
        "step_timeout must be",
    );
    reject(
        "test \"bad\" step_timeout 40000001 cycles {}",
        "step_timeout must be",
    );
}
#[test]
fn ethernet_program_fits_the_existing_chip() {
    let b = compile(include_str!("../examples/ethernet_tx.spt")).unwrap();
    assert!(b.usage.contains(&("logic cells", 8, 8)));
    assert!(b.usage.contains(&("timers", 4, 4)));
    assert!(b.image.len() < 1024);
}

#[test]
fn stored_tables_still_share_sram_with_instructions() {
    let data = vec!["1"; 1024].join(",");
    reject(
        &format!("shifter s {{}} export fn a() {{ s.send([{data}]) }}"),
        "chip has 1024",
    );
    reject(
        "shifter s { frame: [data(16)] } export fn a() { s.send([1]) }",
        "at most 8 bits",
    );
}
