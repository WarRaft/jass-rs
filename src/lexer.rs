use crate::token::{JASS_KEYWORDS, Token, VJASS_KEYWORDS};
use parser_rs::{EmbedSpec, LexSpec, NumberSpec, ParseError, Punct};

/// JASS's lexical rules for the shared [`parser_rs::lexer`]: `//` comments
/// only, rawcodes, and JASS's small operator set.
pub const SPEC: LexSpec = LexSpec {
    keywords: JASS_KEYWORDS,
    // JASS reals may start or end with the dot: `.5`, `2144.`; hex may
    // be `$1F`; a leading 0 is octal: `017`.
    numbers: NumberSpec {
        leading_dot: true,
        trailing_dot: true,
        leading_zero_octal: true,
        dollar_hex: true,
        ..LexSpec::BASE.numbers
    },
    line_comment: "//",
    block_comment: None,
    rawcodes: true,
    puncts: JASS_PUNCTS,
    ..LexSpec::BASE
};

const JASS_PUNCTS: &[Punct] = &[
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
];

/// vJASS's: JASS's and `.` (members).
const VJASS_PUNCTS: &[Punct] = &[
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
    Punct::Dot,
    // `a:X`, JassHelper's `X[a]`.
    Punct::Colon,
];

/// A `//! zinc` ... `//! endzinc` block: Zinc embedded in vJASS. The lexer
/// turns each block into one `TokenKind::Embedded` token, and the parser
/// hands it to `zinc-rs` (see `TopLevel::Zinc`).
pub const ZINC_EMBED: EmbedSpec = EmbedSpec {
    lang: "zinc",
    open: "//! zinc",
    close: "//! endzinc",
    prefix: false,
};

/// A `//! textmacro NAME [takes a, b]` (or `textmacro_once`) ...
/// `//! endtextmacro` block: text JassHelper's preprocessor puts where a
/// `//! runtextmacro` names it (a build does: `wc3-build`'s `preprocess`),
/// not code of its own place. One token, skipped by the parser.
pub const TEXTMACRO_EMBED: EmbedSpec = EmbedSpec {
    lang: "textmacro",
    open: "//! textmacro",
    close: "//! endtextmacro",
    prefix: true,
};

/// A `//! novjass` ... `//! endnovjass` block: what JassHelper leaves out
/// (often a library's documentation). One token, skipped by the parser.
pub const NOVJASS_EMBED: EmbedSpec = EmbedSpec {
    lang: "novjass",
    open: "//! novjass",
    close: "//! endnovjass",
    prefix: false,
};

/// The vJASS dialect: JASS plus vJASS keywords, `/* */` comments and
/// embedded Zinc blocks. Same tokens, same parser; only the active
/// vocabulary differs. vJASS's own constructs (`library`, `struct`, ...)
/// aren't parsed yet.
pub const VJASS_SPEC: LexSpec = LexSpec {
    keywords: VJASS_KEYWORDS,
    block_comment: Some(("/*", "*/")),
    // JassHelper's: `/* a /* b */ c */` is one comment.
    nested_block_comments: true,
    embeds: &[ZINC_EMBED, TEXTMACRO_EMBED, NOVJASS_EMBED],
    puncts: VJASS_PUNCTS,
    ..SPEC
};

/// Which JASS dialect to read; it picks the [`LexSpec`].
///
/// Chosen **per file**, so one project can mix plain JASS, vJASS (with
/// embedded Zinc), and later other dialects. A file is plain JASS unless
/// its header says otherwise (see [`Dialect::detect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// Plain JASS: `//! zinc` is just a comment, `library` is a name.
    #[default]
    Jass,
    /// vJASS (JassHelper), including embedded `//! zinc` blocks.
    VJass,
}

impl Dialect {
    /// The header key: `// @dialect <name>`.
    pub const DIRECTIVE: &'static str = "dialect";

    /// The dialect a file declares in its header (`// @dialect vjass`; see
    /// [`parser_rs::directive`]). Plain JASS if there's no declaration or
    /// the name is unknown: the base language is the default, and a file
    /// opts *into* extensions.
    pub fn detect(src: &str) -> Dialect {
        Dialect::declared(src).unwrap_or_default()
    }

    /// The dialect a file's header declares, if it names a known one.
    /// `None` lets the caller choose the fallback, such as the project
    /// default from `wc3.bni` in the editor.
    pub fn declared(src: &str) -> Option<Dialect> {
        parser_rs::header_directive(src, SPEC.line_comment, Dialect::DIRECTIVE)
            .and_then(Dialect::from_name)
    }

    /// The dialect named `name` (case-insensitive): `jass` or `vjass`.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name.to_ascii_lowercase().as_str() {
            "jass" => Some(Dialect::Jass),
            "vjass" => Some(Dialect::VJass),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Dialect::Jass => "jass",
            Dialect::VJass => "vjass",
        }
    }

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
