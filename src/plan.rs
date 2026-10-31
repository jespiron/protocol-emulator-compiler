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
    for (name, body, step_timeout) in &design.tests {
        let mut planner = Planner {
            design,
            peers: BTreeMap::new(),
            wires: Vec::new(),
            steps: Vec::new(),
        };
        for stmt in *body {
            planner.stmt(stmt, line_of(stmt.span))?;
        }
        let timeout = step_timeout
            .map(|expr| {
                let cycles = design.env.cycles(expr, "step_timeout", false)?;
                if !(1..=40_000_000).contains(&cycles) {
                    return Err(Error::new(
                        expr.span,
                        "step_timeout must be 1..40000000 clock cycles",
                    ));
                }
                Ok(cycles)
            })
            .transpose()?;
        tests.push(json!({
            "step_timeout_cycles": timeout,
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
                json!({ "channel": s.channel, "direction": direction, "packets": s.packets }),
            )
        })
        .collect();
    Ok(json!({
        "clock_hz": design.clock_hz,
        "image": design.image,
        "pins": design.pins,
        "streams": streams,
        "calls": {
            "channel": design.exports.channel,
            "functions": design.exports.functions,
            "fails": design.exports.fails,
        },
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
    let not_a_pin = |label: String| {
        Error::new(expr.span, "can only write an `always` rule about a pin").label(label)
    };
    Ok(match &expr.kind {
        ExprKind::Int(b @ (0 | 1)) => json!({ "const": b }),
        ExprKind::Name(name) => match pin(expr) {
            Some(pad) => json!({ "pin": pad }),
            None => return Err(not_a_pin(format!("`{name}` is not a pin"))),
        },
        ExprKind::Field(object, field) => match (pin(object), field.text.as_str()) {
            (Some(pad), edge @ ("changed" | "rise" | "fall")) => {
                json!({ "edge": edge, "pin": pad })
            }
            (Some(_), _) => {
                return Err(not_a_pin(
                    "a pin only has `.changed`, `.rise` and `.fall`".to_owned(),
                ));
            }
            (None, field) => {
                let name = match &object.kind {
                    ExprKind::Name(name) => format!("`{name}.{field}`"),
                    _ => "this".to_owned(),
                };
                return Err(not_a_pin(format!("{name} is not a pin")));
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
            Value::Cycles(_) => json!(self.design.env.cycles(expr, "this would be {}", false)?),
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
            let needed = if matches!(method, "write" | "write_packet") {
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
            if stream.packets != method.ends_with("_packet") {
                return Err(Error::new(
                    expr.span,
                    if stream.packets {
                        "packet streams use `write_packet(bytes)` and `read_packet()`"
                    } else {
                        "byte streams use `write(bytes)` and `read(n)`; declare `export packets` for packet I/O"
                    },
                ));
            }
            Ok(Some((json!({ "stream": target }), args)))
        } else if self.peers.contains_key(target) {
            if method.ends_with("_packet") {
                return Err(Error::new(
                    expr.span,
                    "packet methods require an exported packet stream",
                ));
            }
            Ok(Some((json!({ "peer": target }), args)))
        } else {
            Err(Error::new(
                object.span,
                format!("`{target}` is not an exported stream or an attached peer"),
            ))
        }
    }

    fn packet_step(&self, expr: &Expr) -> Result<Option<Json>> {
        if let Some((target, args)) = self.endpoint(expr, "write_packet")? {
            let [arg] = args else {
                return Err(Error::new(expr.span, "`write_packet` takes one byte list"));
            };
            if arg.name.is_some() {
                return Err(Error::new(expr.span, "pass the byte list directly"));
            }
            let bytes = self.bytes(&arg.value)?;
            if bytes.is_empty() {
                return Err(Error::new(
                    expr.span,
                    "an empty packet has no byte to carry the end tag",
                ));
            }
            return Ok(Some(
                json!({"op": "write_packet", "target": target, "bytes": bytes}),
            ));
        }
        if let Some((target, args)) = self.endpoint(expr, "read_packet")? {
            if !args.is_empty() {
                return Err(Error::new(expr.span, "`read_packet()` takes no arguments"));
            }
            return Ok(Some(json!({"op": "read_packet", "target": target})));
        }
        Ok(None)
    }

    fn packet_expected(&self, expr: &Expr) -> Result<Json> {
        let ExprKind::Call(callee, args) = &expr.kind else {
            return Err(Error::new(
                expr.span,
                "compare against `packet([bytes], error: 0)`",
            ));
        };
        if !matches!(&callee.kind, ExprKind::Name(n) if n == "packet")
            || !(1..=2).contains(&args.len())
            || args[0].name.is_some()
        {
            return Err(Error::new(
                expr.span,
                "expected `packet([bytes], error: 0)`",
            ));
        }
        let data = self.bytes(&args[0].value)?;
        if data.is_empty() {
            return Err(Error::new(
                expr.span,
                "a received packet needs at least one byte",
            ));
        }
        let error = if let Some(arg) = args.get(1) {
            if !arg.name.as_ref().is_some_and(|n| n.text == "error") {
                return Err(Error::new(
                    arg.value.span,
                    "the second packet argument is `error: 0` or `error: 1`",
                ));
            }
            let n = self.design.env.integer(&arg.value)?;
            if !(0..=1).contains(&n) {
                return Err(Error::new(arg.value.span, "packet error must be 0 or 1"));
            }
            n == 1
        } else {
            false
        };
        Ok(json!({"data": data, "error": error}))
    }

    /// A call to an exported function: its name and arguments by parameter name.
    fn call(&self, expr: &Expr) -> Result<Option<Json>> {
        let ExprKind::Call(callee, args) = &expr.kind else {
            return Ok(None);
        };
        let ExprKind::Name(function) = &callee.kind else {
            return Ok(None);
        };
        let Some(spec) = self.design.exports.functions.get(function) else {
            return Err(Error::new(
                callee.span,
                format!("`{function}` is not an exported function"),
            ));
        };
        let params = spec["params"].as_array().expect("params are a list");
        let mut values = Map::new();
        for (i, arg) in args.iter().enumerate() {
            let param = match &arg.name {
                Some(name) => params.iter().find(|p| p["name"] == name.text.as_str()),
                None => params.get(i),
            }
            .ok_or_else(|| {
                Error::new(
                    arg.value.span,
                    format!("`{function}` has no parameter here"),
                )
            })?;
            let value = self.argument(&arg.value)?;
            let fits = match param["type"].as_str() {
                Some("bytes") => value.is_array(),
                _ => value.as_i64().is_some_and(|v| {
                    (param["min"].as_i64().unwrap_or(0)..=param["max"].as_i64().unwrap_or(0))
                        .contains(&v)
                }),
            };
            if !fits {
                return Err(Error::new(
                    arg.value.span,
                    match param["type"].as_str() {
                        Some("bytes") => {
                            format!("`{}` takes bytes", param["name"].as_str().unwrap_or(""))
                        }
                        _ => format!(
                            "`{}` takes a number from {} to {}",
                            param["name"].as_str().unwrap_or(""),
                            param["min"],
                            param["max"]
                        ),
                    },
                ));
            }
            values.insert(param["name"].as_str().unwrap_or("").into(), value);
        }
        if let Some(missing) = params
            .iter()
            .find(|p| !values.contains_key(p["name"].as_str().unwrap_or("")))
        {
            return Err(Error::new(
                expr.span,
                format!(
                    "`{function}` needs `{}`",
                    missing["name"].as_str().unwrap_or("")
                ),
            ));
        }
        Ok(Some(
            json!({ "op": "call", "function": function, "args": values }),
        ))
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
                if let Some(step) = self.packet_step(expr)? {
                    step
                } else if let Some((target, args)) = self.endpoint(expr, "write")? {
                    let [arg] = args else {
                        return Err(Error::new(expr.span, "`write` takes the bytes to send"));
                    };
                    json!({ "op": "write", "target": target, "bytes": self.bytes(&arg.value)? })
                } else if let Some((target, args)) = self.endpoint(expr, "read")? {
                    // A read without `expect` just shows what arrives (in the console).
                    let [count] = args else {
                        return Err(Error::new(expr.span, "`read` takes how many bytes to read"));
                    };
                    let count = self.design.env.integer(&count.value)?;
                    json!({ "op": "read", "target": target, "count": count })
                } else if let Some(call) = self.call(expr)? {
                    call
                } else {
                    return Err(Error::new(
                        expr.span,
                        "expected `<stream or peer>.write(...)`, `.read(n)`, or a call to an exported function",
                    ));
                }
            }
            StmtKind::Expect {
                condition,
                fails: Some(message),
            } => {
                let mut call = self.call(condition)?.ok_or_else(|| {
                    Error::new(
                        condition.span,
                        "`fails` follows a call to an exported function",
                    )
                })?;
                if !self.design.exports.fails.contains(&message.text) {
                    return Err(Error::new(
                        message.span,
                        format!(
                            "no function in this program fails with \"{}\"",
                            message.text
                        ),
                    ));
                }
                call["fails"] = json!(message.text);
                call
            }
            StmtKind::Expect {
                condition,
                fails: None,
            } => {
                let ExprKind::Binary("==", actual, expected) = &condition.kind else {
                    return Err(Error::new(
                        condition.span,
                        "expected `expect <read or call> == <bytes>`",
                    ));
                };
                if let Some(mut step) = self.packet_step(actual)? {
                    if step["op"] != "read_packet" {
                        return Err(Error::new(
                            actual.span,
                            "only a packet read returns a packet",
                        ));
                    }
                    step["expect"] = self.packet_expected(expected)?;
                    step["line"] = json!(line);
                    self.steps.push(step);
                    return Ok(());
                }
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
                    json!({ "op": "read", "target": target, "count": count, "expect": expected })
                } else if let Some(mut call) = self.call(actual)? {
                    call["expect"] = json!(expected);
                    call
                } else {
                    return Err(Error::new(
                        actual.span,
                        "expected `<stream or peer>.read(n)` or a call to an exported function",
                    ));
                }
            }
            StmtKind::Wait {
                signal: duration,
                next: false,
            } => {
                let cycles = self
                    .design
                    .env
                    .cycles(duration, "`wait` would last {}", false)?;
                json!({ "op": "wait", "cycles": cycles })
            }
            _ => {
                return Err(Error::new(
                    stmt.span,
                    "this statement belongs in a function, not a test",
                ));
            }
        };
        step["line"] = json!(line);
        self.steps.push(step);
        Ok(())
    }
}
