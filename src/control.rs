//! Lower functions to PX8 controller code, and describe exported functions
//! so the host knows what to send.
//!
//! # How a call travels
//!
//! Exported functions share one FIFO channel that no stream uses. To call
//! one, the host sends its id, then one byte for every parameter-derived
//! value the body reads, in program order, then a terminator byte tagged
//! end-of-packet. Expressions over parameters only (`address << 1`,
//! `count - 1`) are computed by the host before sending, because the
//! controller has no shift or add. The chip answers with each `reply` byte,
//! then a status byte tagged end-of-packet: 0 for success, or a `fail` code
//! tagged error. After a `fail` the chip discards the call's remaining bytes
//! up to the terminator, so the next call starts cleanly.
//!
//! Events that happened before a call are forgotten; waits inside the call
//! see only events after it was requested.
//!
//! Function calls are inlined; their arguments are constants, parameter
//! expressions, or byte-list parameters. The controller has one loop counter,
//! so runtime loops (`repeat` over a parameter, `for` over bytes) cannot nest.
use crate::diagnostic::{Error, Result};
use crate::elaborate::{Chip, Direction, Kind};
use crate::hardware::{SHIFTERS, config};
use crate::px8::{Assembler, Label};
use crate::syntax::ast::{Arg, Expr, ExprKind, Field, Item, Name, Param, Stmt, StmtKind};
use crate::value::{Env, Ratio, Value};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, HashMap};

/// End-of-packet tag bit in a pulled byte (`A[8]`).
const EOP_TAG: u16 = 0x100;
const UNKNOWN_FUNCTION: u16 = 0xFF;

