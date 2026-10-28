//! Public-API boundary tests, independent of the frontend's private structures.
use spatial_compiler::compile;

#[test]
fn invalid_sources() {
    for (source, category) in [
        ("bogus=1", "schema"),
        ("clock_hz=true", "schema"),
        ("data=[256]", "schema"),
        ("[[pins]]\nname='x'\npads=[8]", "pin-pad"),
        ("[[pins]]\nname='x'\npads=[]", "pin-pad"),
        ("[[pins]]\nname='x'\nmode='push_pull'", "pin-source"),
        ("[[pins]]\nname='x'\nsource='one'", "pin-source"),
        (
            "[[pins]]\nname='x'\n[[timers]]\nname='x'\nperiod_cycles=1",
            "name",
        ),
        ("[[pins]]\nname='zero'", "name"),
        ("[[timers]]\nname='t'\nperiod_ns=21", "timing"),
        ("[[timers]]\nname='t'\nperiod_cycles=0", "range"),
        ("[[timers]]\nname='t'\nperiod_cycles=65537", "range"),
        (
            "[[timers]]\nname='t'\nperiod_cycles=4\nresync='one'",
            "signal-class",
        ),
        (
            "[[timers]]\nname='t'\nperiod_cycles=4\nstart='fire0'",
            "signal",
        ),
        (
            "[[timers]]\nname='t'\nperiod_cycles=4\nhold_invert=true",
            "timer-hold",
        ),
        ("[[shifters]]\nname='s'\nbits=17", "range"),
        ("[[shifters]]\nname='s'\npush=true", "rx-source"),
        ("[[shifters]]\nname='s'\npull=true\nvalue=1", "tx-source"),
        (
            "[[pins]]\nname='x'\nmode='push_pull'\nsource='missing.out'",
            "signal",
        ),
        (
            "[[pins]]\nname='x'\nmode='push_pull'\nsource='x.rise'",
            "signal-class",
        ),
    ] {
        assert_eq!(compile(source).unwrap_err().code, category, "{source}");
    }
}
#[test]
fn budgets_and_greedy_trap() {
    for (kind, capacity, fields) in [
        ("pins", 8, ""),
        ("timers", 4, "period_cycles=64"),
        ("shifters", 2, "bits=8"),
    ] {
        let source = (0..capacity)
            .map(|i| format!("[[{kind}]]\nname='r{i}'\n{fields}\n"))
            .collect::<String>();
        assert!(compile(&source).is_ok());
        assert!(compile(&(source + &format!("[[{kind}]]\nname='extra'\n{fields}"))).is_err());
    }
    let result = compile("[[pins]]\nname='a'\npads=[0,1]\n[[pins]]\nname='b'\npads=[0]").unwrap();
    assert_eq!(result.report.pins["a"], 1);
    assert_eq!(result.report.pins["b"], 0);
}
#[test]
fn byte_path_and_register_path_differ() {
    for direction in ["pull", "push"] {
        for bits in [8, 9, 16] {
            let mut source = format!("[[shifters]]\nname='s'\nbits={bits}\n{direction}=true\n");
            if direction == "push" {
                source += "sample='one'\nsource='rx.level'\n[[pins]]\nname='rx'";
            }
            assert_eq!(compile(&source).is_ok(), bits == 8);
        }
    }
    assert!(compile("[[shifters]]\nname='s'\nbits=16\nvalue=65535").is_ok());
    assert!(compile("[[shifters]]\nname='s'\nbits=8\nvalue=256").is_err());
}
#[test]
fn constants_and_exact_timing() {
    let result = compile("[[pins]]\nname='bus'\nmode='open_drain'\npads=[3]\nsource='one'\n[[timers]]\nname='t'\nperiod_ns=1280").unwrap();
    assert!(result.report.config.contains(&(0x1b, 0xb0)));
    assert!(result.report.config.contains(&(0, 63)));
    assert!(
        compile("clock_hz=1000000000\n[[timers]]\nname='t'\nperiod_ns=1000000000000000").is_err()
    );
}
#[test]
fn independent_decode_and_sram() {
    let mut source = "[[pins]]\nname='a'\nmode='push_pull'\nsource='one'".to_owned();
    let c = compile(&source).unwrap();
    let mut decoded = Vec::new();
    let mut operand = 0;
    let mut at = 0;
    while decoded.len() < 4 {
        let byte = c.image[at];
        at += 1;
        operand |= usize::from(byte & 15);
        if byte >> 4 == 0 {
            operand <<= 4;
        } else {
            decoded.push((byte >> 4, operand));
            operand = 0;
        }
    }
    assert_eq!(
        decoded,
        [(2, at), (7, c.report.config.len()), (14, 9), (4, at - 3)]
    );
    assert_eq!(c.report.config.last(), Some(&(0x31, 1)));
    for (&(address, value), bytes) in c
        .report
        .config
        .iter()
        .zip(c.image[at..c.report.data_offset].as_chunks::<3>().0)
    {
        assert_eq!(
            (address, value),
            (bytes[0], u16::from_le_bytes([bytes[1], bytes[2]]))
        );
    }
    source = format!(
        "data=[{}]\n{source}",
        vec!["0"; 1024 - c.image.len()].join(",")
    );
    assert_eq!(compile(&source).unwrap().image.len(), 1024);
    assert!(compile(&source.replacen("data=[", "data=[0,", 1)).is_err());
}
#[test]
fn examples() {
    for source in [
        include_str!("../examples/uart_tx.toml"),
        include_str!("../examples/uart_full_duplex.toml"),
    ] {
        let c = compile(source).unwrap();
        assert!(c.image.len() < 1024);
    }
    let c = compile(include_str!("../examples/uart_full_duplex.toml")).unwrap();
    assert_eq!(c.report.shifter_fifo_channels["receiver"], 0);
    assert_eq!(c.report.shifter_fifo_channels["transmitter"], 1);
    assert_eq!(c.report.pins["rx"], 7);
}
