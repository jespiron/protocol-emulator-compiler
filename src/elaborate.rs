//! Resolve names, assign hardware, and turn declarations into configuration
//! writes. This is where "does it fit on the chip?" is decided.
use crate::diagnostic::{Error, Result, Span};
use crate::hardware::{self, Line, PinMode, config};
use crate::logic::{self, Formula};
use crate::px8::Assembler;
use crate::syntax::ast::{Expr, ExprKind, Field, Item, Name, Stmt};
use crate::value::{Env, Value};
use std::collections::{BTreeMap, HashMap};

const DEFAULT_CLOCK_HZ: i128 = 50_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Pin,
    Timer,
    Shifter,
    Counter,
    Reg,
}

impl Kind {
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Kind::Pin => "pin",
            Kind::Timer => "timer",
            Kind::Shifter => "shifter",
            Kind::Counter => "counter",
            Kind::Reg => "reg",
        }
    }

    /// Signals a declaration of this kind provides, for error hints.
    fn outputs(self) -> &'static str {
        match self {
            Kind::Pin => "`.rise`, `.fall` (pulses) or the pin name itself (its level)",
            Kind::Timer => "`.tick`/`.rise`, `.fall` (pulses) or `.out` (level)",
            Kind::Shifter => "`.word`, `.done` (pulses), `.out` or `.busy` (levels)",
            Kind::Counter => "`.reached` (pulse) or `.at_least` (level)",
            Kind::Reg => "its name",
        }
    }

    fn capacity(self) -> (usize, &'static str) {
        match self {
            Kind::Pin => (hardware::PADS, "pins"),
            Kind::Timer => (hardware::TIMERS, "timers"),
            Kind::Shifter => (hardware::SHIFTERS, "shifters"),
            Kind::Counter => (hardware::COUNTERS, "counters"),
            Kind::Reg => (hardware::LOGIC_CELLS, "logic cells (`reg`s)"),
        }
    }
}

/// A declared name and the hardware index it was given.
#[derive(Clone, Copy)]
pub(crate) struct Decl {
    pub kind: Kind,
    pub index: usize,
}

/// Which way bytes flow through an exported stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Host writes, the chip sends on the wire.
    ToChip,
    /// The chip receives from the wire, the host reads.
    FromChip,
}

#[derive(Clone, Debug)]
pub(crate) struct Stream {
    pub channel: usize,
    pub direction: Direction,
}

/// A shifter's FIFO connections to the host.
#[derive(Clone, Copy, Default)]
pub(crate) struct Fifo {
    /// Takes words from the host (TX FIFO).
    pub pull: bool,
    /// Gives words to the host (RX FIFO).
    pub push: bool,
}

/// The elaborated program.
pub(crate) struct Design<'a> {
    pub clock_hz: i128,
    /// Constants, for evaluating test values.
    pub env: Env,
    pub image: Vec<u8>,
    /// Pin name to physical `uio` pad.
    pub pins: BTreeMap<String, usize>,
    pub streams: BTreeMap<String, Stream>,
    pub usage: Vec<(&'static str, usize, usize)>,
    pub tests: Vec<(&'a Name, &'a [Stmt])>,
}

/// Names and hardware assignments.
pub(crate) struct Chip {
    pub env: Env,
    pub decls: HashMap<String, Decl>,
    pub pads: Vec<usize>,
    pub fifos: Vec<Fifo>,
}