/// A value the host computes from the call's arguments before sending it.
#[derive(Clone, Debug)]
enum HostExpr {
    Int(i128),
    Param(String),
    Len(String),
    /// The current element of a `for` loop over a byte-list parameter.
    Element(String),
    Op(&'static str, Box<HostExpr>, Box<HostExpr>),
}

impl HostExpr {
    fn to_json(&self) -> Json {
        match self {
            HostExpr::Int(n) => json!(n),
            HostExpr::Param(p) => json!({ "param": p }),
            HostExpr::Len(p) => json!({ "len": p }),
            HostExpr::Element(v) => json!({ "element": v }),
            HostExpr::Op(op, a, b) => json!({ "op": op, "a": a.to_json(), "b": b.to_json() }),
        }
    }
}

/// What the host sends, mirroring the body's runtime loops.
enum Send {
    Value(HostExpr),
    For {
        var: String,
        list: String,
        body: Vec<Send>,
    },
    Repeat {
        count: HostExpr,
        body: Vec<Send>,
    },
}

fn sends_json(sends: &[Send]) -> Json {
    Json::Array(
        sends
            .iter()
            .map(|s| match s {
                Send::Value(v) => json!({ "value": v.to_json() }),
                Send::For { var, list, body } => {
                    json!({ "for": var, "in": list, "body": sends_json(body) })
                }
                Send::Repeat { count, body } => {
                    json!({ "repeat": count.to_json(), "body": sends_json(body) })
                }
            })
            .collect(),
    )
}

/// The host's description of the exported functions.
#[derive(Default)]
pub(crate) struct Exports {
    /// FIFO channel carrying calls and replies.
    pub channel: Option<usize>,
    pub functions: BTreeMap<String, Json>,
    /// `fail` messages; a failure's code is its index plus one.
    pub fails: Vec<String>,
}

#[derive(Clone)]
enum Binding {
    Value(Value),
    Host(HostExpr),
    /// A byte-list parameter of the exported function being lowered.
    Bytes(String),
}

/// A value the controller can load into `A`.
enum Operand {
    Const(i128),
    Host(HostExpr),
    /// A status register read (`RD address`), optionally masked.
    Chip {
        address: u16,
        mask: Option<u16>,
    },
}

fn function_parts(
    item: &Item,
) -> (
    &bool,
    &Name,
    &[Param],
    &Option<crate::syntax::ast::Type>,
    &[Stmt],
) {
    match item {
        Item::Fn {
            exported,
            name,
            params,
            returns,
            body,
        } => (exported, name, params, returns, body),
        _ => unreachable!("only functions are lowered"),
    }
}

/// Connect the FIFOs that exported functions stream through (`via`).
pub(crate) fn claim_fifos(chip: &mut Chip<'_>, functions: &[&Item]) -> Result<()> {
    for item in functions {
        let (exported, name, params, returns, _) = function_parts(item);
        let vias = params
            .iter()
            .filter_map(|p| p.ty.via.as_ref().map(|v| (v, Direction::ToChip)))
            .chain(
                returns
                    .iter()
                    .filter_map(|r| r.via.as_ref().map(|v| (v, Direction::FromChip))),
            );
        for (via, direction) in vias {
            if !exported {
                return Err(Error::new(
                    via.span,
                    format!(
                        "only exported functions exchange bytes with the host; `{}` is private",
                        name.text
                    ),
                ));
            }
            let shifter = Expr {
                kind: ExprKind::Name(via.text.clone()),
                span: via.span,
            };
            let k = chip.shifter_index(&shifter)?;
            chip.claim(k, direction, via.span)?;
        }
    }
    Ok(())
}

/// Emit the controller program after the fabric configuration: an idle loop,
/// or a dispatcher serving the exported functions.
pub(crate) fn lower(chip: &Chip<'_>, functions: &[&Item], asm: &mut Assembler) -> Result<Exports> {
    let exported: Vec<&Item> = functions
        .iter()
        .copied()
        .filter(|f| *function_parts(f).0)
        .collect();
    if exported.is_empty() {
        asm.idle_forever();
        return Ok(Exports::default());
    }
    let channel = (0..SHIFTERS)
        .find(|&k| !chip.fifos[k].pull && !chip.fifos[k].push)
        .ok_or_else(|| {
            Error::new(
                function_parts(exported[0]).1.span,
                "exported functions need a FIFO channel that no stream uses, and every channel is taken",
            )
        })?;
    let dispatch = asm.label();
    let drain = asm.label();
    let mut lowering = Lowering {
        chip,
        asm,
        functions: functions
            .iter()
            .map(|f| (function_parts(f).1.text.clone(), *f))
            .collect(),
        channel,
        fails: Vec::new(),
        sends: Vec::new(),
        in_loop: false,
        inlining: Vec::new(),
        dispatch,
        drain,
    };
    let entries: Vec<Label> = exported.iter().map(|_| lowering.asm.label()).collect();

    // Dispatch on the function id. XRC accumulates, so A ^ id_i is formed incrementally.
    // Forget events while idle, before the next call can start anything, so a
    // call sees exactly the events that happen after it was requested.
    lowering.asm.place(lowering.dispatch);
    lowering.asm.clear_events();
    lowering.asm.pull(channel);
    let mut previous = 0u16;
    for (id, entry) in entries.iter().enumerate() {
        lowering.asm.xor(previous ^ id as u16);
        lowering.asm.jump_if_zero(*entry);
        previous = id as u16;
    }
    lowering.asm.load(UNKNOWN_FUNCTION);
    lowering.asm.push(channel, true, true);
    // Discard the rest of a call: everything up to its terminator.
    lowering.asm.place(lowering.drain);
    lowering.asm.pull(channel);
    lowering.asm.and(EOP_TAG);
    lowering.asm.jump_if_zero(lowering.drain);
    lowering.asm.jump(lowering.dispatch);

    let mut functions_json = BTreeMap::new();
    for (id, (item, entry)) in exported.iter().zip(entries).enumerate() {
        let (_, name, params, returns, body) = function_parts(item);
        lowering.asm.place(entry);
        let mut scope = HashMap::new();
        let mut params_json = Vec::new();
        for param in params {
            params_json.push(lowering.param(param, &mut scope)?);
        }
        lowering.sends.clear();
        lowering.body(body, &scope)?;
        lowering.asm.pull(channel); // the terminator
        lowering.asm.load(0);
        lowering.asm.push(channel, true, false);
        lowering.asm.jump(lowering.dispatch);
        let result = match returns {
            None => Json::Null,
            Some(ty) if ty.name.text != "bytes" => {
                return Err(Error::new(
                    ty.name.span,
                    "exported functions return `bytes` or `bytes via <shifter>`",
                ));
            }
            Some(ty) => match &ty.via {
                Some(via) => json!({ "channel": chip.decls[&via.text].index }),
                None => json!("replies"),
            },
        };
        functions_json.insert(
            name.text.clone(),
            json!({
                "id": id,
                "params": params_json,
                "sends": sends_json(&lowering.sends),
                "result": result,
            }),
        );
    }
    Ok(Exports {
        channel: Some(channel),
        functions: functions_json,
        fails: lowering.fails,
    })
}

struct Lowering<'c, 'a> {
    chip: &'c Chip<'a>,
    asm: &'c mut Assembler,
    functions: HashMap<String, &'a Item>,
    channel: usize,
    fails: Vec<String>,
    /// What the host sends for the function being lowered.
    sends: Vec<Send>,
    in_loop: bool,
    /// Functions being inlined, to reject recursion.
    inlining: Vec<String>,
    dispatch: Label,
    drain: Label,
}

impl Lowering<'_, '_> {
    fn param(&self, param: &Param, scope: &mut HashMap<String, Binding>) -> Result<Json> {
        let ty = &param.ty;
        let (low, high) = match ty.name.text.as_str() {
            "bytes" => {
                scope.insert(
                    param.name.text.clone(),
                    Binding::Bytes(param.name.text.clone()),
                );
                let via = ty.via.as_ref().map(|v| self.chip.decls[&v.text].index);
                return Ok(json!({ "name": param.name.text, "type": "bytes", "via": via }));
            }
            "bit" => (0, 1),
            "u7" => (0, 127),
            "u8" => (0, 255),
            other => {
                return Err(Error::new(
                    ty.name.span,
                    format!("parameter types are bit, u7, u8 and bytes, not `{other}`"),
                ));
            }
        };
        if let Some(via) = &ty.via {
            return Err(Error::new(
                via.span,
                "only `bytes` parameters stream `via` a shifter",
            ));
        }
        let (low, high) = match &ty.range {
            None => (low, high),
            Some((a, b)) => {
                let (a_value, b_value) = (self.chip.env.integer(a)?, self.chip.env.integer(b)?);
                if a_value < low || b_value > high || a_value > b_value {
                    return Err(Error::new(
                        a.span.to(b.span),
                        format!("the range must lie within {low}..={high}"),
                    ));
                }
                (a_value, b_value)
            }
        };
        scope.insert(
            param.name.text.clone(),
            Binding::Host(HostExpr::Param(param.name.text.clone())),
        );
        Ok(json!({ "name": param.name.text, "type": "int", "min": low, "max": high }))
    }

