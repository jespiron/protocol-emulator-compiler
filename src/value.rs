//! Compile-time values. Arithmetic is exact: a duration that is not a whole
//! number of clock cycles is reported, never rounded, unless the program asks
//! for it with `round(...)`.
use crate::diagnostic::{Error, Result, Span};
use crate::syntax::ast::{Expr, ExprKind};
use std::collections::HashMap;
use std::fmt;

/// An exact fraction with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ratio {
    num: i128,
    den: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

impl Ratio {
    pub(crate) fn new(num: i128, den: i128) -> Ratio {
        let sign = if den < 0 { -1 } else { 1 };
        let divisor = gcd(num, den).max(1);
        Ratio {
            num: sign * num / divisor,
            den: sign * den / divisor,
        }
    }

    pub(crate) fn int(n: i128) -> Ratio {
        Ratio { num: n, den: 1 }
    }

    pub(crate) fn whole(self) -> Option<i128> {
        (self.den == 1).then_some(self.num)
    }

    /// The nearest whole number; halves round away from zero.
    fn round(self) -> i128 {
        let twice = 2 * self.num.abs() + self.den;
        self.num.signum() * (twice / (2 * self.den))
    }

    fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    fn add(self, other: Ratio) -> Ratio {
        Ratio::new(
            self.num * other.den + other.num * self.den,
            self.den * other.den,
        )
    }

    fn neg(self) -> Ratio {
        Ratio::new(-self.num, self.den)
    }

    fn mul(self, other: Ratio) -> Ratio {
        Ratio::new(self.num * other.num, self.den * other.den)
    }

    fn div(self, other: Ratio) -> Option<Ratio> {
        (other.num != 0).then(|| Ratio::new(self.num * other.den, self.den * other.num))
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.whole() {
            Some(n) => write!(f, "{n}"),
            None => write!(f, "{:.2}", self.num as f64 / self.den as f64),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Number(Ratio),
    /// A duration, in clock cycles.
    Cycles(Ratio),
    Hertz(Ratio),
    List(Vec<Value>),
    Str(String),
}

impl Value {
    fn describe(&self) -> &'static str {
        match self {
            Value::Number(_) => "a number",
            Value::Cycles(_) => "a duration",
            Value::Hertz(_) => "a frequency",
            Value::List(_) => "a list",
            Value::Str(_) => "a string",
        }
    }
}

/// Constants in scope plus the clock that turns time units into cycles.
#[derive(Default)]
pub(crate) struct Env {
    pub clock_hz: i128,
    pub consts: HashMap<String, Value>,
    /// Where each constant's value is written, so errors can point there.
    pub defs: HashMap<String, Span>,
}

/// A percentage with as many decimals as it takes to show something.
fn percent(x: f64) -> String {
    match x.abs() {
        x if x >= 1.0 => format!("{x:.1}%"),
        x if x >= 0.1 => format!("{x:.2}%"),
        x => format!("{x:.3}%"),
    }
}

/// A frequency the way people write it: `50 MHz`, `115207 Hz`.
fn frequency(hz: f64) -> String {
    match hz {
        hz if hz >= 1e6 && (hz / 1e6).fract() == 0.0 => format!("{} MHz", hz / 1e6),
        hz if hz >= 1e3 && (hz / 1e3).fract() == 0.0 => format!("{} kHz", hz / 1e3),
        hz => format!("{hz:.0} Hz"),
    }
}

impl Env {
    pub(crate) fn eval(&self, expr: &Expr) -> Result<Value> {
        let span = expr.span;
        Ok(match &expr.kind {
            ExprKind::Int(n) => Value::Number(Ratio::int(*n)),
            ExprKind::Quantity(n, unit) => self.quantity(*n, unit),
            ExprKind::Str(s) => Value::Str(s.clone()),
            ExprKind::Name(name) => self
                .consts
                .get(name)
                .cloned()
                .ok_or_else(|| Error::new(span, format!("`{name}` is not a constant")))?,
            ExprKind::List(items) => {
                Value::List(items.iter().map(|e| self.eval(e)).collect::<Result<_>>()?)
            }
            ExprKind::Call(callee, args)
                if matches!(&callee.kind, ExprKind::Name(n) if n == "round") && args.len() == 1 =>
            {
                match self.eval(&args[0].value)? {
                    Value::Number(r) => Value::Number(Ratio::int(r.round())),
                    Value::Cycles(r) => Value::Cycles(Ratio::int(r.round())),
                    other => return Err(mismatch(span, "round", &other)),
                }
            }
            ExprKind::Unary("-", operand) => match self.eval(operand)? {
                Value::Number(r) => Value::Number(r.neg()),
                Value::Cycles(r) => Value::Cycles(r.neg()),
                other => return Err(mismatch(span, "negate", &other)),
            },
            ExprKind::Binary(op, left, right) => {
                binary(span, op, self.eval(left)?, self.eval(right)?, self.clock_hz)?
            }
            _ => return Err(Error::new(span, "expected a constant value here")),
        })
    }