pub(crate) fn elaborate(items: &[Item]) -> Result<Design<'_>> {
    let clock_hz = match items.iter().find_map(|item| match item {
        Item::Clock(expr) => Some(expr),
        _ => None,
    }) {
        Some(expr) => clock(expr)?,
        None => DEFAULT_CLOCK_HZ,
    };
    let mut chip = Chip {
        env: Env {
            clock_hz,
            consts: HashMap::new(),
        },
        decls: HashMap::new(),
        pads: Vec::new(),
        fifos: vec![Fifo::default(); hardware::SHIFTERS],
    };
    let mut names: HashMap<String, Span> = HashMap::new();
    let mut counts = [0usize; 6];
    let mut pins = Vec::new();
    let mut tests = Vec::new();
    let mut declare = |name: &Name, kind: Option<Kind>, chip: &mut Chip| {
        if names.insert(name.text.clone(), name.span).is_some() {
            return Err(Error::new(
                name.span,
                format!("`{}` is already declared", name.text),
            ));
        }
        if let Some(kind) = kind {
            let index = counts[kind as usize];
            counts[kind as usize] += 1;
            let (capacity, plural) = kind.capacity();
            if index >= capacity {
                return Err(Error::new(
                    name.span,
                    format!(
                        "the chip has only {capacity} {plural}; `{}` would be number {}",
                        name.text,
                        index + 1
                    ),
                ));
            }
            chip.decls.insert(name.text.clone(), Decl { kind, index });
        }
        Ok(())
    };

    for item in items {
        match item {
            Item::Clock(_) => {}
            Item::Const { name, value } => {
                declare(name, None, &mut chip)?;
                let value = chip.env.eval(value)?;
                chip.env.consts.insert(name.text.clone(), value);
            }
            Item::Pin { name, pads, .. } => {
                declare(name, Some(Kind::Pin), &mut chip)?;
                let allowed = match pads {
                    None => (0..hardware::PADS).collect(),
                    Some(list) => list
                        .iter()
                        .map(|e| chip.pad(e))
                        .collect::<Result<Vec<_>>>()?,
                };
                pins.push((name, allowed));
            }
            Item::Tile { kind, name, .. } => {
                let kind = match kind.text.as_str() {
                    "timer" => Kind::Timer,
                    "shifter" => Kind::Shifter,
                    _ => Kind::Counter,
                };
                declare(name, Some(kind), &mut chip)?;
            }
            Item::Reg { name, .. } => declare(name, Some(Kind::Reg), &mut chip)?,
            Item::Stream { name, .. } => declare(name, None, &mut chip)?,
            Item::Test { name, body } => tests.push((name, body.as_slice())),
        }
    }
    chip.pads = assign_pads(&pins)?;

    let mut streams = BTreeMap::new();
    for item in items {
        if let Item::Stream { name, from, to } = item {
            streams.insert(name.text.clone(), chip.stream(name, from, to)?);
        }
    }

    let mut asm = Assembler::default();
    for item in items {
        let writes = match item {
            Item::Pin {
                name, mode, source, ..
            } => chip.pin(name, mode, source.as_ref())?,
            Item::Tile { name, fields, .. } => {
                let fields: Vec<&Field> = fields.iter().collect();
                chip.tile(name, &fields)?
            }
            Item::Reg { name, init, value } => chip.reg(name, init.as_ref(), value)?,
            _ => Vec::new(),
        };
        for (address, value) in writes {
            asm.config(address, value);
        }
    }
    asm.config(config::ARM, 1);
    asm.idle_forever();
    let image = asm
        .assemble()
        .map_err(|message| Error::new(Span::default(), message))?;

    let pins = pins
        .iter()
        .map(|(name, _)| (name.text.clone(), chip.pads[chip.decls[&name.text].index]))
        .collect();
    let kinds = [
        Kind::Pin,
        Kind::Timer,
        Kind::Shifter,
        Kind::Counter,
        Kind::Reg,
    ];
    let mut usage: Vec<_> = kinds
        .iter()
        .map(|&kind| {
            let (capacity, plural) = kind.capacity();
            (
                plural.split(' ').next().unwrap_or(plural),
                counts[kind as usize],
                capacity,
            )
        })
        .collect();
    usage.push(("sram", image.len(), hardware::SRAM_BYTES));
    Ok(Design {
        clock_hz,
        env: chip.env,
        image,
        pins,
        streams,
        usage,
        tests,
    })
}

fn clock(expr: &Expr) -> Result<i128> {
    let probe = Env {
        clock_hz: 1,
        consts: HashMap::new(),
    };
    match probe.eval(expr)? {
        Value::Hertz(hz) => hz.whole().filter(|&hz| hz > 0).ok_or_else(|| {
            Error::new(
                expr.span,
                "the clock must be a whole, positive number of Hz",
            )
        }),
        _ => Err(Error::new(
            expr.span,
            "expected a frequency, such as `clock 50 MHz`",
        )),
    }
}

