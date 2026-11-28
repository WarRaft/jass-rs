use crate::token::{JASS_KEYWORDS, Token, VJASS_KEYWORDS};
use parser_rs::{EmbedSpec, LexSpec, NumberSpec, ParseError, Punct};

/// Extended JASS's lexical rules for the shared [`parser_rs::lexer`]: `//`
/// comments only, rawcodes, JASS's small operator set and the extended
/// one's (`!`, `&&`, `||`, `+=`, `-=`; see [`Dialect::Jass`]).
pub const SPEC: LexSpec = LexSpec {
    puncts: EXTENDED_PUNCTS,
    ..STRICT_SPEC
};

/// Strict JASS's: exactly the game's tokens (see [`Dialect::Strict`]).
pub const STRICT_SPEC: LexSpec = LexSpec {
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

/// Extended JASS's: the game's, and `!` (`not`), `&&` (`and`), `||`
/// (`or`), `+=` and `-=` (in `set`). No program of the game's reads
/// otherwise with them.
const EXTENDED_PUNCTS: &[Punct] = &[
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
    Punct::Bang,
    Punct::AndAnd,
    Punct::OrOr,
    Punct::PlusAssign,
    Punct::MinusAssign,
];

/// vJASS's: extended JASS's and `.` (members).
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
    Punct::Bang,
    Punct::AndAnd,
    Punct::OrOr,
    Punct::PlusAssign,
    Punct::MinusAssign,
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

/// An `//! inject main` (`config`) ... `//! endinject` block: the lines
/// JassHelper puts in place of the map's `main` (`config`); the build reads
/// them as a function (`wc3-build`'s `preprocess`). One token, skipped by
/// the parser, as a statement of it isn't one outside a function.
pub const INJECT_EMBED: EmbedSpec = EmbedSpec {
    lang: "inject",
    open: "//! inject",
    close: "//! endinject",
    prefix: true,
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
    embeds: &[ZINC_EMBED, TEXTMACRO_EMBED, NOVJASS_EMBED, INJECT_EMBED],
    puncts: VJASS_PUNCTS,
    ..SPEC
};

/// Which JASS dialect to read; it picks the [`LexSpec`].
///
/// Chosen **per file**, so one project can mix plain JASS, vJASS (with
/// embedded Zinc), and later other dialects. A file is (extended) JASS
/// unless its header says otherwise (see [`Dialect::detect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// Extended JASS, the default: plain JASS (`//! zinc` is just a
    /// comment, `library` is a name) and what no program of the game's can
    /// mean otherwise: `!` for `not`, `&&` for `and`, `||` for `or`,
    /// `set x += e` and `set x -= e`. A build writes them as the game's.
    #[default]
    Jass,
    /// Strict JASS: exactly what the game reads, for checking compiled
    /// code (a built `war3map.j`, the game's own scripts).
    Strict,
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

    /// The dialect named `name` (case-insensitive): `jass`, `strict` or
    /// `vjass`.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name.to_ascii_lowercase().as_str() {
            "jass" => Some(Dialect::Jass),
            "strict" => Some(Dialect::Strict),
            "vjass" => Some(Dialect::VJass),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Dialect::Jass => "jass",
            Dialect::Strict => "strict",
            Dialect::VJass => "vjass",
        }
    }

    pub fn spec(self) -> &'static LexSpec {
        match self {
            Dialect::Jass => &SPEC,
            Dialect::Strict => &STRICT_SPEC,
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
