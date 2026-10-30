//! Packet API validation and plan metadata.
use spatial::compile;
fn reject(source: &str, message: &str) {
    let error = compile(source).expect_err("invalid packet configuration must fail");
    assert!(format!("{error:?}").contains(message), "{error:?}");
}
#[test]
fn example_preserves_packet_data_and_error_expectations() {
    let build = compile(include_str!("../examples/packet_errors.spt")).unwrap();
    assert_eq!(build.plan["streams"]["recv"]["packets"], true);
    assert_eq!(
        build.plan["tests"][1]["steps"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["expect"]["error"],
        true
    );
}
#[test]
fn requires_usable_packet_boundaries() {
    reject(
        "shifter s {} export packets p: host -> s",
        "needs a shifter with `burst`",
    );
    reject(
        "reg last = 0 shifter s { burst, end: last } export packets p: host -> s",
        "needs `end_on_packet`",
    );
    reject("shifter s { end_on_packet }", "need `burst`");
    reject(
        "shifter s { burst, end_on_packet }",
        "needs an explicit `end`",
    );
    reject(
        "reg last = 0 shifter s { burst, end: last, end_on_packet }",
        "host-to-shifter FIFO",
    );
    reject("event e shifter s { burst, end: e }", "needs a level");
}
#[test]
fn rejects_empty_packets_and_wrong_methods() {
    let base =
        "reg last = 0 shifter s { burst, end: last, end_on_packet } export packets p: host -> s";
    reject(
        &format!(r#"{base} test "bad" {{ p.write_packet([]) }}"#),
        "empty packet",
    );
    reject(
        &format!(r#"{base} test "bad" {{ p.write([1]) }}"#),
        "packet streams use",
    );
    reject(
        r#"shifter s {} export stream p: host -> s test "bad" { p.write_packet([1]) }"#,
        "declare `export packets`",
    );
}
#[test]
fn packet_reads_require_metadata_expectations() {
    let base = "pin p: in reg last = 1 shifter s { burst, end: last, sample: p at 1 } export packets r: s -> host";
    reject(
        &format!(r#"{base} test "bad" {{ expect r.read_packet() == [1] }}"#),
        "compare against",
    );
    reject(
        &format!(r#"{base} test "bad" {{ expect r.read_packet() == packet([1], error: 2) }}"#),
        "error must be 0 or 1",
    );
}