/// Give every pin a distinct pad from its allowed list (bipartite matching
/// by augmenting paths), or explain why no assignment exists.
fn assign_pads(pins: &[(&Name, Vec<usize>)]) -> Result<Vec<usize>> {
    fn augment(
        pin: usize,
        pins: &[(&Name, Vec<usize>)],
        owner: &mut [Option<usize>],
        seen: &mut [bool],
    ) -> bool {
        // Prefer a free pad, so pins keep the lowest pads in declaration order.
        if let Some(&pad) = pins[pin].1.iter().find(|&&pad| owner[pad].is_none()) {
            owner[pad] = Some(pin);
            return true;
        }
        for &pad in &pins[pin].1 {
            if !seen[pad] {
                seen[pad] = true;
                if owner[pad].is_none_or(|other| augment(other, pins, owner, seen)) {
                    owner[pad] = Some(pin);
                    return true;
                }
            }
        }
        false
    }
    let mut owner = vec![None; hardware::PADS];
    for (pin, (name, _)) in pins.iter().enumerate() {
        if !augment(pin, pins, &mut owner, &mut [false; hardware::PADS]) {
            return Err(Error::new(
                name.span,
                format!(
                    "no free pad left for `{}`: the pins before it already take every pad it allows",
                    name.text
                ),
            ));
        }
    }
    let mut pads = vec![0; pins.len()];
    for (pad, pin) in owner.iter().enumerate() {
        if let Some(pin) = pin {
            pads[*pin] = pad;
        }
    }
    Ok(pads)
}

/// Index fields by name, rejecting unknown and repeated ones. Later fields
/// override earlier ones, which is how `.set { ... }` overrides a declaration.
fn fields<'f>(
    list: &[&'f Field],
    allowed: &[&str],
    noun: &str,
) -> Result<HashMap<&'f str, &'f Field>> {
    let mut found = HashMap::new();
    for f in list {
        if !allowed.contains(&f.name.text.as_str()) {
            return Err(Error::new(
                f.name.span,
                format!(
                    "a {noun} has no `{}`; it has: {}",
                    f.name.text,
                    allowed.join(", ")
                ),
            ));
        }
        found.insert(f.name.text.as_str(), *f);
    }
    Ok(found)
}

pub(crate) fn value(f: &Field) -> Result<&Expr> {
    f.value.as_ref().ok_or_else(|| {
        Error::new(
            f.name.span,
            format!("`{}` needs a value: `{}: ...`", f.name.text, f.name.text),
        )
    })
}

impl Chip {
    pub(crate) fn decl(&self, expr: &Expr) -> Result<(String, Decl)> {
        let ExprKind::Name(name) = &expr.kind else {
            return Err(Error::new(expr.span, "expected a name"));
        };
        let decl = *self
            .decls
            .get(name)
            .ok_or_else(|| Error::new(expr.span, format!("`{name}` is not declared")))?;
        Ok((name.clone(), decl))
    }

    /// Resolve a signal reference to its line.
    pub(crate) fn signal(&self, expr: &Expr) -> Result<Line> {
        match &expr.kind {
            ExprKind::Int(0) => Ok(Line::ZERO),
            ExprKind::Int(1) => Ok(Line::ONE),
            ExprKind::Name(_) => {
                let (name, decl) = self.decl(expr)?;
                match decl.kind {
                    Kind::Pin => Ok(Line::pin_level(self.pads[decl.index])),
                    Kind::Reg => Ok(Line::logic(decl.index)),
                    kind => Err(Error::new(
                        expr.span,
                        format!(
                            "`{name}` is a {}; use one of its signals: {}",
                            kind.noun(),
                            kind.outputs()
                        ),
                    )),
                }
            }
            ExprKind::Field(object, field) => {
                let (_, decl) = self.decl(object)?;
                let k = decl.index;
                Ok(match (decl.kind, field.text.as_str()) {
                    (Kind::Pin, "rise") => Line::pin_rise(self.pads[k]),
                    (Kind::Pin, "fall") => Line::pin_fall(self.pads[k]),
                    (Kind::Timer, "tick" | "rise") => Line::timer_rise(k),
                    (Kind::Timer, "fall") => Line::timer_fall(k),
                    (Kind::Timer, "out") => Line::timer_out(k),
                    (Kind::Shifter, "word") => Line::shifter_word(k),
                    (Kind::Shifter, "done") => Line::shifter_stop(k),
                    (Kind::Shifter, "out") => Line::shifter_out(k),
                    (Kind::Shifter, "busy") => Line::shifter_active(k),
                    (Kind::Counter, "reached") => Line::counter_reached(k),
                    (Kind::Counter, "at_least") => Line::counter_at_least(k),
                    (kind, _) => {
                        return Err(Error::new(
                            field.span,
                            format!(
                                "a {} has no signal `{}`; it has {}",
                                kind.noun(),
                                field.text,
                                kind.outputs()
                            ),
                        ));
                    }
                })
            }
            _ => Err(Error::new(expr.span, "expected a signal")),
        }
    }

