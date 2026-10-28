//! The parsed program, before names are resolved or hardware is assigned.
use crate::diagnostic::Span;

#[derive(Clone, Debug)]
pub(crate) struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Debug)]
pub(crate) enum Item {
    /// `clock 50 MHz`
    Clock(Expr),
    /// `const BIT = 64 cycles`
    Const { name: Name, value: Expr },
    /// `pin tx @ [1, 2]: out = sender.out`
    Pin {
        name: Name,
        pads: Option<Vec<Expr>>,
        mode: Name,
        source: Option<Expr>,
    },
    /// `timer bit { ... }`, `shifter sender { ... }`, `counter sent { ... }`
    Tile {
        kind: Name,
        name: Name,
        fields: Vec<Field>,
    },
    /// `reg selected init 1 = begin || selected`
    Reg {
        name: Name,
        init: Option<Expr>,
        value: Expr,
    },
    /// `export stream send: host -> sender`
    Stream { name: Name, from: Expr, to: Expr },
    /// `test "loopback" { ... }`
    Test { name: Name, body: Vec<Stmt> },
    /// `always "cs idles high" { expr }`
    Always { name: Name, condition: Expr },
}

/// `name: value`, or a bare flag such as `pulse`.
#[derive(Debug)]
pub(crate) struct Field {
    pub name: Name,
    pub value: Option<Expr>,
}

#[derive(Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub(crate) enum StmtKind {
    Wait(Expr),
    /// `expect a == b`
    Expect(Expr),
    Attach {
        peer: Expr,
        alias: Option<Name>,
    },
    Wire {
        from: Expr,
        to: Expr,
    },
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum ExprKind {
    Int(i128),
    /// `64 cycles`, `50 MHz`, `2 us`
    Quantity(i128, String),
    Str(String),
    Name(String),
    Field(Box<Expr>, Name),
    Call(Box<Expr>, Vec<Arg>),
    List(Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub(crate) struct Arg {
    pub name: Option<Name>,
    pub value: Expr,
}
