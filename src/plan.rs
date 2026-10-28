//! Turn `test` blocks and `always` invariants into a plan the simulator follows.
//!
//! Peers are opaque here: `attach uart_device(from_chip: tx, bit: BIT)` becomes
//! a peer kind plus arguments, with pins resolved to pads and durations to
//! cycles. The simulator's peer library decides what the arguments mean.
use crate::diagnostic::{Error, Result, Span};
use crate::elaborate::{Design, Direction};
use crate::syntax::ast::{Arg, Expr, ExprKind, Stmt, StmtKind};
use crate::value::Value;
use serde_json::{Map, Value as Json, json};
use std::collections::BTreeMap;

pub(crate) fn plan(design: &Design<'_>, source: &str) -> Result<Json> {
    let line_of = |span: Span| source[..span.start.min(source.len())].matches('\n').count() + 1;
    let mut tests = Vec::new();
    for (name, body) in &design.tests {
        let mut planner = Planner {
            design,
            peers: BTreeMap::new(),
            wires: Vec::new(),
            steps: Vec::new(),
        };
        for stmt in *body {
            planner.stmt(stmt, line_of(stmt.span))?;
        }
        tests.push(json!({
            "name": name.text,
            "line": line_of(name.span),
            "wires": planner.wires,
            "peers": planner.peers.into_values().collect::<Vec<_>>(),
            "steps": planner.steps,
        }));
    }
    let invariants = design
        .invariants
        .iter()
        .map(|(name, condition)| {
            Ok(json!({ "name": name.text, "line": line_of(name.span), "check": invariant(design, condition)? }))
        })
        .collect::<Result<Vec<_>>>()?;
    let streams: Map<String, Json> = design
        .streams
        .iter()
        .map(|(name, s)| {
            let direction = match s.direction {
                Direction::ToChip => "to_chip",
                Direction::FromChip => "from_chip",
            };
            (
                name.clone(),
                json!({ "channel": s.channel, "direction": direction }),
            )
        })
        .collect();
    Ok(json!({
        "clock_hz": design.clock_hz,
        "image": design.image,
        "pins": design.pins,
        "streams": streams,
        "tests": tests,
        "invariants": invariants,
    }))
}

/// An `always` condition as a tree the simulator evaluates every cycle.
fn invariant(design: &Design<'_>, expr: &Expr) -> Result<Json> {
    let pin = |e: &Expr| match &e.kind {
        ExprKind::Name(name) => design.pins.get(name).copied(),
        _ => None,
    };
    let sub = |e: &Expr| invariant(design, e);
    Ok(match &expr.kind {
        ExprKind::Int(b @ (0 | 1)) => json!({ "const": b }),
        ExprKind::Name(_) => match pin(expr) {
            Some(pad) => json!({ "pin": pad }),
            None => {
                return Err(Error::new(
                    expr.span,
                    "invariants read pins; this is not a pin",
                ));
            }
        },
        ExprKind::Field(object, field) => match (pin(object), field.text.as_str()) {
            (Some(pad), edge @ ("changed" | "rise" | "fall")) => {
                json!({ "edge": edge, "pin": pad })
            }
            _ => {
                return Err(Error::new(
                    expr.span,
                    "expected a pin, or a pin's `.changed`, `.rise` or `.fall`",
                ));
            }
        },
        ExprKind::Unary("!", a) => json!({ "op": "not", "a": sub(a)? }),
        ExprKind::Binary(op @ ("&&" | "||" | "==" | "!=" | "implies"), a, b) => {
            json!({ "op": op, "a": sub(a)?, "b": sub(b)? })
        }
        _ => {
            return Err(Error::new(
                expr.span,
                "invariants use pins, 0, 1, !, &&, ||, ==, != and implies",
            ));
        }
    })
}

struct Planner<'d, 'a> {
    design: &'d Design<'a>,
    peers: BTreeMap<String, Json>,
    wires: Vec<[usize; 2]>,
    steps: Vec<Json>,
}

impl Planner<'_, '_> {
    fn pad(&self, expr: &Expr) -> Result<usize> {
        match &expr.kind {
            ExprKind::Name(name) => self.design.pins.get(name).copied(),
            _ => None,
        }
        .ok_or_else(|| Error::new(expr.span, "expected a pin name"))
    }

