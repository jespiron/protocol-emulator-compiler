//! Encode the two fixed byte stages between each FIFO and shifter.
use crate::diagnostic::{Error, Result};
use crate::elaborate::{Chip, Direction, fields, value};
use crate::hardware::{self, Line, config};
use crate::syntax::ast::{ByteStage, ExprKind};

#[derive(Default)]
pub(crate) struct Pipeline {
    pub writes: Vec<(u8, u16)>,
    pub frames: bool,
    pub consumes_eop: bool,
}

pub(crate) fn encode(
    chip: &Chip<'_>,
    stages: &[ByteStage],
    channel: usize,
    direction: Direction,
    mut boundaries: bool,
) -> Result<Pipeline> {
    if stages.len() > hardware::BYTE_STAGES {
        return Err(Error::new(
            stages[2].kind.span,
            "each direction of a shifter has only two byte stages",
        ));
    }
    let mut result = Pipeline::default();
    for (stage, item) in stages.iter().enumerate() {
        let kind = item.kind.text.as_str();
        let allowed: &[&str] = match kind {
            "pass" => &[],
            "escape" => &[
                "delimiter",
                "escape",
                "mapped_delimiter",
                "mapped_escape",
                "prepend",
                "append",
            ],
            "unescape" => &["delimiter", "escape", "mapped_delimiter", "mapped_escape"],
            "checksum" => &["algorithm", "operation"],
            "tag" => &["length", "end", "error"],
            "filter" => &["value", "mask"],
            _ => {
                return Err(Error::new(
                    item.kind.span,
                    "expected a byte stage: pass, escape, unescape, checksum, tag or filter",
                ));
            }
        };
        let mut seen = std::collections::HashSet::new();
        for field in &item.fields {
            if !seen.insert(&field.name.text) {
                return Err(Error::new(
                    field.name.span,
                    "a byte-stage field cannot appear more than once",
                ));
            }
        }
        let list: Vec<_> = item.fields.iter().collect();
        let f = fields(&list, allowed, kind)?;
        let number = |name: &str, default: Option<u16>| -> Result<u16> {
            let Some(field) = f.get(name) else {
                return default
                    .ok_or_else(|| Error::new(item.kind.span, format!("`{kind}` needs `{name}`")));
            };
            let expr = value(field)?;
            let n = chip.env.integer(expr)?;
            if !(0..=255).contains(&n) {
                return Err(Error::new(
                    expr.span,
                    format!("`{name}` must fit in one byte (0..255)"),
                ));
            }
            Ok(n as u16)
        };
        let switch = |name: &str, default: u16| -> Result<u16> {
            match f.get(name) {
                None => Ok(default),
                Some(field) => field.value.as_ref().map(|e| chip.bit(e)).unwrap_or(Ok(1)),
            }
        };
        let choice = |name: &str, options: &[&str]| -> Result<usize> {
            let field = f
                .get(name)
                .ok_or_else(|| Error::new(item.kind.span, format!("`{kind}` needs `{name}`")))?;
            let expr = value(field)?;
            if let ExprKind::Name(n) = &expr.kind
                && let Some(index) = options.iter().position(|option| *option == n)
            {
                return Ok(index);
            }
            Err(Error::new(
                expr.span,
                format!("`{name}` must be {}", options.join(" or ")),
            ))
        };
        let line = |name: &str| -> Result<Line> {
            f.get(name)
                .map(|field| chip.signal(value(field)?))
                .transpose()
                .map(|v| v.unwrap_or(Line::ZERO))
        };
        let (mode, a, b, c) = match kind {
            "pass" => (0, 0, 0, 0),
            "escape" | "unescape" => {
                let delimiter = number("delimiter", None)?;
                let escape = number("escape", None)?;
                let mapped_d = number("mapped_delimiter", None)?;
                let mapped_e = number("mapped_escape", None)?;
                if delimiter == escape
                    || mapped_d == mapped_e
                    || mapped_d == delimiter
                    || mapped_e == delimiter
                {
                    return Err(Error::new(
                        item.kind.span,
                        "escape/delimiter codes must be distinct and replacements must be distinct non-delimiter bytes",
                    ));
                }
                let mode = if kind == "escape" {
                    1 | switch("append", 1)? << 3 | switch("prepend", 1)? << 4
                } else {
                    result.frames = true;
                    2
                };
                (
                    mode,
                    delimiter | escape << 8,
                    mapped_d | mapped_e << 8,
                    escape | delimiter << 8,
                )
            }
            "checksum" => (
                4 | (choice("algorithm", &["add", "xor"])? as u16) << 5
                    | (choice("operation", &["generate", "verify"])? as u16) << 6,
                0,
                0,
                0,
            ),
            "tag" => {
                let length = number("length", Some(0))?;
                let end = line("end")?;
                let error = line("error")?;
                if length != 0 || end.0 != 0 {
                    result.frames = true;
                }
                (3, length, u16::from(end.0) | u16::from(error.0) << 6, 0)
            }
            "filter" => (
                5,
                number("value", None)? | number("mask", Some(255))? << 8,
                0,
                0,
            ),
            _ => unreachable!(),
        };
        if matches!(kind, "escape" | "checksum" | "filter") && !boundaries {
            return Err(Error::new(
                item.kind.span,
                format!(
                    "`{kind}` needs upstream packet boundaries; use a packet TX stream, a burst receiver, or put tag/unescape first"
                ),
            ));
        }
        if kind == "unescape" || (kind == "tag" && (a != 0 || b & 0x3f != 0)) {
            boundaries = true;
        }
        result.consumes_eop |= kind != "pass";
        let tile = 4 * channel + 2 * usize::from(direction == Direction::FromChip) + stage;
        let base = config::byte_tile(tile, 0);
        result
            .writes
            .extend([(base + 1, a), (base + 2, b), (base + 3, c), (base, mode)]);
    }
    Ok(result)
}
