pub mod ast;
pub mod html;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod printer;
pub mod query;
pub mod semantic;
pub mod token;

pub use ast::{ColumnEncoding, Program, Span};
pub use html::render_html;
pub use lint::{lint, Diagnostic, Level};
pub use parser::{ParseError, Parser};
pub use printer::{render_dot, render_tree};
pub use query::{find_node_at, find_node_in, AstRef};
pub use semantic::{encoded_semantic_tokens, semantic_tokens};

/// Parses JASS source into an AST.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    Parser::parse_str(src)
}
