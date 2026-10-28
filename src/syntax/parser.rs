//! Recursive-descent parser. Newlines are insignificant; every construct is
//! self-delimiting, and commas between fields and statements are optional.
use super::ast::{Arg, Expr, ExprKind, Field, Item, Name, Param, Stmt, StmtKind, Type};
use super::lexer::{Kind, Token};
use crate::diagnostic::{Error, Result, Span};

const UNITS: [&str; 9] = ["cycle", "cycles", "ns", "us", "ms", "s", "Hz", "kHz", "MHz"];

/// Binary operators from loosest to tightest binding.
const LEVELS: [&[&str]; 10] = [
    &["until", "at"],
    &["implies"],
    &["||"],
    &["&&"],
    &["==", "!=", "<", ">", "<=", ">="],
    &["|"],
    &["^"],
    &["&"],
    &["<<", ">>", "+", "-"],
    &["*", "/"],
];

pub(crate) struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    pub(crate) fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, at: 0 }
    }

    fn peek(&self) -> &Kind {
        &self.tokens[self.at].kind
    }

    fn peek_at(&self, offset: usize) -> &Kind {
        let index = (self.at + offset).min(self.tokens.len() - 1);
        &self.tokens[index].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.at].span
    }

    fn previous(&self) -> Span {
        self.tokens[self.at.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.at].clone();
        if self.at + 1 < self.tokens.len() {
            self.at += 1;
        }
        token
    }

    fn is(&self, symbol: &str) -> bool {
        matches!(self.peek(), Kind::Sym(s) if *s == symbol)
            || matches!(self.peek(), Kind::Ident(s) if s == symbol)
    }

    fn eat(&mut self, symbol: &str) -> bool {
        let found = self.is(symbol);
        if found {
            self.bump();
        }
        found
    }

    fn expect(&mut self, symbol: &str) -> Result<Span> {
        if self.is(symbol) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(&format!("`{symbol}`")))
        }
    }

    fn unexpected(&self, wanted: &str) -> Error {
        let found = match self.peek() {
            Kind::Ident(s) => format!("`{s}`"),
            Kind::Int(n) => format!("`{n}`"),
            Kind::Str(s) => format!("\"{s}\""),
            Kind::Sym(s) => format!("`{s}`"),
            Kind::Eof => "end of file".into(),
        };
        Error::new(self.span(), format!("expected {wanted}, found {found}"))
    }

    fn name(&mut self) -> Result<Name> {
        match self.peek().clone() {
            Kind::Ident(text) => Ok(Name {
                text,
                span: self.bump().span,
            }),
            _ => Err(self.unexpected("a name")),
        }
    }

    fn string(&mut self) -> Result<Name> {
        match self.peek().clone() {
            Kind::Str(text) => Ok(Name {
                text,
                span: self.bump().span,
            }),
            _ => Err(self.unexpected("a quoted name")),
        }
    }

    pub(crate) fn file(&mut self) -> Result<Vec<Item>> {
        let mut items = Vec::new();
        while *self.peek() != Kind::Eof {
            items.push(self.item()?);
        }
        Ok(items)
    }

    fn item(&mut self) -> Result<Item> {
        let keyword = self.name()?;
        Ok(match keyword.text.as_str() {
            "clock" => Item::Clock(self.expr()?),
            "const" => {
                let name = self.name()?;
                self.expect("=")?;
                Item::Const {
                    name,
                    value: self.expr()?,
                }
            }
            "pin" => {
                let name = self.name()?;
                let pads = if self.eat("@") {
                    Some(if self.eat("[") {
                        self.list("]", Self::expr)?
                    } else {
                        vec![self.expr()?]
                    })
                } else {
                    None
                };
                self.expect(":")?;
                let mode = self.name()?;
                let source = if self.eat("=") {
                    Some(self.expr()?)
                } else {
                    None
                };
                Item::Pin {
                    name,
                    pads,
                    mode,
                    source,
                }
            }
            "timer" | "shifter" | "counter" | "crc" => Item::Tile {
                kind: keyword,
                name: self.name()?,
                fields: self.fields()?,
            },
            "reg" => {
                let name = self.name()?;
                let init = if self.eat("init") {
                    Some(self.expr()?)
                } else {
                    None
                };
                let on = if self.eat("on") {
                    Some(self.binary(1)?)
                } else {
                    None
                };
                self.expect("=")?;
                Item::Reg {
                    name,
                    init,
                    on,
                    value: self.expr()?,
                }
            }
            "event" | "flag" => {
                let mut names = vec![self.name()?];
                while self.eat(",") {
                    names.push(self.name()?);
                }
                if keyword.text == "event" {
                    Item::Event(names)
                } else {
                    Item::Flag(names)
                }
            }
            "export" if self.eat("stream") => {
                let name = self.name()?;
                self.expect(":")?;
                let from = self.expr()?;
                self.expect("->")?;
                Item::Stream {
                    name,
                    from,
                    to: self.expr()?,
                }
            }
            "export" if self.eat("fn") => self.function(true)?,
            "fn" => self.function(false)?,
            "test" => Item::Test {
                name: self.string()?,
                body: self.block()?,
            },
            "always" => {
                let name = self.string()?;
                self.expect("{")?;
                let condition = self.expr()?;
                self.expect("}")?;
                Item::Always { name, condition }
            }
            _ => {
                return Err(Error::new(
                    keyword.span,
                    format!(
                        "expected a declaration (clock, const, pin, timer, shifter, counter, crc, \
                         reg, event, flag, fn, export, test or always), found `{}`",
                        keyword.text
                    ),
                ));
            }
        })
    }

    fn function(&mut self, exported: bool) -> Result<Item> {
        let name = self.name()?;
        self.expect("(")?;
        let params = self.list(")", |p| {
            let name = p.name()?;
            p.expect(":")?;
            Ok(Param { name, ty: p.ty()? })
        })?;
        let returns = if self.eat("->") {
            Some(self.ty()?)
        } else {
            None
        };
        Ok(Item::Fn {
            exported,
            name,
            params,
            returns,
            body: self.block()?,
        })
    }

    fn ty(&mut self) -> Result<Type> {
        let name = self.name()?;
        let via = if self.eat("via") {
            Some(self.name()?)
        } else {
            None
        };
        let range = if self.eat("in") {
            let low = self.binary(LEVELS.len() - 2)?;
            self.expect("..=")?;
            Some((low, self.binary(LEVELS.len() - 2)?))
        } else {
            None
        };
        Ok(Type { name, via, range })
    }

    /// Comma-separated items up to `close`; a trailing comma is allowed.
    fn list<T>(
        &mut self,
        close: &str,
        mut item: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut items = Vec::new();
        while !self.eat(close) {
            items.push(item(self)?);
            if !self.eat(",") {
                self.expect(close)?;
                break;
            }
        }
        Ok(items)
    }

    fn fields(&mut self) -> Result<Vec<Field>> {
        self.expect("{")?;
        let mut fields = Vec::new();
        while !self.eat("}") {
            let name = self.name()?;
            let value = if self.eat(":") {
                Some(self.expr()?)
            } else {
                None
            };
            fields.push(Field { name, value });
            self.eat(",");
        }
        Ok(fields)
    }

    fn block(&mut self) -> Result<Vec<Stmt>> {
        self.expect("{")?;
        let mut body = Vec::new();
        while !self.eat("}") {
            body.push(self.stmt()?);
            self.eat(",");
        }
        Ok(body)
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let start = self.span();
        let keyword = match self.peek() {
            Kind::Ident(s) => s.clone(),
            _ => String::new(),
        };
        // A keyword followed by `.`, `(` or `=` is an ordinary name instead.
        let is_keyword = !matches!(self.peek_at(1), Kind::Sym("." | "(" | "="));
        let kind = match keyword.as_str() {
            "fire" if is_keyword => {
                self.bump();
                StmtKind::Fire(self.name()?)
            }
            "wait" if is_keyword => {
                self.bump();
                let next = matches!(self.peek(), Kind::Ident(n) if n == "next")
                    && matches!(self.peek_at(1), Kind::Ident(_));
                if next {
                    self.bump();
                }
                StmtKind::Wait {
                    signal: self.expr()?,
                    next,
                }
            }
            "repeat" if is_keyword => {
                self.bump();
                StmtKind::Repeat {
                    count: self.expr()?,
                    body: self.block()?,
                }
            }
            "for" if is_keyword => {
                self.bump();
                let var = self.name()?;
                self.expect("in")?;
                StmtKind::For {
                    var,
                    iter: self.expr()?,
                    body: self.block()?,
                }
            }
            "if" if is_keyword => {
                self.bump();
                let condition = self.expr()?;
                let then = self.block()?;
                let otherwise = if self.eat("else") {
                    self.block()?
                } else {
                    Vec::new()
                };
                StmtKind::If {
                    condition,
                    then,
                    otherwise,
                }
            }
            "reply" if is_keyword => {
                self.bump();
                StmtKind::Reply(self.expr()?)
            }
            "fail" if is_keyword => {
                self.bump();
                StmtKind::Fail(self.string()?)
            }
            "expect" if is_keyword => {
                self.bump();
                let condition = self.expr()?;
                let fails = if self.eat("fails") {
                    Some(self.string()?)
                } else {
                    None
                };
                StmtKind::Expect { condition, fails }
            }
            "attach" if is_keyword => {
                self.bump();
                let peer = self.expr()?;
                let alias = if self.eat("as") {
                    Some(self.name()?)
                } else {
                    None
                };
                StmtKind::Attach { peer, alias }
            }
            "wire" if is_keyword => {
                self.bump();
                let from = self.expr()?;
                self.expect("->")?;
                StmtKind::Wire {
                    from,
                    to: self.expr()?,
                }
            }
            _ => {
                let target = self.expr()?;
                if matches!(&target.kind, ExprKind::Field(_, field) if field.text == "set")
                    && self.is("{")
                {
                    let ExprKind::Field(object, _) = target.kind else {
                        unreachable!()
                    };
                    StmtKind::Set {
                        target: *object,
                        fields: self.fields()?,
                    }
                } else if self.eat("=") {
                    StmtKind::Assign {
                        target,
                        value: self.expr()?,
                    }
                } else {
                    StmtKind::Expr(target)
                }
            }
        };
        Ok(Stmt {
            kind,
            span: start.to(self.previous()),
        })
    }

    pub(crate) fn expr(&mut self) -> Result<Expr> {
        if self.eat("if") {
            let start = self.previous();
            let condition = self.expr()?;
            self.expect("{")?;
            let then = self.expr()?;
            self.expect("}")?;
            self.expect("else")?;
            let otherwise = if self.is("if") {
                self.expr()?
            } else {
                self.expect("{")?;
                let value = self.expr()?;
                self.expect("}")?;
                value
            };
            return Ok(Expr {
                span: start.to(self.previous()),
                kind: ExprKind::If(Box::new(condition), Box::new(then), Box::new(otherwise)),
            });
        }
        self.binary(0)
    }

    fn binary(&mut self, level: usize) -> Result<Expr> {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = LEVELS[level].iter().find(|op| self.is(op)) {
            self.bump();
            let right = self.binary(level + 1)?;
            left = Expr {
                span: left.span.to(right.span),
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        for op in ["!", "-"] {
            if self.is(op) {
                let start = self.bump().span;
                let operand = self.unary()?;
                return Ok(Expr {
                    span: start.to(operand.span),
                    kind: ExprKind::Unary(op, Box::new(operand)),
                });
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(".") {
                let field = self.name()?;
                expr = Expr {
                    span: expr.span.to(field.span),
                    kind: ExprKind::Field(Box::new(expr), field),
                };
            } else if self.eat("(") {
                let args = self.list(")", |p| {
                    let name = if matches!(p.peek(), Kind::Ident(_))
                        && matches!(p.peek_at(1), Kind::Sym(":"))
                    {
                        let name = p.name()?;
                        p.bump();
                        Some(name)
                    } else {
                        None
                    };
                    Ok(Arg {
                        name,
                        value: p.expr()?,
                    })
                })?;
                expr = Expr {
                    span: expr.span.to(self.previous()),
                    kind: ExprKind::Call(Box::new(expr), args),
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Kind::Int(value) => {
                self.bump();
                match self.peek().clone() {
                    Kind::Ident(unit) if UNITS.contains(&unit.as_str()) => {
                        self.bump();
                        ExprKind::Quantity(value, unit)
                    }
                    _ => ExprKind::Int(value),
                }
            }
            Kind::Str(text) => {
                self.bump();
                ExprKind::Str(text)
            }
            Kind::Ident(text) => {
                self.bump();
                ExprKind::Name(text)
            }
            Kind::Sym("(") => {
                self.bump();
                let inner = self.expr()?;
                self.expect(")")?;
                return Ok(Expr {
                    span: span.to(self.previous()),
                    kind: inner.kind,
                });
            }
            Kind::Sym("[") => {
                self.bump();
                ExprKind::List(self.list("]", Self::expr)?)
            }
            _ => return Err(self.unexpected("a value")),
        };
        Ok(Expr {
            span: span.to(self.previous()),
            kind,
        })
    }
}
