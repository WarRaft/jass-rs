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

/// A source location, used to map an AST node back to the exact text it was
/// parsed from (for the HTML viewer's click-to-highlight sync in both
/// directions). Columns are 1-indexed; `end_col` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    pub fn new(start_line: usize, start_col: usize, end_line: usize, end_col: usize) -> Self {
        Span {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    /// A span with no column information, meaning "somewhere on this line" —
    /// used for statements, which only track their starting line.
    pub fn whole_line(line: usize) -> Self {
        Span::new(line, 0, line, 0)
    }

    fn to(self, other: Span) -> Span {
        Span::new(
            self.start_line,
            self.start_col,
            other.end_line,
            other.end_col,
        )
    }

    /// Recodes this span's columns into `encoding`, given the source text
    /// it was parsed from. Only columns change — `Span` always stores plain
    /// Unicode codepoint counts internally (the cheapest, simplest
    /// representation, and the one every consumer gets for free), and
    /// recoding into a consumer's own convention (UTF-16 for VS Code/LSP,
    /// UTF-8 bytes, ...) happens on demand for just the span(s) actually
    /// needed. That keeps `Span` itself at a fixed 4 `usize`s regardless of
    /// how many encodings exist, instead of multiplying its size across
    /// every AST node in a large file for encodings most callers never use.
    ///
    /// A "whole line" span (see [`Span::whole_line`], `start_col`/`end_col`
    /// both `0`) is returned unchanged: there's no column to recode.
    /// Lines outside `source` are also left unchanged.
    pub fn in_encoding(self, source: &str, encoding: ColumnEncoding) -> Span {
        let recode = |line: usize, col: usize| {
            line.checked_sub(1)
                .and_then(|i| source.lines().nth(i))
                .map(|text| encoding.from_codepoints(text, col))
                .unwrap_or(col)
        };
        Span::new(
            self.start_line,
            recode(self.start_line, self.start_col),
            self.end_line,
            recode(self.end_line, self.end_col),
        )
    }
}

/// How a character offset within a line is counted. `Span` always stores
/// plain Unicode codepoint counts (see [`Span::in_encoding`] for why); this
/// picks the unit a *consumer* wants their columns translated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEncoding {
    /// One unit per Unicode codepoint (`char`). `Span`'s native counting,
    /// so converting to/from it is a no-op.
    Codepoints,
    /// One unit per UTF-16 code unit — two for codepoints outside the
    /// Basic Multilingual Plane (emoji, some CJK extension ideographs, and
    /// the supplementary-plane Private Use Area some icon fonts use, e.g.
    /// for in-game glyphs). This is what VS Code's and LSP's default
    /// `Position` expects, because both are ultimately backed by
    /// UTF-16-native JavaScript strings.
    Utf16,
    /// One unit per UTF-8 byte. Matches LSP's `positionEncoding: "utf-8"`
    /// and any tool that works on raw byte offsets.
    Utf8,
}

impl ColumnEncoding {
    /// Converts `col`, measured in `self`'s units on `line_text`, to a
    /// codepoint-counted column (`Span`'s native representation). `0` (the
    /// "whole line" sentinel) always maps to `0` unchanged. A `col` that
    /// falls strictly inside a multi-unit codepoint (e.g. mid-surrogate-pair
    /// for `Utf16`) snaps forward to the codepoint boundary after it,
    /// rather than panicking or guessing.
    pub fn to_codepoints(self, line_text: &str, col: usize) -> usize {
        if col == 0 || self == ColumnEncoding::Codepoints {
            return col;
        }
        let mut units = 0usize;
        for (i, c) in line_text.chars().enumerate() {
            if units >= col - 1 {
                return i + 1;
            }
            units += self.unit_len(c);
        }
        line_text.chars().count() + 1
    }

    /// The inverse of [`to_codepoints`](Self::to_codepoints): converts a
    /// codepoint-counted column into one measured in `self`'s units.
    pub fn from_codepoints(self, line_text: &str, codepoint_col: usize) -> usize {
        if codepoint_col == 0 || self == ColumnEncoding::Codepoints {
            return codepoint_col;
        }
        line_text
            .chars()
            .take(codepoint_col - 1)
            .map(|c| self.unit_len(c))
            .sum::<usize>()
            + 1
    }

    fn unit_len(self, c: char) -> usize {
        match self {
            ColumnEncoding::Codepoints => 1,
            ColumnEncoding::Utf16 => c.len_utf16(),
            ColumnEncoding::Utf8 => c.len_utf8(),
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
        Expr::new(kind, from.to(to))
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
    Not,
}

#[cfg(test)]
#[path = "ast.test.rs"]
mod tests;