    fn level(&self, expr: &Expr) -> Result<Line> {
        let line = self.signal(expr)?;
        if !line.is_level() {
            return Err(Error::new(
                expr.span,
                "this needs a level (a value that stays high or low), but this is a one-cycle pulse or a constant",
            ));
        }
        Ok(line)
    }

    fn pulse(&self, expr: &Expr) -> Result<Line> {
        let line = self.signal(expr)?;
        if line.is_level() {
            return Err(Error::new(
                expr.span,
                "this needs a pulse (such as `.tick` or `.rise`), but this is a level",
            ));
        }
        Ok(line)
    }

    pub(crate) fn bit(&self, expr: &Expr) -> Result<u16> {
        match self.env.integer(expr)? {
            b @ (0 | 1) => Ok(b as u16),
            _ => Err(Error::new(expr.span, "expected 0 or 1")),
        }
    }

    fn ranged(expr: &Expr, value: i128, low: i128, high: i128, what: &str) -> Result<u16> {
        if (low..=high).contains(&value) {
            Ok(value as u16)
        } else {
            Err(Error::new(
                expr.span,
                format!("{what} is {value}; it must be between {low} and {high}"),
            ))
        }
    }

    fn pad(&self, expr: &Expr) -> Result<usize> {
        let pad = self.env.integer(expr)?;
        usize::try_from(pad)
            .ok()
            .filter(|&p| p < hardware::PADS)
            .ok_or_else(|| {
                Error::new(
                    expr.span,
                    format!("pads are numbered 0 to {}", hardware::PADS - 1),
                )
            })
    }

    pub(crate) fn pin(
        &self,
        name: &Name,
        mode: &Name,
        source: Option<&Expr>,
    ) -> Result<Vec<(u8, u16)>> {
        let pad = self.pads[self.decls[&name.text].index];
        let mode = match mode.text.as_str() {
            "in" => PinMode::Input,
            "out" => PinMode::PushPull,
            "open_drain" => PinMode::OpenDrain,
            other => {
                return Err(Error::new(
                    mode.span,
                    format!("pin modes are `in`, `out` and `open_drain`, not `{other}`"),
                ));
            }
        };
        let source = match (mode, source) {
            (PinMode::Input, None) => return Ok(Vec::new()),
            (PinMode::Input, Some(source)) => {
                return Err(Error::new(
                    source.span,
                    "an `in` pin only listens; remove `= ...` or make it `out`",
                ));
            }
            (_, None) => {
                return Err(Error::new(
                    name.span,
                    "an output pin needs a source: `= some.signal`",
                ));
            }
            (_, Some(source)) => source,
        };
        if matches!(
            source.kind,
            ExprKind::Unary(..) | ExprKind::Binary(..) | ExprKind::If(..)
        ) {
            return Err(Error::new(
                source.span,
                "a pin copies one signal; compute this in a `reg` (adds one cycle)",
            ));
        }
        let encoded = match self.signal(source)? {
            // Constants have their own encoding on pins.
            line if line.is_constant() => u16::from(line.0) << 4 | 1 << 5,
            line if line.is_level() => line.level_select(),
            _ => {
                return Err(Error::new(
                    source.span,
                    "a pin is driven by a level, but this is a one-cycle pulse; drive it from a `reg` or a `.out`/`.busy` signal",
                ));
            }
        };
        Ok(vec![(config::pin(pad), encoded | (mode as u16) << 6)])
    }

