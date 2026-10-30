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
    reject(
        "flag phase contexts(select: phase) { context 2 {} }",
        "must be 0 or 1",
    );
}

#[test]
fn rejects_pulse_selector() {
    reject(
        "event go contexts(select: go) { context 0 {} context 1 {} }",
        "must be a level",
    );
}

#[test]
fn requires_grouped_syntax_and_both_contexts() {
    reject("context 1 {}", "contexts(select:");
    reject("flag a contexts a", "expected `(`");
    reject(
        "flag a contexts(select: a) { context 1 {} }",
        "include both",
    );
    reject(
        "flag a contexts(select: a) { context 0 {} context 1 {} } contexts(select: a) { context 0 {} context 1 {} }",
        "only one",
    );
}

#[test]
fn rejects_context_one_only_resources_and_pad_changes() {
    reject(
        "flag a contexts(select: a) { context 0 {} context 1 { timer t { every: 4 cycles } } }",
        "declare the shared tile",
    );
    reject(
        "flag a pin p: out = 0 contexts(select: a) { context 0 {} context 1 { pin p @ 2: out = 1 } }",
        "pad assignment",
    );
}

#[test]
fn rejects_shared_crc_and_duplicate_bank() {
    reject(
        "flag a contexts(select: a) { context 0 { crc c { width: 16, poly: 0x8005, check: 0 } } context 1 { crc c { width: 8 } } }",
        "only existing timers",
    );
    reject(
        "flag a contexts(select: a) { context 1 {} context 1 {} }",
        "already defined",
    );
}

#[test]
fn rejects_duplicate_overrides_and_fields() {
    reject(
        "flag a contexts(select: a) { context 0 { pin p: out = 0 } context 1 { pin p: out = 1 pin p: out = 0 } }",
        "overridden twice",
    );
    reject(
        "flag a contexts(select: a) { context 0 { timer t { every: 8 cycles, run: 1 } } context 1 { timer t { every: 4 cycles, every: 6 cycles } } }",
        "duplicate context field",
    );
}

#[test]
fn rejects_unsupported_declarations_in_context() {
    reject(
        "flag a contexts(select: a) { context 0 {} context 1 { flag b } }",
        "only overrides existing",
    );
}

#[test]
fn context_order_does_not_allocate_extra_hardware() {
    let build = compile("flag a = 1 contexts(select: a) { context 1 { pin p: out = 1 } context 0 { pin p: out = 0 } }").unwrap();
    assert!(build.usage.contains(&("pins", 1, 8)));
}

#[test]
fn validates_flag_initializers() {
    reject("flag a = 2", "0 or 1");
    compile("flag a = 0, b = 1").unwrap();
}
