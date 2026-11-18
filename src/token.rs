//! JASS's token vocabulary: its keywords, on top of the shared
//! [`parser_rs::TokenKind`] (identifiers, literals, [`Punct`]uation).

pub use parser_rs::{Keyword, Punct};

parser_rs::keywords! {
    /// Every JASS keyword.
    pub enum Kw {
        Function = "function",
        EndFunction = "endfunction",
        Takes = "takes",
        Returns = "returns",
        Nothing = "nothing",
        Native = "native",
        Globals = "globals",
        EndGlobals = "endglobals",
        Constant = "constant",
        Local = "local",
        Set = "set",
        Call = "call",
        If = "if",
        Then = "then",
        ElseIf = "elseif",
        Else = "else",
        EndIf = "endif",
        Loop = "loop",
        EndLoop = "endloop",
        ExitWhen = "exitwhen",
        Return = "return",
        Type = "type",
        Extends = "extends",
        Array = "array",
        And = "and",
        Or = "or",
        Not = "not",
        True = "true",
        False = "false",
        Null = "null",
    }
}

/// A JASS token kind — the shared [`parser_rs::TokenKind`] over [`Kw`].
pub type TokenKind = parser_rs::TokenKind<Kw>;

/// A JASS token.
pub type Token = parser_rs::Token<TokenKind>;
