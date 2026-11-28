use crate::ast::Program;
use crate::lexer::{Dialect, SPEC, VJASS_SPEC};
use crate::printer::build_tree;
use parser_rs::HtmlLang;

/// The viewer for plain JASS: highlighting by JASS's own lexer.
pub static JASS_HTML: HtmlLang = HtmlLang {
    spec: &SPEC,
    embedded: &[],
};

/// The viewer for vJASS: `//! zinc` blocks are highlighted by Zinc's lexer.
pub static VJASS_HTML: HtmlLang = HtmlLang {
    spec: &VJASS_SPEC,
    embedded: &[("zinc", &zinc_rs::lexer::SPEC)],
};

/// Renders a self-contained HTML page that visualizes a parsed program's
/// AST next to its highlighted source — see [`parser_rs::html`] for what
/// the viewer does; this only supplies the JASS-specific bits.
pub fn render_html(program: &Program, source: &str, title: &str) -> String {
    let lang = match Dialect::detect(source) {
        Dialect::Jass | Dialect::Strict => &JASS_HTML,
        Dialect::VJass => &VJASS_HTML,
    };
    parser_rs::render_html(&build_tree(program), source, title, lang)
}

#[cfg(test)]
#[path = "html.test.rs"]
mod tests;
