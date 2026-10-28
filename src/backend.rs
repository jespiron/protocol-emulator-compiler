//! Lower only validated programs. The controller boots the parallel fabric, then idles.
use crate::{
    ABI, Compilation, CompileError as Error, HARDWARE_REVISION, Report, constraints,
    frontend::Program,
};
use std::collections::BTreeMap;

fn instruction(opcode: u8, operand: u16) -> Vec<u8> {
    let mut digits = Vec::new();
    let mut operand = operand;
    loop {
        digits.push((operand & 15) as u8);
        operand >>= 4;
        if operand == 0 {
            break;
        }
    }
    digits.reverse();
    *digits.last_mut().expect("at least one digit") |= opcode << 4;
    digits
}
fn address_instruction(opcode: u8, address: usize) -> Result<Vec<u8>, Error> {
    if address >= 1024 {
        return Err(Error::new("range", "SRAM address must fit 0..1023"));
    }
    Ok(vec![
        (address >> 8) as u8,
        ((address >> 4) & 15) as u8,
        opcode << 4 | (address & 15) as u8,
    ])
}
fn encode_image(config: &[(u8, u16)], data: &[u8]) -> Result<(Vec<u8>, usize), Error> {
    if config.last() != Some(&(0x31, 1)) {
        return Err(Error::new(
            "arm-order",
            "last configuration write must arm the fabric",
        ));
    }
    let count = u16::try_from(config.len())
        .map_err(|_| Error::new("range", "too many configuration writes"))?;
    let mut body = instruction(7, count);
    body.extend(instruction(14, 9));
    let idle = 3 + body.len();
    let mut image = address_instruction(2, idle + 3)?;
    image.extend(body);
    image.extend(address_instruction(4, idle)?);
    let code_size = image.len();
    for &(address, value) in config {
        image.push(address);
        image.extend(value.to_le_bytes());
    }
    let offset = image.len();
    image.extend(data);
    if image.len() > 1024 {
        return Err(Error::new(
            "sram-capacity",
            format!(
                "image needs {} bytes: code={code_size}, config={}, data={}; available=1024",
                image.len(),
                config.len() * 3,
                data.len()
            ),
        ));
    }
    Ok((image, offset))
}

