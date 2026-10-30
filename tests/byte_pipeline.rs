//! Byte-stage capacity, field validation and packet-boundary checks.
use spatial::compile;
fn reject(source: &str, message: &str) {
    let error = compile(source).expect_err("invalid pipeline must fail");
    assert!(format!("{error:?}").contains(message), "{error:?}");
}
#[test]
fn pipeline_uses_existing_fixed_stages() {
    let build = compile(include_str!("../examples/byte_framing.spt")).unwrap();
    assert!(build.usage.contains(&("byte tiles", 4, 8)));
    assert!(build.usage.contains(&("shifters", 2, 2)));
}
#[test]
fn rejects_a_third_stage_and_unknown_modes() {
    reject(
        "shifter s {} export stream out: host -> s { pass {} pass {} pass {} }",
        "only two byte stages",
    );
    reject(
        "shifter s {} export stream out: host -> s { crc32 {} }",
        "expected a byte stage",
    );
}
#[test]
fn validates_escape_mappings() {
    let prefix = "shifter s {} export stream out: host -> s { escape";
    reject(
        &format!("{prefix} {{ delimiter: 0x7E }} }}"),
        "needs `escape`",
    );
    reject(
        &format!(
            "{prefix} {{ delimiter: 0x7E, escape: 0x7E, mapped_delimiter: 1, mapped_escape: 2 }} }}"
        ),
        "must be distinct",
    );
    reject(
        &format!(
            "{prefix} {{ delimiter: 0x7E, escape: 0x7D, mapped_delimiter: 0x7E, mapped_escape: 2 }} }}"
        ),
        "must be distinct",
    );
}
#[test]
fn validates_checksum_fields() {
    reject(
        "shifter s {} export packets out: host -> s { checksum { algorithm: crc16, operation: generate } }",
        "must be add or xor",
    );
    reject(
        "shifter s {} export packets out: host -> s { checksum { algorithm: add, operation: invalid } }",
        "must be generate or verify",
    );
    reject(
        "shifter s {} export packets out: host -> s { checksum { algorithm: add, operation: generate, initial: 1 } }",
        "initial",
    );
}
#[test]
fn validates_byte_ranges_and_duplicate_fields() {
    reject(
        "shifter s {} export stream out: host -> s { tag { length: 256 } }",
        "must fit in one byte",
    );
    reject(
        "shifter s {} export stream out: host -> s { filter { value: 1, mask: -1 } }",
        "must fit in one byte",
    );
    reject(
        "shifter s {} export stream out: host -> s { filter { value: 1, value: 2 } }",
        "more than once",
    );
}
#[test]
fn packet_receive_still_needs_an_actual_end_source() {
    reject(
        "pin p: in shifter s { sample: p at 1 } export packets r: s -> host { filter { value: 1 } }",
        "upstream packet boundaries",
    );
    reject(
        "shifter s {} export packets t: host -> s { pass {} }",
        "needs a shifter with `burst`",
    );
    compile(
        "pin p: in shifter s { sample: p at 1 } export packets r: s -> host { tag { length: 2 } }",
    )
    .unwrap();
}

#[test]
fn rejects_checksum_before_receive_framing() {
    reject(
        "pin p: in shifter s { sample: p at 1 } export packets r: s -> host { checksum { algorithm: add, operation: verify } tag { length: 2 } }",
        "upstream packet boundaries",
    );
}