    /// Constants plus the scope's constant bindings, for evaluating fields and counts.
    fn env(&self, scope: &HashMap<String, Binding>) -> Env {
        let mut consts = self.chip.env.consts.clone();
        let mut defs = self.chip.env.defs.clone();
        for (name, binding) in scope {
            if let Binding::Value(value) = binding {
                consts.insert(name.clone(), value.clone());
                defs.remove(name);
            }
        }
        Env {
            clock_hz: self.chip.env.clock_hz,
            consts,
            defs,
        }
    }

    fn body(&mut self, body: &[Stmt], scope: &HashMap<String, Binding>) -> Result<()> {
        body.iter().try_for_each(|stmt| self.stmt(stmt, scope))
    }

    fn operand(&self, expr: &Expr, scope: &HashMap<String, Binding>) -> Result<Operand> {
        let chip_read = |address: u16| {
            Ok(Operand::Chip {
                address,
                mask: None,
            })
        };
        match &expr.kind {
            ExprKind::Int(n) => Ok(Operand::Const(*n)),
            ExprKind::Name(name) => match scope.get(name) {
                Some(Binding::Host(h)) => Ok(Operand::Host(h.clone())),
                Some(Binding::Bytes(_)) => Err(Error::new(
                    expr.span,
                    format!("`{name}` is a list of bytes; use `{name}.len` or `for b in {name}`"),
                )),
                _ => Ok(Operand::Const(self.env(scope).integer(expr)?)),
            },
            ExprKind::Field(object, field) => {
                if let ExprKind::Name(name) = &object.kind
                    && let Some(Binding::Bytes(param)) = scope.get(name)
                    && field.text == "len"
                {
                    return Ok(Operand::Host(HostExpr::Len(param.clone())));
                }
                let (name, decl) = self.chip.decl(object)?;
                let k = decl.index as u16;
                match (decl.kind, field.text.as_str()) {
                    (Kind::Shifter, "received") => chip_read(1 + 2 * k),
                    (Kind::Shifter, "post_bits") => chip_read(2 + 2 * k),
                    (Kind::Counter, "count") => chip_read(5 + k),
                    (Kind::Crc, "low") => chip_read(7),
                    (Kind::Crc, "high") => chip_read(8),
                    (kind, other) => Err(Error::new(
                        field.span,
                        format!(
                            "the controller cannot read `{name}.{other}`; it can read a shifter's `.received` \
                             and `.post_bits`, and a counter's `.count` (this is a {})",
                            kind.noun()
                        ),
                    )),
                }
            }
            ExprKind::Binary(op @ ("+" | "-" | "<<" | ">>" | "|" | "&" | "^"), a, b) => {
                match (self.operand(a, scope)?, self.operand(b, scope)?) {
                    (Operand::Const(_), Operand::Const(_)) => {
                        Ok(Operand::Const(self.env(scope).integer(expr)?))
                    }
                    (
                        Operand::Chip {
                            address,
                            mask: None,
                        },
                        Operand::Const(m),
                    ) if *op == "&" => Ok(Operand::Chip {
                        address,
                        mask: Some(m as u16),
                    }),
                    (Operand::Chip { .. }, _) | (_, Operand::Chip { .. }) => Err(Error::new(
                        expr.span,
                        "the controller can only mask a chip value with a constant (`x & 1`); it has no add or shift",
                    )),
                    (a, b) => {
                        let host = |o: Operand| match o {
                            Operand::Const(n) => HostExpr::Int(n),
                            Operand::Host(h) => h,
                            Operand::Chip { .. } => unreachable!(),
                        };
                        Ok(Operand::Host(HostExpr::Op(
                            op,
                            Box::new(host(a)),
                            Box::new(host(b)),
                        )))
                    }
                }
            }
            _ => Ok(Operand::Const(self.env(scope).integer(expr)?)),
        }
    }