pub(crate) fn compile(p: Program) -> Result<Compilation, Error> {
    let pins = constraints::pins(
        &p.pins
            .iter()
            .map(|p| (p.name.clone(), p.pads.clone()))
            .collect(),
        8,
    )?;
    let timers = constraints::units(p.timers.iter().map(|t| t.name.as_str()), 4, "timers")?;
    let shifters = constraints::units(p.shifters.iter().map(|s| s.name.as_str()), 2, "shifters")?;
    let mut signals: BTreeMap<String, u16> =
        BTreeMap::from([("zero".into(), 0), ("one".into(), 1)]);
    for (name, &k) in &pins {
        for (suffix, value) in [("rise", 16 + k), ("fall", 24 + k), ("level", 32 + k)] {
            signals.insert(format!("{name}.{suffix}"), value as u16);
        }
    }
    for (name, &k) in &timers {
        for (suffix, value) in [("rise", 4 + 2 * k), ("fall", 5 + 2 * k), ("out", 40 + k)] {
            signals.insert(format!("{name}.{suffix}"), value as u16);
        }
    }
    for (name, &k) in &shifters {
        for (suffix, value) in [
            ("word", 12 + 2 * k),
            ("stop", 13 + 2 * k),
            ("out", 44 + k),
            ("active", 46 + k),
        ] {
            signals.insert(format!("{name}.{suffix}"), value as u16);
        }
    }
    let signal = |name: &str, level: bool| -> Result<u16, Error> {
        let &value = signals.get(name).ok_or_else(|| {
            Error::new("signal", format!("unknown or unsupported signal {name:?}"))
        })?;
        if level && !(32..64).contains(&value) {
            return Err(Error::new(
                "signal-class",
                format!("{name}: selector requires a level line (32..63)"),
            ));
        }
        Ok(if level { value - 32 } else { value })
    };
    let mut config = Vec::new();
    for t in &p.timers {
        let base = (4 * timers[&t.name]) as u8;
        let mut sync = 0;
        if let Some(resync) = &t.resync {
            sync |= signal(resync, true)? | 1 << 5;
        }
        if let Some(hold) = &t.hold {
            sync |= signal(hold, true)? << 6 | 1 << 11 | u16::from(t.hold_invert) << 12;
        }
        config.extend([
            (
                base,
                (t.period_cycles.expect("validated period") - 1) as u16,
            ),
            (base + 1, t.phase_cycles.expect("validated phase")),
            (base + 3, sync),
            (
                base + 2,
                signal(&t.start, false)?
                    | signal(&t.stop, false)? << 6
                    | t.idle << 12
                    | u16::from(t.pulse) << 13,
            ),
        ]);
    }
    for s in &p.shifters {
        let base = 0x10 + (4 * shifters[&s.name]) as u8;
        let frame = (s.bits - 1)
            | s.pre << 4
            | s.pre_value << 6
            | s.post << 7
            | s.post_value << 9
            | s.parity << 10
            | u16::from(s.msb) << 12
            | u16::from(s.lead) << 13
            | u16::from(s.end_on_sample) << 14
            | s.idle << 15;
        let data = s
            .source
            .as_ref()
            .map(|name| signal(name, true))
            .transpose()?
            .unwrap_or(0);
        config.extend([
            (base, s.value.unwrap_or(0)),
            (base + 1, frame),
            (
                base + 2,
                signal(&s.shift, false)? | signal(&s.sample, false)? << 6,
            ),
            (
                base + 3,
                signal(&s.start, false)?
                    | data << 6
                    | u16::from(s.pull) << 11
                    | u16::from(s.push) << 12,
            ),
        ]);
    }
    for pin in &p.pins {
        let mode = match pin.mode.as_str() {
            "push_pull" => 1,
            "open_drain" => 2,
            _ => 0,
        };
        let value = match pin.source.as_deref() {
            None => 0,
            Some("zero" | "one") => {
                u16::from(pin.source.as_deref() == Some("one")) << 4 | 1 << 5 | mode << 6
            }
            Some(source) => signal(source, true)? | mode << 6,
        };
        config.push((0x18 + pins[&pin.name] as u8, value));
    }
    config.push((0x31, 1));
    let (image, data_offset) = encode_image(&config, &p.data)?;
    let report = Report {
        abi: ABI,
        hardware_revision: HARDWARE_REVISION,
        clock_hz: p.clock_hz,
        pins,
        timers,
        shifter_fifo_channels: shifters,
        pin_modes: p
            .pins
            .iter()
            .map(|p| (p.name.clone(), p.mode.clone()))
            .collect(),
        tx_shifters: p
            .shifters
            .iter()
            .filter(|s| s.pull)
            .map(|s| s.name.clone())
            .collect(),
        rx_shifters: p
            .shifters
            .iter()
            .filter(|s| s.push)
            .map(|s| s.name.clone())
            .collect(),
        image_bytes: image.len(),
        capacity_bytes: 1024,
        data_offset,
        config,
    };
    Ok(Compilation { image, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_and_boundary() {
        assert_eq!(instruction(1, 0x1234), [1, 2, 3, 0x14]);
        assert_eq!(instruction(1, 0), [0x10]);
        assert_eq!(address_instruction(4, 1023).unwrap(), [3, 15, 0x4f]);
        assert!(address_instruction(4, 1024).is_err());
        assert!(address_instruction(2, 1024).is_err());
        let (base, _) = encode_image(&[(0x31, 1)], &[]).unwrap();
        assert_eq!(
            encode_image(&[(0x31, 1)], &vec![0; 1024 - base.len()])
                .unwrap()
                .0
                .len(),
            1024
        );
        assert!(encode_image(&[(0x31, 1)], &vec![0; 1025 - base.len()]).is_err());
        assert!(encode_image(&[(0x18, 1)], &[]).is_err());
    }
}
