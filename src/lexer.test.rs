use super::*;
use crate::token::{TokenKind, Kw, Punct};

fn kinds(src: &str) -> Vec<TokenKind> {
    Lexer::tokenize(src)
        .unwrap()
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

#[test]
fn tokenizes_keywords() {
    assert_eq!(
        kinds("function endfunction takes returns nothing"),
        vec![
            TokenKind::Keyword(Kw::Function),
            TokenKind::Keyword(Kw::EndFunction),
            TokenKind::Keyword(Kw::Takes),
            TokenKind::Keyword(Kw::Returns),
            TokenKind::Keyword(Kw::Nothing),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn tokenizes_identifiers_and_numbers() {
    assert_eq!(
        kinds("foo 123 4.25 0x1F"),
        vec![
            TokenKind::Ident("foo".to_string()),
            TokenKind::Int(123),
            TokenKind::Real(4.25),
            TokenKind::Int(31),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn tokenizes_string_literal_with_escapes() {
    assert_eq!(
        kinds(r#""hello\nworld""#),
        vec![
            TokenKind::Str("hello\nworld".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn tokenizes_rawcode_literal() {
    assert_eq!(
        kinds("'hfoo'"),
        vec![TokenKind::Int(0x68666f6f), TokenKind::Eof,]
    );
}

#[test]
fn tokenizes_operators() {
    assert_eq!(
        kinds("== != >= <= = > <"),
        vec![
            TokenKind::Punct(Punct::EqEq),
            TokenKind::Punct(Punct::NotEq),
            TokenKind::Punct(Punct::GtEq),
            TokenKind::Punct(Punct::LtEq),
            TokenKind::Punct(Punct::Assign),
            TokenKind::Punct(Punct::Gt),
            TokenKind::Punct(Punct::Lt),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn skips_line_comments() {
    assert_eq!(
        kinds("set x = 1 // comment\nset y = 2"),
        vec![
            TokenKind::Keyword(Kw::Set),
            TokenKind::Ident("x".to_string()),
            TokenKind::Punct(Punct::Assign),
            TokenKind::Int(1),
            TokenKind::Keyword(Kw::Set),
            TokenKind::Ident("y".to_string()),
            TokenKind::Punct(Punct::Assign),
            TokenKind::Int(2),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn reports_unterminated_string() {
    let err = Lexer::tokenize("\"abc").unwrap_err();
    assert!(err.message.contains("unterminated string"));
}

#[test]
fn reports_invalid_rawcode_length() {
    let err = Lexer::tokenize("'ab'").unwrap_err();
    assert!(err.message.contains("4 characters"));
}

#[test]
fn vjass_keywords_are_identifiers_in_plain_jass() {
    let jass = Lexer::tokenize("library").unwrap();
    assert_eq!(jass[0].kind, TokenKind::Ident("library".to_string()));

    let vjass = parser_rs::tokenize("library /* c */", &VJASS_SPEC).unwrap();
    assert_eq!(vjass[0].kind, TokenKind::Keyword(Kw::Library));
    assert_eq!(vjass[1].kind, TokenKind::Eof);
}
