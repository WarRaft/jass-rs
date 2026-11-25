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
        /// vJASS's dynamic array type, `type T extends integer array [16]`:
        /// its size.
        size: Option<Expr>,
        modifiers: Modifiers,
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
    /// vJASS: `library` (`library_once`) ... `endlibrary`, `scope` ...
    /// `endscope`.
    Library(LibraryDecl),
    /// vJASS: `struct` ... `endstruct`.
    Struct(StructDecl),
    /// vJASS: `interface` ... `endinterface`: methods' signatures.
    Interface(TypeDecl),
    /// vJASS: `module` ... `endmodule`: members a struct takes in.
    Module(TypeDecl),
    /// vJASS: `function interface F takes ... returns ...`: a type of
    /// functions.
    FunctionInterface(FunctionSig),
    /// vJASS: `keyword name`: a name of the scope, declared ahead.
    Keyword {
        modifiers: Modifiers,
        name: String,
        line: usize,
    },
    /// vJASS: `hook Name Target`: before every call of `Name` (a native
    /// or a function), `Target` (a function, or a struct's static method
    /// `S.m`) is called with the same arguments.
    Hook {
        name: String,
        target: String,
        line: usize,
    },
}

impl TopLevel {
    /// The line the item starts on.
    pub fn line(&self) -> usize {
        match self {
            TopLevel::Globals(g) => g.first().map_or(0, |g| g.line),
            TopLevel::TypeDef { line, .. } | TopLevel::Keyword { line, .. } | TopLevel::Hook { line, .. } => *line,
            TopLevel::Native(sig) | TopLevel::FunctionInterface(sig) => sig.line,
            TopLevel::Function(f) => f.sig.line,
            TopLevel::Global(g) => g.line,
            TopLevel::Stmt(s) => s.line(),
            TopLevel::Zinc(z) => z.open.start_line,
            TopLevel::Library(l) => l.line,
            TopLevel::Struct(s) => s.line,
            TopLevel::Interface(t) | TopLevel::Module(t) => t.line,
        }
    }
}

/// Who sees a vJASS declaration of a library or scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    /// Everyone, by its name.
    #[default]
    Default,
    /// Its scope by its name, everyone else as `Scope_name`.
    Public,
    /// Its scope only (in a struct: the struct only).
    Private,
}

/// vJASS's modifiers before a declaration, in any order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Modifiers {
    pub access: Access,
    pub is_static: bool,
    pub is_readonly: bool,
    pub is_stub: bool,
    pub is_delegate: bool,
}

/// A vJASS library or scope.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryDecl {
    pub name: String,
    /// A `scope` (no `requires`; may be in a library or a scope).
    pub scope: bool,
    /// `library_once`: a second one of the name is skipped.
    pub once: bool,
    /// `initializer F`.
    pub initializer: Option<String>,
    /// `requires` (`needs`, `uses`): each name, and whether `optional`.
    pub requires: Vec<(String, bool)>,
    pub items: Vec<TopLevel>,
    pub line: usize,
    /// The line of `endlibrary` / `endscope`.
    pub end_line: usize,
}

/// A vJASS struct.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub modifiers: Modifiers,
    pub name: String,
    /// `struct S[20000]`: how many instances.
    pub size: Option<Expr>,
    /// `extends array`, a struct or an interface.
    pub extends: Option<String>,
    pub members: Vec<StructMember>,
    pub line: usize,
    pub end_line: usize,
}

/// A vJASS interface or module: a name and members.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub modifiers: Modifiers,
    pub name: String,
    /// An interface's index space (`interface A[20000]`).
    pub size: Option<Expr>,
    pub members: Vec<StructMember>,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructMember {
    /// A field: its modifiers say if it's `static`, `readonly`,
    /// `delegate`, private.
    Field(GlobalDecl),
    Method(MethodDecl),
    /// `implement [optional] Module`.
    Implement { name: String, optional: bool, line: usize },
}

