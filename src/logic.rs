//! Turn a `reg` expression into a logic cell: four input selectors and a
//! 16-entry truth table (`src/fabric_logic.v`).
//!
//! Inputs 0 and 1 accept any line; inputs 2 and 3 accept only level lines.
//! Pulses therefore go first, and at most two pulses fit in one cell.
use crate::diagnostic::{Error, Result, Span};
use crate::hardware::Line;
use crate::syntax::ast::{Expr, ExprKind};

/// A boolean formula over signal lines, with constants folded in.
pub(crate) enum Formula {
    Constant(bool),
    Signal(Line),
    Not(Box<Formula>),
    And(Box<Formula>, Box<Formula>),
    Or(Box<Formula>, Box<Formula>),
    Xor(Box<Formula>, Box<Formula>),
    If(Box<Formula>, Box<Formula>, Box<Formula>),
}

impl Formula {
    /// Build from source, resolving each signal reference with `signal`.
    pub(crate) fn from_expr(expr: &Expr, signal: &dyn Fn(&Expr) -> Result<Line>) -> Result<Self> {
        let sub = |e: &Expr| Formula::from_expr(e, signal).map(Box::new);
        Ok(match &expr.kind {
            ExprKind::Unary("!", operand) => Formula::Not(sub(operand)?),
            ExprKind::Binary(op, l, r) => match *op {
                "&&" => Formula::And(sub(l)?, sub(r)?),
                "||" => Formula::Or(sub(l)?, sub(r)?),
                "^" | "!=" => Formula::Xor(sub(l)?, sub(r)?),
                "==" => Formula::Not(Box::new(Formula::Xor(sub(l)?, sub(r)?))),
                "implies" => Formula::Or(Box::new(Formula::Not(sub(l)?)), sub(r)?),
                _ => {
                    return Err(Error::new(
                        expr.span,
                        format!("`{op}` is not a logic operator; use !, &&, ||, ^, ==, != or if"),
                    ));
                }
            },
            ExprKind::If(c, t, e) => Formula::If(sub(c)?, sub(t)?, sub(e)?),
            _ => match signal(expr)? {
                Line::ZERO => Formula::Constant(false),
                Line::ONE => Formula::Constant(true),
                line => Formula::Signal(line),
            },
        })
    }

    fn signals(&self, out: &mut Vec<Line>) {
        match self {
            Formula::Constant(_) => {}
            Formula::Signal(line) => {
                if !out.contains(line) {
                    out.push(*line);
                }
            }
            Formula::Not(a) => a.signals(out),
            Formula::And(a, b) | Formula::Or(a, b) | Formula::Xor(a, b) => {
                a.signals(out);
                b.signals(out);
            }
            Formula::If(c, t, e) => {
                c.signals(out);
                t.signals(out);
                e.signals(out);
            }
        }
    }

    fn eval(&self, value: &dyn Fn(Line) -> bool) -> bool {
        match self {
            Formula::Constant(b) => *b,
            Formula::Signal(line) => value(*line),
            Formula::Not(a) => !a.eval(value),
            Formula::And(a, b) => a.eval(value) && b.eval(value),
            Formula::Or(a, b) => a.eval(value) || b.eval(value),
            Formula::Xor(a, b) => a.eval(value) != b.eval(value),
            Formula::If(c, t, e) => {
                if c.eval(value) {
                    t.eval(value)
                } else {
                    e.eval(value)
                }
            }
        }
    }
}

/// A configured logic cell.
pub(crate) struct Cell {
    /// Truth table indexed by `{in3, in2, in1, in0}`.
    pub table: u16,
    /// Input selectors; unused slots read the constant-0 line (or the first
    /// level line, which the table ignores).
    pub inputs: [Line; 4],
}

pub(crate) fn synthesize(formula: &Formula, span: Span) -> Result<Cell> {
    let mut signals = Vec::new();
    formula.signals(&mut signals);
    let (pulses, levels): (Vec<Line>, Vec<Line>) = signals.iter().partition(|l| !l.is_level());
    if signals.len() > 4 || pulses.len() > 2 {
        return Err(Error::new(
            span,
            format!(
                "a logic cell reads at most 4 signals, at most 2 of them pulses; this reads {} \
                 ({} pulses). Split it into two `reg`s (each adds one cycle of delay)",
                signals.len(),
                pulses.len()
            ),
        ));
    }
    // Slots 0 and 1 take any line, so pulses first and then levels always fits.
    let mut slots: [Option<Line>; 4] = [None; 4];
    for (slot, line) in pulses.iter().chain(&levels).enumerate() {
        slots[slot] = Some(*line);
    }
    let mut table = 0u16;
    for index in 0..16 {
        let value = |line: Line| {
            let slot = slots
                .iter()
                .position(|s| *s == Some(line))
                .expect("every signal has a slot");
            (index >> slot) & 1 == 1
        };
        if formula.eval(&value) {
            table |= 1 << index;
        }
    }
    let inputs = std::array::from_fn(|slot| {
        slots[slot].unwrap_or(if slot < 2 { Line::ZERO } else { Line(32) })
    });
    Ok(Cell { table, inputs })
}
