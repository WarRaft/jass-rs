use super::*;
use parser_rs::SemanticModifier as M;

const SRC: &str = "\
globals
    constant integer MAX = 10 // limit
    unit array units
endglobals
type mytype extends handle
native GetUnit takes integer id returns unit
function Sum takes integer n, mytype h returns integer
    local integer i = MAX
    set i = n + GetUnit(i)
    call ForGroup(g, function Sum)
    return i
endfunction
";

/// The token covering the `nth` (0-based) occurrence of `word` on `line`.
fn find(toks: &[SemanticToken], line: usize, word: &str, nth: usize) -> (T, Modifiers) {
    let text = SRC.lines().nth(line - 1).unwrap();
    let col = text
        .match_indices(word)
        .filter(|(i, _)| {
            let before = text[..*i].chars().last();
            let after = text[i + word.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .nth(nth)
        .map(|(i, _)| text[..i].chars().count() + 1)
        .unwrap_or_else(|| panic!("{word:?} not on line {line}"));
    let t = toks
        .iter()
        .find(|t| t.line == line && t.start_col == col)
        .unwrap_or_else(|| panic!("no token for {word:?} at {line}:{col}"));
    (t.ty, t.mods)
}

fn m(mods: &[M]) -> Modifiers {
    Modifiers::of(mods)
}

#[test]
fn classifies_declarations() {
    let toks = semantic_tokens(SRC);
    assert_eq!(find(&toks, 2, "constant", 0), (T::Keyword, m(&[])));
    assert_eq!(find(&toks, 2, "integer", 0), (T::Type, m(&[M::DefaultLibrary])));
    assert_eq!(find(&toks, 2, "MAX", 0), (T::Variable, m(&[M::Declaration, M::Readonly])));
    assert_eq!(find(&toks, 3, "unit", 0), (T::Type, m(&[])));
    assert_eq!(find(&toks, 3, "units", 0), (T::Variable, m(&[M::Declaration])));
    assert_eq!(find(&toks, 5, "mytype", 0), (T::Type, m(&[M::Declaration])));
    assert_eq!(find(&toks, 5, "handle", 0), (T::Type, m(&[M::DefaultLibrary])));
    assert_eq!(
        find(&toks, 6, "GetUnit", 0),
        (T::Function, m(&[M::Declaration, M::DefaultLibrary]))
    );
    assert_eq!(find(&toks, 6, "id", 0), (T::Parameter, m(&[M::Declaration])));
    assert_eq!(find(&toks, 7, "Sum", 0), (T::Function, m(&[M::Declaration])));
    assert_eq!(find(&toks, 7, "n", 0), (T::Parameter, m(&[M::Declaration])));
    assert_eq!(find(&toks, 7, "mytype", 0), (T::Type, m(&[])));
    assert_eq!(find(&toks, 8, "i", 0), (T::Variable, m(&[M::Declaration])));
}

#[test]
fn classifies_references() {
    let toks = semantic_tokens(SRC);
    assert_eq!(find(&toks, 8, "MAX", 0), (T::Variable, m(&[M::Readonly])));
    assert_eq!(find(&toks, 9, "i", 0), (T::Variable, m(&[M::Modification])));
    assert_eq!(find(&toks, 9, "n", 0), (T::Parameter, m(&[])));
    assert_eq!(find(&toks, 9, "GetUnit", 0), (T::Function, m(&[M::DefaultLibrary])));
    assert_eq!(find(&toks, 9, "i", 1), (T::Variable, m(&[])));
    assert_eq!(find(&toks, 10, "ForGroup", 0), (T::Function, m(&[])));
    assert_eq!(find(&toks, 10, "function", 0), (T::Keyword, m(&[])));
    assert_eq!(find(&toks, 10, "Sum", 0), (T::Function, m(&[])));
}

#[test]
fn includes_lexical_tokens_and_comments() {
    let toks = semantic_tokens(SRC);
    assert_eq!(find(&toks, 2, "10", 0).0, T::Number);
    assert_eq!(find(&toks, 2, "//", 0).0, T::Comment);
    assert_eq!(find(&toks, 9, "+", 0).0, T::Operator);
    assert_eq!(find(&toks, 2, "=", 0).0, T::Operator);
}

#[test]
fn still_highlights_unparsable_code() {
    let src = "function F takes nothing returns nothing\n    set x = \"unterminated\n    call G(\nendfunction";
    assert!(crate::parse(src).is_err());
    let toks = semantic_tokens(src);
    assert!(toks.iter().any(|t| t.line == 3 && t.ty == T::Function)); // G(
    assert!(toks.iter().any(|t| t.line == 4 && t.ty == T::Keyword)); // endfunction
}

#[test]
fn encodes_for_the_editor() {
    let data = encoded_semantic_tokens("set x = 1");
    // set (keyword), x (variable+modification), = (operator), 1 (number)
    assert_eq!(data.len(), 4 * 5);
    assert_eq!(&data[..5], &[0, 0, 3, T::Keyword.index(), 0]);
    assert_eq!(&data[5..10], &[0, 4, 1, T::Variable.index(), M::Modification.bit()]);
}