    /// Configuration writes for a timer, shifter or counter from its fields.
    pub(crate) fn tile(&self, name: &Name, list: &[&Field]) -> Result<Vec<(u8, u16)>> {
        let decl = self.decls[&name.text];
        match decl.kind {
            Kind::Timer => self.timer(name, decl.index, list),
            Kind::Shifter => self.shifter(name, decl.index, list),
            Kind::Counter => self.counter(name, decl.index, list),
            kind => Err(Error::new(
                name.span,
                format!("a {} has no settings to change", kind.noun()),
            )),
        }
    }

    fn timer(&self, name: &Name, k: usize, list: &[&Field]) -> Result<Vec<(u8, u16)>> {
        let f = fields(
            list,
            &["every", "first", "pulse", "idle", "run", "hold", "resync"],
            "timer",
        )?;
        let every = f
            .get("every")
            .ok_or_else(|| Error::new(name.span, "a timer needs `every: <duration>`"))?;
        let every = value(every)?;
        let reload = Self::ranged(
            every,
            self.env.cycles(every)? - 1,
            0,
            hardware::MAX_TIMER_CYCLES - 1,
            "`every` minus one cycle",
        )?;
        let phase = match f.get("first") {
            Some(first) => {
                let first = value(first)?;
                Self::ranged(
                    first,
                    self.env.cycles(first)? - 1,
                    0,
                    hardware::MAX_TIMER_CYCLES - 1,
                    "`first` minus one cycle",
                )?
            }
            None => reload,
        };
        if let Some(pulse) = f.get("pulse").filter(|p| p.value.is_some()) {
            return Err(Error::new(
                pulse.name.span,
                "`pulse` is a flag; write just `pulse`",
            ));
        }
        let pulse = u16::from(f.contains_key("pulse"));
        let idle = f
            .get("idle")
            .map(|i| value(i).and_then(|e| self.bit(e)))
            .transpose()?
            .unwrap_or(0);
        let run = f.get("run").ok_or_else(|| {
            Error::new(
                name.span,
                "a timer needs `run: <start> until <stop>`, or `run: 1` to run forever",
            )
        })?;
        let (start, stop) = match &value(run)?.kind {
            ExprKind::Binary("until", start, stop) => (self.signal(start)?, self.signal(stop)?),
            _ => (self.signal(value(run)?)?, Line::ZERO),
        };
        let mut sync = 0;
        if let Some(resync) = f.get("resync") {
            sync |= self.level(value(resync)?)?.level_select() | 1 << 5;
        }
        if let Some(hold) = f.get("hold") {
            let expr = value(hold)?;
            let (line, invert) = match &expr.kind {
                ExprKind::Unary("!", inner) => (self.level(inner)?, 1),
                _ => (self.level(expr)?, 0),
            };
            sync |= line.level_select() << 6 | 1 << 11 | invert << 12;
        }
        Ok(vec![
            (config::timer(k, 0), reload),
            (config::timer(k, 1), phase),
            (config::timer(k, 3), sync),
            (
                config::timer(k, 2),
                u16::from(start.0) | u16::from(stop.0) << 6 | idle << 12 | pulse << 13,
            ),
        ])
    }

