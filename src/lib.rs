pub mod array_struct;
pub mod ast;
pub mod html;
pub mod lexer;
pub mod lint;
pub mod names;
pub mod parser;
pub mod printer;
pub mod query;
pub mod semantic;
pub mod strict;
pub mod token;

pub use ast::{ColumnEncoding, Program, Span};
pub use lexer::Dialect;
pub use html::render_html;
pub use lint::{lint, Diagnostic, Level};
pub use parser::{ParseError, Parser};
pub use printer::{render_dot, render_tree};
pub use query::{find_node_at, find_node_in, AstRef};
pub use semantic::{
    encoded_semantic_tokens, semantic_tokens, semantic_tokens_and_symbols, semantic_tokens_with, symbols,
};

/// Parses strict JASS, exactly what the game reads (a built `war3map.j`,
/// `common.j`, `Blizzard.j`), into an AST: [`Dialect::Strict`].
pub fn parse(src: &str) -> Result<Program, ParseError> {
    Parser::parse_str_with(src, Dialect::Strict)
}

/// Parses source in the dialect its header declares (`// @dialect vjass`),
/// extended JASS by default. This is what tools should use for real files.
pub fn parse_file(src: &str) -> Result<Program, ParseError> {
    Parser::parse_str_with(src, Dialect::detect(src))
}

/// Parses source in the given dialect. In [`Dialect::VJass`], `//! zinc`
/// blocks are handed to `zinc-rs` and come back as [`ast::TopLevel::Zinc`].
pub fn parse_with(src: &str, dialect: Dialect) -> Result<Program, ParseError> {
    Parser::parse_str_with(src, dialect)
}
