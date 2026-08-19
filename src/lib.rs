pub mod ast;
pub mod html;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod printer;
pub mod token;

pub use ast::Program;
pub use html::render_html;
pub use lint::{lint, Diagnostic, Level};
pub use parser::{ParseError, Parser};
pub use printer::{render_dot, render_tree};

/// Parses JASS source into an AST.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    Parser::parse_str(src)
}