/// A method: an operator's name is `operator []`, `operator x=` ...; an
/// interface's has no body, and maybe a value it `defaults` to.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub sig: FunctionSig,
    pub body: Option<Vec<Stmt>>,
    pub defaults: Option<Expr>,
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
    /// vJASS: `private`, `public`, `static` ...
    pub modifiers: Modifiers,
    /// vJASS: a sized array, `integer array a[100]`.
    pub size: Option<Expr>,
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
    /// vJASS: `private`, `public`, `static`, `stub`.
    pub modifiers: Modifiers,
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
        /// Whether `local` is written. Left out (`integer i = 0` in a
        /// function), this toolchain reads a local, and the game doesn't
        /// (`implicit-keyword`): globals can't be declared in a function.
        keyword: bool,
    },
    Set {
        name: String,
        index: Option<Expr>,
        value: Expr,
        line: usize,
        /// Whether `set` is written. Left out (`x = 1`), this toolchain
        /// reads an assignment, and the game doesn't (`implicit-keyword`).
        keyword: bool,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        line: usize,
        /// Whether `call` is written. Left out (`Foo()`), this toolchain
        /// reads a call, and the game doesn't (`implicit-keyword`).
        keyword: bool,
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
    /// vJASS: `set a.b = v`, `set .b[i] = v`: `target` the member
    /// (`ExprKind::Member`).
    SetMember {
        target: Expr,
        index: Option<Expr>,
        value: Expr,
        line: usize,
        keyword: bool,
    },
    /// vJASS: `call a.b(...)`: `call` an `ExprKind::MethodCall`.
    CallMethod {
        call: Expr,
        line: usize,
        keyword: bool,
    },
    /// vJASS: `static if`: kept or dropped by constants.
    StaticIf {
        branches: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
        line: usize,
        end_line: usize,
    },
}

impl Stmt {
    pub fn line(&self) -> usize {
        match self {
            Stmt::Local { line, .. }
            | Stmt::Set { line, .. }
            | Stmt::Call { line, .. }
            | Stmt::If { line, .. }
            | Stmt::Loop { line, .. }
            | Stmt::ExitWhen { line, .. }
            | Stmt::Return { line, .. }
            | Stmt::Debug { line, .. }
            | Stmt::SetMember { line, .. }
            | Stmt::CallMethod { line, .. }
            | Stmt::StaticIf { line, .. } => *line,
        }
    }
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
    /// A `function Foo` reference, e.g. passed to `Condition(...)`/`Filter(...)`;
    /// vJASS's `function Data.onTimer` too.
    FuncRef(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    /// vJASS: `this`.
    This,
    /// vJASS: `thistype`.
    ThisType,
    /// vJASS: `super`.
    Super,
    /// vJASS: `object.name`; `.name` is `this.name` (`object` a `This`
    /// with an empty span).
    Member(Box<Expr>, String),
    /// vJASS: `object.name(args)`.
    MethodCall(Box<Expr>, String, Vec<Expr>),
    /// vJASS: an element of what isn't a name (`a.b[i]`).
    Index(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Direct sub-expressions, in source order.
    pub fn children(&self) -> Vec<&Expr> {
        match &self.kind {
            ExprKind::ArrayAccess(_, i) => vec![i],
            ExprKind::Call(_, args) => args.iter().collect(),
            ExprKind::Unary(_, x) | ExprKind::Member(x, _) => vec![x],
            ExprKind::Binary(a, _, b) | ExprKind::Index(a, b) => vec![a, b],
            ExprKind::MethodCall(o, _, args) => std::iter::once(&**o).chain(args).collect(),
            ExprKind::IntLiteral(_)
            | ExprKind::RealLiteral(_)
            | ExprKind::StringLiteral(_)
            | ExprKind::BoolLiteral(_)
            | ExprKind::Null
            | ExprKind::Var(_)
            | ExprKind::FuncRef(_)
            | ExprKind::This
            | ExprKind::ThisType
            | ExprKind::Super => Vec::new(),
        }
    }
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
