use crate::token::{JASS_KEYWORDS, Token, VJASS_KEYWORDS};
use parser_rs::{EmbedSpec, LexSpec, ParseError, Punct};

/// JASS's lexical rules for the shared [`parser_rs::lexer`]: `//` comments
/// only, rawcodes, and JASS's small operator set.
pub const SPEC: LexSpec = LexSpec {
    keywords: JASS_KEYWORDS,
    line_comment: "//",
    block_comment: None,
    rawcodes: true,
    embeds: &[],
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

/// A `//! zinc` ... `//! endzinc` block: Zinc embedded in vJASS. The lexer
/// turns each block into one `TokenKind::Embedded` token, and the parser
/// hands it to `zinc-rs` (see `TopLevel::Zinc`).
pub const ZINC_EMBED: EmbedSpec = EmbedSpec {
    lang: "zinc",
    open: "//! zinc",
    close: "//! endzinc",
};

/// The vJASS dialect: JASS plus vJASS keywords, `/* */` comments and
/// embedded Zinc blocks. Same tokens, same parser; only the active
/// vocabulary differs. vJASS's own constructs (`library`, `struct`, ...)
/// aren't parsed yet.
pub const VJASS_SPEC: LexSpec = LexSpec {
    keywords: VJASS_KEYWORDS,
    block_comment: Some(("/*", "*/")),
    embeds: &[ZINC_EMBED],
    ..SPEC
};

/// Which JASS dialect to read; it picks the [`LexSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// Plain JASS: `//! zinc` is just a comment, `library` is a name.
    #[default]
    Jass,
    /// vJASS (JassHelper), including embedded `//! zinc` blocks.
    VJass,
}

impl Dialect {
    pub fn spec(self) -> &'static LexSpec {
        match self {
            Dialect::Jass => &SPEC,
            Dialect::VJass => &VJASS_SPEC,
        }
    }
}

pub struct Lexer;

impl Lexer {
    pub fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
        Lexer::tokenize_with(src, Dialect::Jass)
    }

    pub fn tokenize_with(src: &str, dialect: Dialect) -> Result<Vec<Token>, ParseError> {
        parser_rs::tokenize(src, dialect.spec())
    }
}

#[cfg(test)]
#[path = "lexer.test.rs"]
mod tests;
