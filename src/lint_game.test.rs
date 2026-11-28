use super::*;

#[test]
fn source_finds_text_by_line_and_column() {
    let src = Source::new("set a = 'Ab'\r\nset b = 1.5\nx\n", false);
    assert_eq!(src.text_of(Span::new(1, 9, 1, 13)), Some("'Ab'"));
    assert_eq!(src.text_at(Span::new(2, 9, 2, 12)), Some("1.5"));
    // To the end of the line, without its line break.
    assert_eq!(src.text_at(Span::new(1, 9, 1, 99)), Some("'Ab'"));
    // Several lines: text_of only.
    assert_eq!(src.text_of(Span::new(2, 9, 3, 2)), Some("1.5\nx"));
    assert_eq!(src.text_at(Span::new(2, 9, 3, 2)), None);
    // No such line.
    assert_eq!(src.text_at(Span::new(4, 1, 4, 2)), None);
    assert_eq!(src.text_of(Span::new(9, 1, 9, 2)), None);
}

#[test]
fn source_counts_columns_in_characters() {
    let src = Source::new("// ё\nset s = 'ё'\n", false);
    assert_eq!(src.text_of(Span::new(2, 9, 2, 12)), Some("'ё'"));
    assert_eq!(src.text_at(Span::new(2, 9, 2, 12)), Some("'ё'"));
}

#[test]
fn strict_source_breaks_lines_at_a_lone_carriage_return() {
    // As the parser reads strict JASS: a lone `\r` in code ends a line,
    // not in a comment or a string.
    let text = "globals\rreal r = 1.5 // a\rb\rendglobals\r";
    let src = Source::new(text, true);
    assert_eq!(src.text_at(Span::new(2, 10, 2, 13)), Some("1.5"));
    assert_eq!(Source::new(text, false).text_at(Span::new(2, 10, 2, 13)), None);
}

/// A map's script with lone carriage returns (protected maps have them):
/// its numbers read where they are, no panic.
#[test]
fn strict_lint_reads_numbers_after_lone_carriage_returns() {
    let text = "globals\rinteger a = 0\rendglobals\rfunction f takes nothing returns nothing\rcall SetPlayerColor(Player(9), ConvertPlayerColor(9))\rset a = 99999999999\rcall TriggerSleepAction(1.0)\rendfunction\r";
    let program = crate::parse_with(text, crate::Dialect::Strict).unwrap();
    let options = Options { source: Some(text), strict: true, ..Default::default() };
    let diags = crate::lint_with(&program, &options);
    let overflow: Vec<_> = diags.iter().filter(|d| d.rule == crate::lint::rule::NUMBER_OVERFLOW).map(|d| (d.line, d.message.clone())).collect();
    assert_eq!(overflow.len(), 1, "{overflow:?}");
    assert_eq!(overflow[0].0, 6);
    assert!(overflow[0].1.starts_with("99999999999 "), "{overflow:?}");
}

#[test]
fn real_fits_takes_any_text() {
    assert!(real_fits("1.5"));
    assert!(!real_fits("99999999999.0"));
    assert!(real_fits("\rc"));
    assert!(real_fits("ll"));
}
