//! The parsed program, before names are resolved or hardware is assigned.
use crate::diagnostic::Span;

#[derive(Clone, Debug)]
pub(crate) struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum Item {
    /// Internal selector lowered from `contexts(phase) { ... }`.
    Contexts(Expr),
    /// `context 1 { pin tx: out = other.out }`: override existing resources.
    Context { index: Expr, body: Vec<Item> },
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
    /// `reg selected init 1 = begin || selected`; with `on bit.tick` it
    /// updates only on that pulse instead of every cycle.
    Reg {
        name: Name,
        init: Option<Expr>,
        on: Option<Expr>,
        value: Expr,
    },
    /// `event begin, finish`
    Event(Vec<Name>),
    /// `flag sending = 0, ending = 1`: controller levels, zero by default.
    Flag(Vec<(Name, Option<Expr>)>),
    /// `export stream send: host -> sender`
    Stream { name: Name, from: Expr, to: Expr },
    /// `export fn transfer(...) -> bytes { ... }` or a private `fn`.
    Fn {
        exported: bool,
        name: Name,
        params: Vec<Param>,
        returns: Option<Type>,
        body: Vec<Stmt>,
    },
    /// `test "loopback" { ... }`
    Test { name: Name, body: Vec<Stmt> },
    /// `always "cs idles high" { expr }`
    Always { name: Name, condition: Expr },
}

/// `name: value`, or a bare flag such as `pulse`.
#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub name: Name,
    pub value: Option<Expr>,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub name: Name,
    pub ty: Type,
}

/// `u8`, `bytes via bits`, `u8 in 1..=16`
#[derive(Clone, Debug)]
pub(crate) struct Type {
    pub name: Name,
    pub via: Option<Name>,
    pub range: Option<(Expr, Expr)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum StmtKind {
    Fire(Name),
    /// `wait x`, or `wait next x` to ignore events that happened earlier.
    Wait {
        signal: Expr,
        next: bool,
    },
    Repeat {
        count: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: Name,
        iter: Expr,
        body: Vec<Stmt>,
    },
    If {
        condition: Expr,
        then: Vec<Stmt>,
        otherwise: Vec<Stmt>,
    },
    Reply(Expr),
    Fail(Name),
    /// `expect a == b`, or `expect call fails "reason"`.
    Expect {
        condition: Expr,
        fails: Option<Name>,
    },
    Attach {
        peer: Expr,
        alias: Option<Name>,
    },
    Wire {
        from: Expr,
        to: Expr,
    },
    /// `sender.set { frame: [...] }`
    Set {
        target: Expr,
        fields: Vec<Field>,
    },
    Assign {
        target: Expr,
        value: Expr,
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