    fn quantity(&self, n: i128, unit: &str) -> Value {
        let seconds = |per_second: i128| Value::Cycles(Ratio::new(n * self.clock_hz, per_second));
        match unit {
            "cycle" | "cycles" => Value::Cycles(Ratio::int(n)),
            "ns" => seconds(1_000_000_000),
            "us" => seconds(1_000_000),
            "ms" => seconds(1_000),
            "s" => seconds(1),
            "Hz" => Value::Hertz(Ratio::int(n)),
            "kHz" => Value::Hertz(Ratio::int(n * 1_000)),
            "MHz" => Value::Hertz(Ratio::int(n * 1_000_000)),
            _ => unreachable!("the parser only accepts known units"),
        }
    }

    /// A whole number of cycles. When it is not one, the error says what
    /// `what` would be (`timer `bit` would tick every {}`), and suggests
    /// rounding where the value is written: at the constant it came from, or
    /// here. A `period` is also shown as the frequency rounding gives.
    pub(crate) fn cycles(&self, expr: &Expr, what: &str, period: bool) -> Result<i128> {
        let r = match self.eval(expr)? {
            Value::Cycles(r) => r,
            other => {
                return Err(Error::new(
                    expr.span,
                    format!(
                        "expected a duration such as `64 cycles` or `2 us`, found {}",
                        other.describe()
                    ),
                ));
            }
        };
        if let Some(n) = r.whole() {
            return Ok(n);
        }
        let nearest = r.round();
        let wanted = r.to_f64();
        let clock = frequency(self.clock_hz as f64);
        let cost = if period {
            let (got, want) = (
                self.clock_hz as f64 / nearest as f64,
                self.clock_hz as f64 / wanted,
            );
            let off = (got / want - 1.0) * 100.0;
            let way = if off > 0.0 { "fast" } else { "slow" };
            format!(
                "{} instead of {} ({} {way})",
                frequency(got),
                frequency(want),
                percent(off)
            )
        } else {
            let off = (nearest as f64 / wanted - 1.0) * 100.0;
            let way = if off > 0.0 { "longer" } else { "shorter" };
            format!("{} {way}", percent(off))
        };
        let error = Error::new(
            expr.span,
            format!(
                "{}, but the chip only counts whole cycles",
                what.replace("{}", &format!("{r} cycles"))
            ),
        )
        .note(format!(
            "{nearest} {} is {cost}",
            if nearest == 1 { "cycle" } else { "cycles" }
        ));
        let help = "round to the nearest whole cycle";
        let def = match &expr.kind {
            ExprKind::Name(name) => self.defs.get(name).copied(),
            _ => None,
        };
        Err(match def {
            Some(def) => error
                .label("needs a whole number of cycles")
                .related(def, format!("this is {r} cycles at {clock}"))
                .help(help, def, "round({})"),
            None => error.label(format!("this is {r} cycles at {clock}")).help(
                help,
                expr.span,
                "round({})",
            ),
        })
    }

    pub(crate) fn integer(&self, expr: &Expr) -> Result<i128> {
        match self.eval(expr)? {
            Value::Number(r) => r.whole().ok_or_else(|| {
                Error::new(expr.span, format!("expected a whole number, found {r}"))
            }),
            other => Err(Error::new(
                expr.span,
                format!("expected a number, found {}", other.describe()),
            )),
        }
    }
}

fn mismatch(span: Span, op: &str, value: &Value) -> Error {
    Error::new(span, format!("cannot {op} {}", value.describe()))
}

fn binary(span: Span, op: &str, left: Value, right: Value, clock_hz: i128) -> Result<Value> {
    use Value::{Cycles, Hertz, Number};
    let divide = |a: Ratio, b: Ratio| a.div(b).ok_or_else(|| Error::new(span, "division by zero"));
    Ok(match (op, left, right) {
        ("+", Number(a), Number(b)) => Number(a.add(b)),
        ("-", Number(a), Number(b)) => Number(a.add(b.neg())),
        ("*", Number(a), Number(b)) => Number(a.mul(b)),
        ("/", Number(a), Number(b)) => Number(divide(a, b)?),
        ("+", Cycles(a), Cycles(b)) => Cycles(a.add(b)),
        ("-", Cycles(a), Cycles(b)) => Cycles(a.add(b.neg())),
        ("*", Cycles(a), Number(b)) | ("*", Number(b), Cycles(a)) => Cycles(a.mul(b)),
        ("/", Cycles(a), Number(b)) => Cycles(divide(a, b)?),
        ("/", Cycles(a), Cycles(b)) => Number(divide(a, b)?),
        ("*", Hertz(a), Number(b)) | ("*", Number(b), Hertz(a)) => Hertz(a.mul(b)),
        // `1 / 115_200 Hz` is one period of that frequency.
        ("/", Number(a), Hertz(b)) => Cycles(divide(a.mul(Ratio::int(clock_hz)), b)?),
        (op @ ("<<" | ">>" | "|" | "&" | "^"), Number(a), Number(b)) => {
            let (Some(a), Some(b)) = (a.whole(), b.whole()) else {
                return Err(Error::new(span, format!("`{op}` needs whole numbers")));
            };
            Number(Ratio::int(match op {
                "<<" => a << b,
                ">>" => a >> b,
                "|" => a | b,
                "&" => a & b,
                _ => a ^ b,
            }))
        }
        (op, left, right) => {
            return Err(Error::new(
                span,
                format!(
                    "cannot apply `{op}` to {} and {}",
                    left.describe(),
                    right.describe()
                ),
            ));
        }
    })
}
