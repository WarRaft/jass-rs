//! vJASS `//! zinc` blocks: the JASS parser hands each block to zinc-rs and
//! takes over again after `//! endzinc`.

use jass_rs::ast::TopLevel;
use jass_rs::{find_node_at, lint, parse, parse_file, parse_with, AstRef, Dialect};

const SRC: &str = include_str!("fixtures/zinc_block.j");

#[test]
fn control_passes_to_zinc_and_back() {
    let program = parse_with(SRC, Dialect::VJass).expect("should parse");
    let kinds: Vec<&str> = program
        .items
        .iter()
        .map(|item| match item {
            TopLevel::Globals(_) => "globals",
            TopLevel::Zinc(_) => "zinc",
            TopLevel::Function(_) => "function",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["globals", "zinc", "function"]);

    let TopLevel::Zinc(block) = &program.items[1] else {
        unreachable!()
    };
    assert_eq!(block.open.start_line, 6);
    assert_eq!(block.close.map(|c| c.start_line), Some(14));
    let lib = &block.program.libraries[0];
    assert_eq!(lib.name, "Counter");
    assert_eq!(lib.line, 7); // file-absolute, not block-relative
}

#[test]
fn the_file_header_selects_the_dialect() {
    assert_eq!(Dialect::detect(SRC), Dialect::VJass); // `// @dialect vjass`
    assert_eq!(Dialect::detect("globals\nendglobals"), Dialect::Jass); // default
    assert_eq!(Dialect::detect("// @dialect klingon\n"), Dialect::Jass); // unknown
    assert_eq!(Dialect::detect("// @dialect VJASS\n"), Dialect::VJass);
    assert!(parse_file(SRC).is_ok());
    // Without the header, the same code is plain JASS and the Zinc block
    // doesn't parse.
    let body = SRC.strip_prefix("// @dialect vjass\n").unwrap();
    assert!(parse_file(body).is_err());
}

#[test]
fn plain_jass_treats_the_block_as_comments_plus_jass() {
    // Without vJASS, `//! zinc` is a comment and the Zinc code is read as
    // JASS, which it isn't.
    assert!(parse(SRC).is_err());
}

#[test]
fn zinc_errors_carry_file_absolute_positions() {
    let src = "//! zinc\nlibrary L {\n    function F() { x = ; }\n}\n//! endzinc\n";
    let err = parse_with(src, Dialect::VJass).unwrap_err();
    assert_eq!((err.line, err.col), (3, 24));
}

#[test]
fn unterminated_blocks_are_reported_at_the_directive() {
    let err = parse_with("//! zinc\nlibrary L {}\n", Dialect::VJass).unwrap_err();
    assert!(err.message.contains("missing '//! endzinc'"));
    assert_eq!(err.line, 1);
}

#[test]
fn zinc_blocks_are_rejected_inside_functions() {
    let src = "function F takes nothing returns nothing\n//! zinc\n//! endzinc\nendfunction";
    let err = parse_with(src, Dialect::VJass).unwrap_err();
    assert!(err.message.contains("embedded zinc block"), "{}", err.message);
}

#[test]
fn lint_includes_zinc_diagnostics_at_file_lines() {
    let src = "//! zinc\nlibrary L {\n    function F() {\n        integer unused;\n    }\n}\n//! endzinc\n";
    let program = parse_with(src, Dialect::VJass).unwrap();
    let diags = lint(&program);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].line, 4);
    assert!(diags[0].message.contains("unused local variable 'unused'"));
}

#[test]
fn queries_reach_into_zinc_blocks() {
    let program = parse_with(SRC, Dialect::VJass).unwrap();
    // line 10: `        gCount += by;`, column 19 is `by`
    match find_node_at(&program, 10, 19) {
        Some(AstRef::Zinc(zinc_rs::AstRef::Expr(e))) => {
            assert_eq!(e.kind, zinc_rs::ast::ExprKind::Var("by".into()));
        }
        other => panic!("expected a Zinc expr, got {other:?}"),
    }
    // the JASS function after the block is still found
    assert!(matches!(
        find_node_at(&program, 17, 5),
        Some(AstRef::Stmt(_))
    ));
}

#[test]
fn semantic_tokens_delegate_to_zinc() {
    use parser_rs::SemanticType as T;
    let toks = jass_rs::semantic_tokens_with(SRC, Dialect::VJass);
    let at = |line, col| toks.iter().find(|t| t.line == line && t.start_col == col).map(|t| t.ty);
    assert_eq!(at(6, 1), Some(T::Macro)); // //! zinc
    assert_eq!(at(14, 1), Some(T::Macro)); // //! endzinc
    assert_eq!(at(7, 9), Some(T::Namespace)); // Counter
    assert_eq!(at(8, 5), Some(T::Comment)); // /* counts up */
    assert_eq!(at(9, 21), Some(T::Function)); // Bump
    assert_eq!(at(17, 10), Some(T::Function)); // call Bump(1), back in JASS
}