    /// Put an operand in `A`, pulling host values from the call channel.
    fn load(&mut self, operand: Operand, span: crate::Span) -> Result<()> {
        match operand {
            Operand::Const(n) => {
                let value = u16::try_from(n)
                    .map_err(|_| Error::new(span, format!("{n} does not fit in 16 bits")))?;
                self.asm.load(value);
            }
            Operand::Host(h) => {
                self.sends.push(Send::Value(h));
                self.asm.pull(self.channel);
            }
            Operand::Chip { address, mask } => {
                self.asm.read(address);
                if let Some(mask) = mask {
                    self.asm.and(mask);
                }
            }
        }
        Ok(())
    }

    /// Write a value to a configuration register, at configuration time when constant.
    fn write(&mut self, address: u8, operand: Operand, span: crate::Span) -> Result<()> {
        if let Operand::Const(n) = operand {
            let value = u16::try_from(n)
                .map_err(|_| Error::new(span, format!("{n} does not fit in 16 bits")))?;
            self.asm.config(address, value);
        } else {
            self.load(operand, span)?;
            self.asm.write_config(address);
        }
        Ok(())
    }

    /// Replace constant-bound names with literals, so tile fields evaluate in this scope.
    fn substitute(&self, expr: &Expr, scope: &HashMap<String, Binding>) -> Result<Expr> {
        let literal = |value: &Value, span| -> Result<Expr> {
            Ok(Expr {
                span,
                kind: match value {
                    Value::Number(r) => ExprKind::Int(
                        r.whole()
                            .ok_or_else(|| Error::new(span, "expected a whole number"))?,
                    ),
                    Value::List(items) => ExprKind::List(
                        items
                            .iter()
                            .map(|v| match v {
                                Value::Number(r) => Ok(Expr {
                                    span,
                                    kind: ExprKind::Int(r.whole().unwrap_or(0)),
                                }),
                                _ => Err(Error::new(span, "expected a list of numbers")),
                            })
                            .collect::<Result<_>>()?,
                    ),
                    _ => {
                        return Err(Error::new(
                            span,
                            "this value cannot be used in a tile setting",
                        ));
                    }
                },
            })
        };
        let sub = |e: &Expr| self.substitute(e, scope).map(Box::new);
        Ok(Expr {
            span: expr.span,
            kind: match &expr.kind {
                ExprKind::Name(name) => match scope.get(name) {
                    Some(Binding::Value(v)) => return literal(v, expr.span),
                    Some(_) => {
                        return Err(Error::new(
                            expr.span,
                            format!(
                                "`{name}` is only known when the host calls; set it with `tile.field = {name}` instead"
                            ),
                        ));
                    }
                    None => expr.kind.clone(),
                },
                ExprKind::Field(object, field) => ExprKind::Field(sub(object)?, field.clone()),
                ExprKind::Call(callee, args) => ExprKind::Call(
                    callee.clone(),
                    args.iter()
                        .map(|a| {
                            Ok(Arg {
                                name: a.name.clone(),
                                value: self.substitute(&a.value, scope)?,
                            })
                        })
                        .collect::<Result<_>>()?,
                ),
                ExprKind::List(items) => ExprKind::List(
                    items
                        .iter()
                        .map(|e| self.substitute(e, scope))
                        .collect::<Result<_>>()?,
                ),
                ExprKind::Unary(op, a) => ExprKind::Unary(op, sub(a)?),
                ExprKind::Binary(op, a, b) => ExprKind::Binary(op, sub(a)?, sub(b)?),
                ExprKind::If(c, t, e) => ExprKind::If(sub(c)?, sub(t)?, sub(e)?),
                other => other.clone(),
            },
        })
    }

