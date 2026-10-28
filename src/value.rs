//! Compile-time values. Arithmetic is exact: a duration that is not a whole
//! number of clock cycles is reported, never rounded.
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
pub(crate) struct Env {
    pub clock_hz: i128,
    pub consts: HashMap<String, Value>,
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

    /// A whole number of cycles, with a correction hint when it is not.
    pub(crate) fn cycles(&self, expr: &Expr) -> Result<i128> {
        match self.eval(expr)? {
            Value::Cycles(r) => r.whole().ok_or_else(|| {
                let nearest = ((r.num as f64) / (r.den as f64)).round().max(1.0);
                let rate = self.clock_hz as f64 / nearest;
                let off =
                    (rate * (r.num as f64) / (r.den as f64) / self.clock_hz as f64 - 1.0) * 100.0;
                Error::new(
                    expr.span,
                    format!(
                        "this is {r} cycles at {} Hz, but the chip only counts whole cycles. \
                         The nearest is `{nearest} cycles` ({rate:.0} per second, {off:+.3}%)",
                        self.clock_hz
                    ),
                )
            }),
            other => Err(Error::new(
                expr.span,
                format!(
                    "expected a duration such as `64 cycles` or `2 us`, found {}",
                    other.describe()
                ),
            )),
        }
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
