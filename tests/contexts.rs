//! Context resource accounting and diagnostics.
use spatial::compile;

fn reject(source: &str, message: &str) {
    let error = compile(source).expect_err("invalid context must fail");
    assert!(format!("{error:?}").contains(message), "{error:?}");
}

#[test]
fn contexts_share_physical_resources_and_inherit_defaults() {
    let build = compile(include_str!("../examples/contexts.spt")).unwrap();
    assert!(build.usage.contains(&("shifters", 1, 2)));
    assert!(build.usage.contains(&("timers", 1, 4)));
    assert_eq!(build.tests.len(), 3);
}

#[test]
fn rejects_third_context() {
    reject("flag phase contexts phase context 2 {}", "between 0 and 1");
}

#[test]
fn rejects_pulse_selector() {
    reject("event go contexts go", "must be a level");
}

#[test]
fn rejects_missing_or_duplicate_selector() {
    reject("context 1 {}", "needs `contexts <level>`");
    reject("flag a contexts a contexts a", "only one");
}

#[test]
fn rejects_new_resources_and_pad_changes() {
    reject(
        "flag a contexts a context 1 { timer t { every: 4 cycles } }",
        "declare the shared tile",
    );
    reject(
        "flag a contexts a pin p: out = 0 context 1 { pin p @ 2: out = 1 }",
        "pad assignment",
    );
}

#[test]
fn rejects_shared_crc_and_duplicate_bank() {
    let crc = "crc c { width: 16, poly: 0x8005, check: 0 } flag a contexts a";
    reject(
        &format!("{crc} context 1 {{ crc c {{ width: 8 }} }}"),
        "only existing timers",
    );
    reject(
        "flag a contexts a context 1 {} context 1 {}",
        "already defined",
    );
}

#[test]
fn rejects_duplicate_overrides_and_fields() {
    let base = "pin p: out = 0 flag a contexts a";
    reject(
        &format!("{base} context 1 {{ pin p: out = 1 pin p: out = 0 }}"),
        "overridden twice",
    );
    reject(
        "timer t { every: 8 cycles, run: 1 } flag a contexts a context 1 { timer t { every: 4 cycles, every: 6 cycles } }",
        "duplicate context field",
    );
}

#[test]
fn rejects_unsupported_declarations_in_context() {
    reject(
        "flag a contexts a context 1 { flag b }",
        "only overrides existing",
    );
}
