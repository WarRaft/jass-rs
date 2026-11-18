//! JASS's token vocabulary: which of the shared [`Kw`]s are active, on top
//! of the shared [`parser_rs::TokenKind`] (identifiers, literals, [`Punct`]).
//!
//! vJASS is a superset dialect: [`VJASS_KEYWORDS`] adds its keywords to
//! [`JASS_KEYWORDS`]. The lexer decides which set applies, so in plain JASS
//! `library` stays an ordinary identifier and one parser serves both.

pub use parser_rs::{Kw, KwSet, Punct};

/// Plain JASS keywords (`common.j` / `war3map.j`).
pub const JASS_KEYWORDS: KwSet = KwSet::of(&[
    Kw::Function,
    Kw::EndFunction,
    Kw::Takes,
    Kw::Returns,
    Kw::Nothing,
    Kw::Native,
    Kw::Globals,
    Kw::EndGlobals,
    Kw::Constant,
    Kw::Local,
    Kw::Set,
    Kw::Call,
    Kw::If,
    Kw::Then,
    Kw::ElseIf,
    Kw::Else,
    Kw::EndIf,
    Kw::Loop,
    Kw::EndLoop,
    Kw::ExitWhen,
    Kw::Return,
    Kw::Debug,
    Kw::Type,
    Kw::Extends,
    Kw::Array,
    Kw::And,
    Kw::Or,
    Kw::Not,
    Kw::True,
    Kw::False,
    Kw::Null,
]);

/// vJASS = JASS plus these (JassHelper). Not parsed yet; see the README's
/// parser status.
pub const VJASS_KEYWORDS: KwSet = JASS_KEYWORDS.union(KwSet::of(&[
    Kw::Library,
    Kw::LibraryOnce,
    Kw::EndLibrary,
    Kw::Scope,
    Kw::EndScope,
    Kw::Requires,
    Kw::Needs,
    Kw::Uses,
    Kw::Initializer,
    Kw::Optional,
    Kw::Private,
    Kw::Public,
    Kw::Static,
    Kw::Struct,
    Kw::EndStruct,
    Kw::Interface,
    Kw::EndInterface,
    Kw::Method,
    Kw::EndMethod,
    Kw::Module,
    Kw::EndModule,
    Kw::Implement,
    Kw::Operator,
    Kw::Stub,
    Kw::Delegate,
    Kw::Readonly,
    Kw::This,
    Kw::ThisType,
    Kw::Super,
    Kw::Hook,
    Kw::Defaults,
    Kw::Keyword,
]));

/// A JASS token kind (the shared one).
pub type TokenKind = parser_rs::TokenKind;

/// A JASS token.
pub type Token = parser_rs::Token<TokenKind>;