    fn bytes(&self, expr: &Expr) -> Result<Vec<u8>> {
        let bad = || Error::new(expr.span, "expected bytes: `\"text\"` or `[0x55, 0xA3]`");
        match self.design.env.eval(expr)? {
            Value::Str(s) => Ok(s.into_bytes()),
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Number(n) => {
                        n.whole().and_then(|n| u8::try_from(n).ok()).ok_or_else(bad)
                    }
                    _ => Err(bad()),
                })
                .collect(),
            _ => Err(bad()),
        }
    }

    /// A value given to a peer or an exported function.
    fn argument(&self, expr: &Expr) -> Result<Json> {
        if let ExprKind::Name(name) = &expr.kind
            && let Some(pad) = self.design.pins.get(name)
        {
            return Ok(json!({ "pin": pad }));
        }
        Ok(match self.design.env.eval(expr)? {
            Value::Cycles(_) => json!(self.design.env.cycles(expr)?),
            Value::Number(_) => json!(self.design.env.integer(expr)?),
            Value::Str(_) | Value::List(_) => json!(self.bytes(expr)?),
            Value::Hertz(_) => {
                return Err(Error::new(
                    expr.span,
                    "pass a duration (such as `1 / 9600 Hz`), not a frequency",
                ));
            }
        })
    }

    /// `target.method(...)`, where target is a stream or an attached peer.
    fn endpoint<'e>(&self, expr: &'e Expr, method: &str) -> Result<Option<(Json, &'e [Arg])>> {
        let ExprKind::Call(callee, args) = &expr.kind else {
            return Ok(None);
        };
        let ExprKind::Field(object, field) = &callee.kind else {
            return Ok(None);
        };
        let ExprKind::Name(target) = &object.kind else {
            return Ok(None);
        };
        if field.text != method {
            return Ok(None);
        }
        if let Some(stream) = self.design.streams.get(target) {
            let needed = if method == "write" {
                Direction::ToChip
            } else {
                Direction::FromChip
            };
            if stream.direction != needed {
                return Err(Error::new(
                    object.span,
                    format!("`{target}` carries bytes the other way; it cannot `{method}`"),
                ));
            }
            Ok(Some((json!({ "stream": target }), args)))
        } else if self.peers.contains_key(target) {
            Ok(Some((json!({ "peer": target }), args)))
        } else {
            Err(Error::new(
                object.span,
                format!("`{target}` is not an exported stream or an attached peer"),
            ))
        }
    }

    fn stmt(&mut self, stmt: &Stmt, line: usize) -> Result<()> {
        let mut step = match &stmt.kind {
            StmtKind::Wire { from, to } => {
                self.wires.push([self.pad(from)?, self.pad(to)?]);
                return Ok(());
            }
            StmtKind::Attach { peer, alias } => {
                let wrong = || Error::new(peer.span, "expected a peer such as `uart_device(...)`");
                let ExprKind::Call(kind, args) = &peer.kind else {
                    return Err(wrong());
                };
                let ExprKind::Name(kind) = &kind.kind else {
                    return Err(wrong());
                };
                let mut arguments = Map::new();
                for arg in args {
                    let name = arg.name.as_ref().ok_or_else(|| {
                        Error::new(
                            arg.value.span,
                            "name peer arguments: `uart_device(from_chip: tx, ...)`",
                        )
                    })?;
                    arguments.insert(name.text.clone(), self.argument(&arg.value)?);
                }
                let name = alias.as_ref().map_or(kind.clone(), |a| a.text.clone());
                if self.peers.contains_key(&name) {
                    return Err(Error::new(
                        stmt.span,
                        format!("a peer named `{name}` is already attached; add `as <name>`"),
                    ));
                }
                self.peers.insert(
                    name.clone(),
                    json!({ "name": name, "kind": kind, "args": arguments, "line": line }),
                );
                return Ok(());
            }
            StmtKind::Expr(expr) => {
                if let Some((target, args)) = self.endpoint(expr, "write")? {
                    let [arg] = args else {
                        return Err(Error::new(expr.span, "`write` takes the bytes to send"));
                    };
                    json!({ "op": "write", "target": target, "bytes": self.bytes(&arg.value)? })
                } else {
                    return Err(Error::new(
                        expr.span,
                        "expected `<stream or peer>.write(...)`",
                    ));
                }
            }
            StmtKind::Expect(condition) => {
                let ExprKind::Binary("==", actual, expected) = &condition.kind else {
                    return Err(Error::new(
                        condition.span,
                        "expected `expect <stream or peer>.read(n) == <bytes>`",
                    ));
                };
                let expected = self.bytes(expected)?;
                if let Some((target, args)) = self.endpoint(actual, "read")? {
                    let [count] = args else {
                        return Err(Error::new(
                            actual.span,
                            "`read` takes how many bytes to read",
                        ));
                    };
                    let count = self.design.env.integer(&count.value)?;
                    if count != expected.len() as i128 {
                        return Err(Error::new(
                            condition.span,
                            format!(
                                "this reads {count} bytes but compares against {}",
                                expected.len()
                            ),
                        ));
                    }
                    json!({ "op": "read", "target": target, "expect": expected })
                } else {
                    return Err(Error::new(
                        actual.span,
                        "expected `<stream or peer>.read(n)`",
                    ));
                }
            }
            StmtKind::Wait(duration) => {
                json!({ "op": "wait", "cycles": self.design.env.cycles(duration)? })
            }
        };
        step["line"] = json!(line);
        self.steps.push(step);
        Ok(())
    }
}