    /// `frame: [0, data(8, msb_first), even, 1]`: framing bits, data bits, optional parity, framing bits.
    fn frame(&self, expr: &Expr) -> Result<(u16, i128)> {
        let ExprKind::List(parts) = &expr.kind else {
            return Err(Error::new(
                expr.span,
                "a frame is a list such as `[0, data(8), 1]`",
            ));
        };
        let is_data = |p: &Expr| matches!(&p.kind, ExprKind::Call(f, _) if matches!(&f.kind, ExprKind::Name(n) if n == "data"));
        let data_at = parts
            .iter()
            .position(is_data)
            .ok_or_else(|| Error::new(expr.span, "a frame needs exactly one `data(bits)`"))?;
        let ExprKind::Call(_, args) = &parts[data_at].kind else {
            unreachable!()
        };
        let count = args.first().ok_or_else(|| {
            Error::new(parts[data_at].span, "`data` needs a bit count: `data(8)`")
        })?;
        let bits = self.env.integer(&count.value)?;
        Self::ranged(
            &count.value,
            bits,
            1,
            hardware::MAX_WORD_BITS,
            "the word length",
        )?;
        let mut msb = 0;
        for arg in &args[1..] {
            match &arg.value.kind {
                ExprKind::Name(n) if n == "msb_first" => msb = 1,
                ExprKind::Name(n) if n == "lsb_first" => msb = 0,
                _ => {
                    return Err(Error::new(
                        arg.value.span,
                        "expected `msb_first` or `lsb_first`",
                    ));
                }
            }
        }
        let mut after = &parts[data_at + 1..];
        let mut parity = 0;
        if let Some(Expr {
            kind: ExprKind::Name(n),
            span,
        }) = after.first()
            && !self.env.consts.contains_key(n)
        {
            parity = match n.as_str() {
                "even" => 1,
                "odd" => 2,
                _ => return Err(Error::new(*span, "expected `even`, `odd`, 0 or 1")),
            };
            after = &after[1..];
        }
        let framing = |bits: &[Expr], side: &str| -> Result<(u16, u16)> {
            let values = bits
                .iter()
                .map(|b| self.bit(b))
                .collect::<Result<Vec<_>>>()?;
            if values.len() as i128 > hardware::MAX_FRAME_BITS {
                return Err(Error::new(
                    expr.span,
                    format!(
                        "at most {} framing bits {side} the data",
                        hardware::MAX_FRAME_BITS
                    ),
                ));
            }
            if values.windows(2).any(|w| w[0] != w[1]) {
                return Err(Error::new(
                    expr.span,
                    format!("framing bits {side} the data must all be equal"),
                ));
            }
            Ok((values.len() as u16, values.first().copied().unwrap_or(0)))
        };
        let (pre, pre_value) = framing(&parts[..data_at], "before")?;
        let (post, post_value) = framing(after, "after")?;
        Ok((
            (bits - 1) as u16
                | pre << 4
                | pre_value << 6
                | post << 7
                | post_value << 9
                | parity << 10
                | msb << 12,
            bits,
        ))
    }

    fn shifter(&self, name: &Name, k: usize, list: &[&Field]) -> Result<Vec<(u8, u16)>> {
        let f = fields(
            list,
            &[
                "frame",
                "idle",
                "start",
                "shift",
                "sample",
                "lead",
                "end_on_sample",
            ],
            "shifter",
        )?;
        let (mut frame, bits) = match f.get("frame") {
            Some(frame) => self.frame(value(frame)?)?,
            None => (7, 8),
        };
        let Fifo { pull, push } = self.fifos[k];
        if (pull || push) && bits > hardware::FIFO_WORD_BITS {
            return Err(Error::new(
                name.span,
                format!(
                    "`{}` exchanges words with the host, so its data is at most 8 bits",
                    name.text
                ),
            ));
        }
        for (flag, bit) in [("lead", 13), ("end_on_sample", 14)] {
            if f.contains_key(flag) {
                frame |= 1 << bit;
            }
        }
        if let Some(idle) = f.get("idle") {
            frame |= self.bit(value(idle)?)? << 15;
        }
        let shift = f
            .get("shift")
            .map(|s| self.signal(value(s)?))
            .transpose()?
            .unwrap_or(Line::ZERO);
        let (sample, input) = match f.get("sample") {
            None => (Line::ZERO, None),
            Some(s) => match &value(s)?.kind {
                ExprKind::Binary("at", input, when) => {
                    (self.signal(when)?, Some(self.level(input)?))
                }
                _ => {
                    return Err(Error::new(
                        value(s)?.span,
                        "say which wire to read and when: `sample: rx at bit.tick`",
                    ));
                }
            },
        };
        if push && input.is_none() {
            return Err(Error::new(
                name.span,
                format!(
                    "`{}` sends received words to the host, so it needs `sample: <pin> at <signal>`",
                    name.text
                ),
            ));
        }
        let start = f
            .get("start")
            .map(|s| self.signal(value(s)?))
            .transpose()?
            .unwrap_or(Line::ZERO);
        let input = input.map_or(0, Line::level_select);
        Ok(vec![
            (config::shifter(k, 1), frame),
            (
                config::shifter(k, 2),
                u16::from(shift.0) | u16::from(sample.0) << 6,
            ),
            (
                config::shifter(k, 3),
                u16::from(start.0) | input << 6 | u16::from(pull) << 11 | u16::from(push) << 12,
            ),
        ])
    }

