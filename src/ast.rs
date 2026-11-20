/// [`Span`] and [`ColumnEncoding`] live in the `parser-rs` crate — they're
/// not JASS-specific, and other hand-written parsers in this author's other
/// projects share the same source-location/column-recoding needs. See
/// `../parser-rs` (a sibling checkout, pulled in as a path dependency).
pub use parser_rs::{ColumnEncoding, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<TopLevel>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Globals(Vec<GlobalDecl>),
    TypeDef {
        name: String,
        extends: String,
        line: usize,
    },
    Native(FunctionSig),
    Function(FunctionDecl),
    /// A variable declared at the top level, outside `globals`. That's
    /// legal here: when the map is built, it goes into the `globals`
    /// block.
    Global(GlobalDecl),
    /// A statement at the top level (`if`, `set`, `call`, `loop`, ...).
    /// It's parsed in full so the rest of the file still parses, and lint
    /// reports it as an error: statements belong in a function.
    Stmt(Stmt),
    /// A vJASS `//! zinc` ... `//! endzinc` block, parsed by `zinc-rs`.
    Zinc(ZincBlock),
}

/// Zinc code embedded in a (v)JASS file. Every position inside `program`
/// is file-absolute, like the rest of the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct ZincBlock {
    pub program: zinc_rs::Program,
    /// The `//! zinc` directive line.
    pub open: Span,
    /// The `//! endzinc` directive line (always present once parsed).
    pub close: Option<Span>,
}

impl ZincBlock {
    /// The block's whole line range, directives included, as a
    /// whole-line span.
    pub fn span(&self) -> Span {
        let end = self.close.unwrap_or(self.open).end_line;
        Span::new(self.open.start_line, 0, end, 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalDecl {
    pub is_constant: bool,
    pub type_name: String,
    pub is_array: bool,
    pub name: String,
    pub initializer: Option<Expr>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub type_name: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<Param>,
    /// `None` means the function returns `nothing`.
    pub returns: Option<String>,
    pub is_constant: bool,
    pub line: usize,
    /// Whether `takes ...` is written. Omitted means `takes nothing`,
    /// which this toolchain accepts and the game doesn't
    /// (`implicit-signature`).
    pub has_takes: bool,
    /// Whether `returns ...` is written. Omitted means `returns nothing`.
    pub has_returns: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub sig: FunctionSig,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Local {
        type_name: String,
        is_array: bool,
        name: String,
        initializer: Option<Expr>,
        line: usize,
    },
    Set {
        name: String,
        index: Option<Expr>,
        value: Expr,
        line: usize,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        line: usize,
    },
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
        line: usize,
        /// The `else` keyword, when there's an `else` branch.
        else_span: Option<Span>,
        /// The line of `endif`.
        end_line: usize,
    },
    Loop {
        body: Vec<Stmt>,
        line: usize,
    },
    ExitWhen {
        cond: Expr,
        line: usize,
    },
    Return {
        value: Option<Expr>,
        line: usize,
    },
    /// `debug set|call|if|loop ...`: run only when the game runs scripts in
    /// debug mode, skipped otherwise. `stmt` is one of those four.
    Debug {
        stmt: Box<Stmt>,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

/// Compares only `kind`, ignoring `span` — spans are source-location
/// bookkeeping, not part of an expression's identity, and hand-writing
/// exact spans in test fixtures would be tedious and brittle.
impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span }
    }

    /// Builds two child spans into this expression's own span, e.g. for a
    /// binary expression spanning from its left operand's start to its
    /// right operand's end.
    pub fn spanning(kind: ExprKind, from: Span, to: Span) -> Self {
        Expr::new(kind, from.join(to))
    }

    #[cfg(test)]
    pub fn dummy(kind: ExprKind) -> Self {
        Expr::new(kind, Span::whole_line(0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntLiteral(i64),
    RealLiteral(f64),
    StringLiteral(String),
    BoolLiteral(bool),
    Null,
    Var(String),
    ArrayAccess(String, Box<Expr>),
    Call(String, Vec<Expr>),
    /// A `function Foo` reference, e.g. passed to `Condition(...)`/`Filter(...)`.
    FuncRef(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    /// `+x`: valid JASS (the game's own AI scripts use it), the value
    /// unchanged.
    Plus,
    Not,
}
