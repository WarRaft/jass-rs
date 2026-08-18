pub mod ast;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod token;

pub use ast::Program;
pub use lint::{lint, Diagnostic, Level};
pub use parser::{ParseError, Parser};

/// Parses JASS source into an AST.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    Parser::parse_str(src)
}
