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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
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
    Not,
}
