use crate::token::{Kw, Token};
use parser_rs::{LexSpec, ParseError, Punct};

/// JASS's lexical rules for the shared [`parser_rs::lexer`]: `//` comments
/// only, rawcodes, and JASS's small operator set.
pub const SPEC: LexSpec = LexSpec {
    line_comment: "//",
    block_comment: None,
    rawcodes: true,
    puncts: &[
        Punct::Plus,
        Punct::Minus,
        Punct::Star,
        Punct::Slash,
        Punct::EqEq,
        Punct::NotEq,
        Punct::Gt,
        Punct::Lt,
        Punct::GtEq,
        Punct::LtEq,
        Punct::Assign,
        Punct::LParen,
        Punct::RParen,
        Punct::LBracket,
        Punct::RBracket,
        Punct::Comma,
    ],
};

pub struct Lexer;

impl Lexer {
    pub fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
        parser_rs::tokenize::<Kw>(src, &SPEC)
    }
}

#[cfg(test)]
#[path = "lexer.test.rs"]
mod tests;
