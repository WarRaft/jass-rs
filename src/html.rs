use crate::ast::Program;
use crate::printer::build_tree;
use crate::token::{Keyword, Kw};
use parser_rs::HtmlLang;

/// JASS's flavor of the shared viewer: its keyword set, `//` comments only.
pub const HTML_LANG: HtmlLang = HtmlLang {
    is_keyword: |word| Kw::from_word(word).is_some(),
    block_comments: false,
};

/// Renders a self-contained HTML page that visualizes a parsed program's
/// AST next to its highlighted source — see [`parser_rs::html`] for what
/// the viewer does; this only supplies the JASS-specific bits.
pub fn render_html(program: &Program, source: &str, title: &str) -> String {
    parser_rs::render_html(&build_tree(program), source, title, &HTML_LANG)
}

#[cfg(test)]
#[path = "html.test.rs"]
mod tests;