    fn counter(&self, name: &Name, k: usize, list: &[&Field]) -> Result<Vec<(u8, u16)>> {
        let f = fields(list, &["count", "limit", "clear"], "counter")?;
        let step = match f.get("count") {
            Some(c) => self.signal(value(c)?)?,
            None => return Err(Error::new(name.span, "a counter needs `count: <signal>`")),
        };
        let limit = match f.get("limit") {
            Some(l) => {
                let expr = value(l)?;
                Self::ranged(expr, self.env.integer(expr)?, 0, 255, "the limit")?
            }
            None => 0,
        };
        let clear = f
            .get("clear")
            .map(|c| self.pulse(value(c)?))
            .transpose()?
            .unwrap_or(Line::ZERO);
        Ok(vec![
            (config::counter(k, 0), limit),
            (config::counter(k, 2), u16::from(clear.0)),
            (config::counter(k, 1), u16::from(step.0)),
        ])
    }

    fn reg(&self, name: &Name, init: Option<&Expr>, expr: &Expr) -> Result<Vec<(u8, u16)>> {
        let k = self.decls[&name.text].index;
        let formula = Formula::from_expr(expr, &|e| self.signal(e))?;
        let cell = logic::synthesize(&formula, expr.span)?;
        let init = init.map(|i| self.bit(i)).transpose()?.unwrap_or(0);
        let [a, b, c, d] = cell.inputs;
        Ok(vec![
            (config::logic(k, 0), cell.table),
            (config::logic(k, 1), u16::from(a.0) | u16::from(b.0) << 6),
            (
                config::logic(k, 2),
                c.level_select() | d.level_select() << 5,
            ),
            (config::logic(k, 3), u16::from(Line::ONE.0) | init << 5),
        ])
    }

    /// The shifter named by `expr`, or an error.
    pub(crate) fn shifter_index(&self, expr: &Expr) -> Result<usize> {
        match self.decl(expr)? {
            (
                _,
                Decl {
                    kind: Kind::Shifter,
                    index,
                },
            ) => Ok(index),
            (name, decl) => Err(Error::new(
                expr.span,
                format!("`{name}` is a {}, not a shifter", decl.kind.noun()),
            )),
        }
    }

    /// Connect a shifter's FIFO in one direction, once.
    pub(crate) fn claim(&mut self, k: usize, direction: Direction, span: Span) -> Result<()> {
        let fifo = &mut self.fifos[k];
        let taken = match direction {
            Direction::ToChip => std::mem::replace(&mut fifo.pull, true),
            Direction::FromChip => std::mem::replace(&mut fifo.push, true),
        };
        if taken {
            return Err(Error::new(
                span,
                "this shifter already exchanges bytes with the host in this direction",
            ));
        }
        Ok(())
    }

    fn stream(&mut self, name: &Name, from: &Expr, to: &Expr) -> Result<Stream> {
        let is_host = |e: &Expr| matches!(&e.kind, ExprKind::Name(n) if n == "host");
        let (shifter, direction) = match (is_host(from), is_host(to)) {
            (true, false) => (to, Direction::ToChip),
            (false, true) => (from, Direction::FromChip),
            _ => {
                return Err(Error::new(
                    name.span,
                    "a stream connects the host and a shifter: `host -> sender` or `receiver -> host`",
                ));
            }
        };
        let channel = self.shifter_index(shifter)?;
        self.claim(channel, direction, name.span)?;
        Ok(Stream { channel, direction })
    }
}