    /// A runtime loop over `C`: the count is already in `A`.
    fn counted_loop(
        &mut self,
        span: crate::Span,
        body: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        if self.in_loop {
            return Err(Error::new(
                span,
                "the controller has one loop counter, so runtime loops cannot nest; give the inner loop a constant count",
            ));
        }
        let end = self.asm.label();
        let top = self.asm.label();
        self.asm.jump_if_zero(end);
        self.asm.counter_from_a();
        self.asm.keep_counter = true;
        self.in_loop = true;
        self.asm.place(top);
        body(self)?;
        self.asm.loop_back(top);
        self.in_loop = false;
        self.asm.keep_counter = false;
        self.asm.place(end);
        Ok(())
    }

    /// Lower `body` and return what it made the host send.
    fn collect_sends(&mut self, body: impl FnOnce(&mut Self) -> Result<()>) -> Result<Vec<Send>> {
        let outer = std::mem::take(&mut self.sends);
        let result = body(self);
        let inner = std::mem::replace(&mut self.sends, outer);
        result.map(|()| inner)
    }

    fn stmt(&mut self, stmt: &Stmt, scope: &HashMap<String, Binding>) -> Result<()> {
        match &stmt.kind {
            StmtKind::Fire(name) => {
                let expr = Expr {
                    kind: ExprKind::Name(name.text.clone()),
                    span: name.span,
                };
                match self.chip.decl(&expr)? {
                    (_, decl) if decl.kind == Kind::Event => self.asm.fire(decl.index),
                    (name, decl) => {
                        return Err(Error::new(
                            stmt.span,
                            format!(
                                "`{name}` is a {}; only events can be fired",
                                decl.kind.noun()
                            ),
                        ));
                    }
                }
            }
            StmtKind::Wait { signal: expr, next } => {
                if let ExprKind::Quantity(..) = expr.kind {
                    return Err(Error::new(
                        expr.span,
                        "the controller cannot wait for a time; start a timer and `wait timer.tick`",
                    ));
                }
                let line = self.chip.signal(expr)?;
                if *next {
                    self.asm.clear_events();
                }
                self.asm.wait(line.0);
            }
            StmtKind::Set { target, fields } => {
                let (name, decl) = self.chip.decl(target)?;
                let declared = self.chip.tiles.get(&name).ok_or_else(|| {
                    Error::new(
                        target.span,
                        format!(
                            "`{name}` is a {}; only tiles have settings",
                            decl.kind.noun()
                        ),
                    )
                })?;
                let overrides = fields
                    .iter()
                    .map(|f| {
                        Ok(Field {
                            name: f.name.clone(),
                            value: f
                                .value
                                .as_ref()
                                .map(|v| self.substitute(v, scope))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut merged: Vec<&Field> = declared.iter().collect();
                merged.extend(&overrides);
                let tile = Name {
                    text: name,
                    span: target.span,
                };
                for (address, value) in self.chip.tile(&tile, &merged)? {
                    self.asm.config(address, value);
                }
            }
            StmtKind::Assign { target, value } => {
                let ExprKind::Field(object, field) = &target.kind else {
                    return Err(Error::new(
                        target.span,
                        "assign to a tile setting, such as `counter.limit = n`",
                    ));
                };
                let (name, decl) = self.chip.decl(object)?;
                let k = decl.index;
                let (address, minus_one) = match (decl.kind, field.text.as_str()) {
                    (Kind::Counter, "limit") => (config::counter(k, 0), false),
                    (Kind::Timer, "every") => (config::timer(k, 0), true),
                    (Kind::Timer, "first") => (config::timer(k, 1), true),
                    _ => {
                        return Err(Error::new(
                            field.span,
                            format!(
                                "`{name}.{}` cannot be assigned; use `{name}.set {{ ... }}`. Assignable: counter `.limit`, timer `.every` and `.first` (in cycles)",
                                field.text
                            ),
                        ));
                    }
                };
                let mut operand = self.operand(value, scope)?;
                if minus_one {
                    operand = match operand {
                        Operand::Const(n) => Operand::Const(n - 1),
                        Operand::Host(h) => Operand::Host(HostExpr::Op(
                            "-",
                            Box::new(h),
                            Box::new(HostExpr::Int(1)),
                        )),
                        Operand::Chip { .. } => {
                            return Err(Error::new(value.span, "a chip value cannot set a timer"));
                        }
                    };
                }
                self.write(address, operand, value.span)?;
            }
            StmtKind::Expr(Expr {
                kind: ExprKind::Call(callee, args),
                span,
            }) => match &callee.kind {
                ExprKind::Field(object, method) if method.text == "load" => {
                    let k = self.chip.shifter_index(object)?;
                    let [arg] = args.as_slice() else {
                        return Err(Error::new(
                            *span,
                            "`load` takes one value: the next word to send",
                        ));
                    };
                    let operand = match self.env(scope).eval(&arg.value) {
                        // A bit list such as [0, 1, 1] loads as a word, first bit first.
                        Ok(Value::List(bits)) => Operand::Const(
                            bits.iter()
                                .enumerate()
                                .map(|(i, b)| match b {
                                    Value::Number(r) if *r == Ratio::int(1) => 1 << i,
                                    _ => 0,
                                })
                                .sum(),
                        ),
                        _ => self.operand(&arg.value, scope)?,
                    };
                    self.write(config::shifter(k, 0), operand, arg.value.span)?;
                }
                ExprKind::Name(function) if function == "flags" => {
                    // `flags(a, b)` raises the named flags and lowers the rest.
                    let mut value = 0u16;
                    for arg in args {
                        match self.chip.decl(&arg.value)? {
                            (_, decl) if decl.kind == Kind::Flag => value |= 1 << decl.index,
                            (name, decl) => {
                                return Err(Error::new(
                                    arg.value.span,
                                    format!("`{name}` is a {}, not a flag", decl.kind.noun()),
                                ));
                            }
                        }
                    }
                    self.asm.config(config::FLAGS, value);
                }
                ExprKind::Field(object, method)
                    if matches!(method.text.as_str(), "release" | "drive") =>
                {
                    let (name, decl) = self.chip.decl(object)?;
                    if decl.kind != Kind::Pin || !args.is_empty() {
                        return Err(Error::new(
                            *span,
                            format!("`.{}()` applies to a pin", method.text),
                        ));
                    }
                    let writes = if method.text == "release" {
                        vec![(config::pin(self.chip.pads[decl.index]), 0)]
                    } else {
                        let (name, mode, source) = self.chip.pin_decls[&name];
                        self.chip.pin(name, mode, source)?
                    };
                    for (address, value) in writes {
                        self.asm.config(address, value);
                    }
                }
                ExprKind::Name(function) => self.inline(function, args, *span, scope)?,
                _ => {
                    return Err(Error::new(
                        *span,
                        "expected a function call or `shifter.load(value)`",
                    ));
                }
            },
            StmtKind::Repeat { count, body } => match self.operand(count, scope)? {
                Operand::Const(n) => {
                    for _ in 0..n {
                        self.body(body, scope)?;
                    }
                }
                operand @ Operand::Host(_) => {
                    let Operand::Host(h) = &operand else {
                        unreachable!()
                    };
                    let h = h.clone();
                    self.load(operand, count.span)?;
                    let inner =
                        self.collect_sends(|l| l.counted_loop(stmt.span, |l| l.body(body, scope)))?;
                    let Some(Send::Value(_)) = self.sends.pop() else {
                        unreachable!("the count was just sent")
                    };
                    self.sends.push(Send::Repeat {
                        count: h,
                        body: inner,
                    });
                }
                Operand::Chip { .. } => {
                    return Err(Error::new(
                        count.span,
                        "a repeat count must be a constant or come from the host",
                    ));
                }
            },
            StmtKind::For { var, iter, body } => {
                let list = match &iter.kind {
                    ExprKind::Name(name) => match scope.get(name) {
                        Some(Binding::Bytes(param)) => param.clone(),
                        _ => {
                            return Err(Error::new(
                                iter.span,
                                "`for` loops over a `bytes` parameter",
                            ));
                        }
                    },
                    _ => {
                        return Err(Error::new(
                            iter.span,
                            "`for` loops over a `bytes` parameter",
                        ));
                    }
                };
                self.asm.pull(self.channel); // the host sends the length first
                let mut inner_scope = scope.clone();
                inner_scope.insert(
                    var.text.clone(),
                    Binding::Host(HostExpr::Element(var.text.clone())),
                );
                let inner = self
                    .collect_sends(|l| l.counted_loop(stmt.span, |l| l.body(body, &inner_scope)))?;
                self.sends.push(Send::For {
                    var: var.text.clone(),
                    list,
                    body: inner,
                });
            }
            StmtKind::If {
                condition,
                then,
                otherwise,
            } => {
                let (chip, value, equal) = match &condition.kind {
                    ExprKind::Binary(op @ ("==" | "!="), a, b) => (
                        self.operand(a, scope)?,
                        self.operand(b, scope)?,
                        *op == "==",
                    ),
                    _ => (self.operand(condition, scope)?, Operand::Const(0), false),
                };
                let (Operand::Chip { .. }, Operand::Const(value)) = (&chip, value) else {
                    return Err(Error::new(
                        condition.span,
                        "a condition compares something the chip measured (`shifter.post_bits & 1`) with a constant",
                    ));
                };
                self.load(chip, condition.span)?;
                self.asm.xor(value as u16);
                // A is zero exactly when the chip value equals `value`.
                let (when_zero, when_nonzero) = if equal {
                    (then, otherwise)
                } else {
                    (otherwise, then)
                };
                let zero = self.asm.label();
                let end = self.asm.label();
                self.asm.jump_if_zero(zero);
                let a = self.collect_sends(|l| l.body(when_nonzero, scope))?;
                self.asm.jump(end);
                self.asm.place(zero);
                let b = self.collect_sends(|l| l.body(when_zero, scope))?;
                self.asm.place(end);
                if !a.is_empty() || !b.is_empty() {
                    return Err(Error::new(
                        stmt.span,
                        "the branches of an `if` cannot use the host's arguments, because the host cannot know which branch runs",
                    ));
                }
            }
            StmtKind::Reply(expr) => {
                let operand = self.operand(expr, scope)?;
                self.load(operand, expr.span)?;
                self.asm.push(self.channel, false, false);
            }
            StmtKind::Fail(message) => {
                let code = match self.fails.iter().position(|f| *f == message.text) {
                    Some(i) => i + 1,
                    None => {
                        self.fails.push(message.text.clone());
                        self.fails.len()
                    }
                };
                self.asm.load(code as u16);
                self.asm.push(self.channel, true, true);
                self.asm.jump(self.drain);
            }
            _ => {
                return Err(Error::new(
                    stmt.span,
                    "this statement belongs in a test, not a function",
                ));
            }
        }
        Ok(())
    }

    fn inline(
        &mut self,
        function: &str,
        args: &[Arg],
        span: crate::Span,
        scope: &HashMap<String, Binding>,
    ) -> Result<()> {
        let item = *self
            .functions
            .get(function)
            .ok_or_else(|| Error::new(span, format!("there is no function `{function}`")))?;
        let (exported, _, params, _, body) = function_parts(item);
        if *exported {
            return Err(Error::new(
                span,
                format!("`{function}` is exported, so only the host calls it"),
            ));
        }
        if self.inlining.iter().any(|f| f == function) {
            return Err(Error::new(
                span,
                format!("`{function}` calls itself; the controller has no call stack"),
            ));
        }
        let mut bound = HashMap::new();
        for (i, arg) in args.iter().enumerate() {
            let param = match &arg.name {
                Some(name) => params.iter().find(|p| p.name.text == name.text),
                None => params.get(i),
            }
            .ok_or_else(|| {
                Error::new(
                    arg.value.span,
                    format!("`{function}` has no parameter here"),
                )
            })?;
            let binding = match &arg.value.kind {
                ExprKind::Name(n) if matches!(scope.get(n), Some(Binding::Bytes(_))) => {
                    scope[n].clone()
                }
                _ => match self.env(scope).eval(&arg.value) {
                    Ok(value) => Binding::Value(value),
                    Err(_) => match self.operand(&arg.value, scope)? {
                        Operand::Host(h) => Binding::Host(h),
                        Operand::Const(n) => Binding::Value(Value::Number(Ratio::int(n))),
                        Operand::Chip { .. } => {
                            return Err(Error::new(
                                arg.value.span,
                                "pass a constant or a host value; read chip values inside the function",
                            ));
                        }
                    },
                },
            };
            if bound.insert(param.name.text.clone(), binding).is_some() {
                return Err(Error::new(
                    arg.value.span,
                    format!("`{}` is given twice", param.name.text),
                ));
            }
        }
        if let Some(missing) = params.iter().find(|p| !bound.contains_key(&p.name.text)) {
            return Err(Error::new(
                span,
                format!("`{function}` needs `{}`", missing.name.text),
            ));
        }
        self.inlining.push(function.into());
        let result = self.body(body, &bound);
        self.inlining.pop();
        result
    }
}
