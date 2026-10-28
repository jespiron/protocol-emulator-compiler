//! Typed source schema. All constructors are private to the compiler pipeline.
use crate::CompileError as Error;
use serde::Deserialize;
use std::collections::BTreeSet;

fn clock() -> u64 {
    50_000_000
}
fn one() -> String {
    "one".into()
}
fn zero() -> String {
    "zero".into()
}
fn eight() -> u16 {
    8
}
fn pads() -> Vec<usize> {
    (0..8).collect()
}
fn input() -> String {
    "input".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Program {
    #[serde(default = "clock")]
    pub clock_hz: u64,
    #[serde(default)]
    pub pins: Vec<Pin>,
    #[serde(default)]
    pub timers: Vec<Timer>,
    #[serde(default)]
    pub shifters: Vec<Shifter>,
    #[serde(default)]
    pub data: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pin {
    pub name: String,
    #[serde(default = "input")]
    pub mode: String,
    #[serde(default = "pads")]
    pub pads: Vec<usize>,
    pub source: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Timer {
    pub name: String,
    pub period_cycles: Option<u32>,
    pub period_ns: Option<u64>,
    pub phase_cycles: Option<u16>,
    #[serde(default = "one")]
    pub start: String,
    #[serde(default = "zero")]
    pub stop: String,
    #[serde(default)]
    pub idle: u16,
    #[serde(default)]
    pub pulse: bool,
    pub resync: Option<String>,
    pub hold: Option<String>,
    #[serde(default)]
    pub hold_invert: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Shifter {
    pub name: String,
    #[serde(default = "eight")]
    pub bits: u16,
    #[serde(default)]
    pub pre: u16,
    #[serde(default)]
    pub pre_value: u16,
    #[serde(default)]
    pub post: u16,
    #[serde(default)]
    pub post_value: u16,
    #[serde(default)]
    pub parity: u16,
    #[serde(default)]
    pub msb: bool,
    #[serde(default)]
    pub lead: bool,
    #[serde(default)]
    pub end_on_sample: bool,
    #[serde(default)]
    pub idle: u16,
    #[serde(default = "zero")]
    pub shift: String,
    #[serde(default = "zero")]
    pub sample: String,
    #[serde(default = "zero")]
    pub start: String,
    pub source: Option<String>,
    #[serde(default)]
    pub pull: bool,
    #[serde(default)]
    pub push: bool,
    pub value: Option<u16>,
}

fn range(value: u64, low: u64, high: u64, field: &str) -> Result<(), Error> {
    if !(low..=high).contains(&value) {
        return Err(Error::new(
            "range",
            format!("{field} must be in {low}..{high}; got {value}"),
        ));
    }
    Ok(())
}

pub(crate) fn parse(source: &str) -> Result<Program, Error> {
    let mut p: Program = toml::from_str(source).map_err(|e| Error::new("schema", e.to_string()))?;
    range(p.clock_hz, 1, 1_000_000_000, "clock_hz")?;
    let mut names: BTreeSet<&str> = ["zero", "one", "fire0", "fire1"].into();
    for name in p
        .pins
        .iter()
        .map(|x| &x.name)
        .chain(p.timers.iter().map(|x| &x.name))
        .chain(p.shifters.iter().map(|x| &x.name))
    {
        let mut chars = name.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid || !names.insert(name) {
            return Err(Error::new(
                "name",
                format!("duplicate, reserved, or invalid resource name {name:?}"),
            ));
        }
    }
    for pin in &p.pins {
        if !["input", "push_pull", "open_drain"].contains(&pin.mode.as_str()) {
            return Err(Error::new(
                "pin-mode",
                format!("{}: unsupported mode {}", pin.name, pin.mode),
            ));
        }
        if (pin.mode == "input") != pin.source.is_none() {
            return Err(Error::new(
                "pin-source",
                format!(
                    "{}: input pins cannot drive; driven pins need source",
                    pin.name
                ),
            ));
        }
    }
    for t in &mut p.timers {
        let period = match (t.period_cycles, t.period_ns) {
            (Some(c), None) => u64::from(c),
            (None, Some(ns)) => {
                range(ns, 1, 1_000_000_000_000_000, "period_ns")?;
                let ticks = u128::from(ns) * u128::from(p.clock_hz);
                if ticks % 1_000_000_000 != 0 {
                    return Err(Error::new(
                        "timing",
                        format!(
                            "{}: {ns} ns is not an integer clock interval at {} Hz",
                            t.name, p.clock_hz
                        ),
                    ));
                }
                (ticks / 1_000_000_000) as u64
            }
            _ => {
                return Err(Error::new(
                    "timing",
                    format!(
                        "{}: specify exactly one of period_cycles or period_ns",
                        t.name
                    ),
                ));
            }
        };
        range(period, 1, 65536, &format!("{}.period", t.name))?;
        t.period_cycles = Some(period as u32);
        t.phase_cycles.get_or_insert((period - 1) as u16);
        range(t.idle.into(), 0, 1, "timer.idle")?;
        if t.hold_invert && t.hold.is_none() {
            return Err(Error::new(
                "timer-hold",
                format!("{}: hold_invert requires hold", t.name),
            ));
        }
    }
    for s in &p.shifters {
        range(s.bits.into(), 1, 16, "shifter.bits")?;
        for (field, v, max) in [
            ("pre", s.pre, 3),
            ("post", s.post, 3),
            ("pre_value", s.pre_value, 1),
            ("post_value", s.post_value, 1),
            ("parity", s.parity, 3),
            ("idle", s.idle, 1),
        ] {
            range(v.into(), 0, max, &format!("{}.{field}", s.name))?;
        }
        range(
            u64::from(s.value.unwrap_or(0)),
            0,
            (1 << s.bits) - 1,
            "shifter.value",
        )?;
        if s.bits > 8 && (s.pull || s.push) {
            return Err(Error::new(
                "fifo-width",
                format!(
                    "{}: automatic FIFO paths carry only 8 bits, requested {}",
                    s.name, s.bits
                ),
            ));
        }
        if (s.push || s.sample != "zero") && s.source.is_none() {
            return Err(Error::new(
                "rx-source",
                format!("{}: sampled RX requires an explicit level source", s.name),
            ));
        }
        if s.push && s.sample == "zero" {
            return Err(Error::new(
                "rx-sample",
                format!("{}: automatic RX requires a sample signal", s.name),
            ));
        }
        if s.pull && s.value.is_some() {
            return Err(Error::new(
                "tx-source",
                format!("{}: choose FIFO pull or a register value", s.name),
            ));
        }
    }
    Ok(p)
}
