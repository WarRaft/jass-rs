use super::*;
use parser_rs::SemanticModifier as M;
use parser_rs::{Scope, SymbolKind as K};

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
    // (A string left open runs to the end of the file in JASS, a line
    // break being part of it: an operator without its right side here.)
    let src = "function F takes nothing returns nothing\n    set x = 1 *\n    call G(\nendfunction";
    assert!(crate::parse(src).is_err());
    let toks = semantic_tokens(src);
    assert!(toks.iter().any(|t| t.line == 3 && t.ty == T::Function)); // G(
    assert!(toks.iter().any(|t| t.line == 4 && t.ty == T::Keyword)); // endfunction
}

#[test]
fn encodes_for_the_editor() {
    let data = encoded_semantic_tokens("set x = 1", Dialect::Jass);
    // set (keyword), x (variable+modification), = (operator), 1 (number)
    assert_eq!(data.len(), 4 * 5);
    assert_eq!(&data[..5], &[0, 0, 3, T::Keyword.index(), 0]);
    assert_eq!(&data[5..10], &[0, 4, 1, T::Variable.index(), M::Modification.bit()]);
}

#[test]
fn classifies_top_level_globals() {
    let src = "constant integer LIMIT = 5\nunit hero\nfunction F takes nothing returns nothing\n    set hero = null\n    call F(LIMIT)\nendfunction";
    let toks = semantic_tokens(src);
    let at = |line, col| toks.iter().find(|t| t.line == line && t.start_col == col).map(|t| (t.ty, t.mods));
    assert_eq!(at(1, 18), Some((T::Variable, m(&[M::Declaration, M::Readonly]))));
    assert_eq!(at(2, 1), Some((T::Type, m(&[]))));
    assert_eq!(at(2, 6), Some((T::Variable, m(&[M::Declaration]))));
    assert_eq!(at(4, 9), Some((T::Variable, m(&[M::Modification]))));
    assert_eq!(at(5, 12), Some((T::Variable, m(&[M::Readonly]))));
}

/// Every occurrence of `name` as (line, kind, declaration, scope).
fn occurrences(name: &str) -> Vec<(usize, K, bool, Scope)> {
    symbols(SRC, Dialect::Jass)
        .into_iter()
        .filter(|o| o.name == name)
        .map(|o| (o.line, o.kind, o.declaration, o.scope))
        .collect()
}

#[test]
fn records_project_wide_declarations_and_references() {
    let p = Scope::Project;
    assert_eq!(occurrences("MAX"), vec![(2, K::Constant, true, p), (8, K::Constant, false, p)]);
    assert_eq!(occurrences("units"), vec![(3, K::Global, true, p)]);
    assert_eq!(occurrences("mytype"), vec![(5, K::Type, true, p), (7, K::Type, false, p)]);
    assert_eq!(occurrences("GetUnit"), vec![(6, K::Native, true, p), (9, K::Native, false, p)]);
    assert_eq!(occurrences("Sum"), vec![(7, K::Function, true, p), (10, K::Function, false, p)]);
    // Not declared here, still a project-wide reference.
    assert_eq!(occurrences("ForGroup"), vec![(10, K::Function, false, p)]);
}

#[test]
fn scopes_parameters_and_locals_to_their_signature() {
    // `GetUnit` is signature 1, `Sum` signature 2.
    assert_eq!(occurrences("id"), vec![(6, K::Parameter, true, Scope::Local(1))]);
    let n = Scope::Local(2);
    assert_eq!(occurrences("n"), vec![(7, K::Parameter, true, n), (9, K::Parameter, false, n)]);
    assert_eq!(
        occurrences("i"),
        vec![(8, K::Local, true, n), (9, K::Local, false, n), (9, K::Local, false, n), (11, K::Local, false, n)]
    );
}

#[test]
fn primitive_types_are_not_symbols() {
    assert!(occurrences("integer").is_empty());
    assert!(occurrences("handle").is_empty());
}

#[test]
fn symbols_carry_exact_columns() {
    let syms = symbols(SRC, Dialect::Jass);
    let sum = syms.iter().find(|o| o.name == "Sum" && o.declaration).unwrap();
    assert_eq!((sum.line, sum.col, sum.end_col), (7, 10, 13));
    assert!(sum.covers(7, 13) && !sum.covers(7, 14));
}

#[test]
fn half_typed_code_is_still_indexed() {
    let syms = symbols("function F takes integer x returns\n    set x = Foo(", Dialect::Jass);
    let names: Vec<_> = syms.iter().map(|o| (o.name.as_str(), o.declaration)).collect();
    assert_eq!(names, vec![("F", true), ("x", true), ("x", false), ("Foo", false)]);
}

#[test]
fn embedded_zinc_is_indexed_with_its_own_scopes() {
    let src = "\
// @dialect vjass
function A takes integer x returns nothing
endfunction
//! zinc
library L { function F(integer x) { } }
//! endzinc
function B takes integer x returns nothing
endfunction
";
    let xs: Vec<_> = symbols(src, Dialect::VJass)
        .into_iter()
        .filter(|o| o.name == "x")
        .map(|o| (o.line, o.scope))
        .collect();
    // A is 1; the library 2 and F 3 inside the block; B goes on at 4.
    assert_eq!(xs, vec![(2, Scope::Local(1)), (5, Scope::Local(3)), (7, Scope::Local(4))]);
    let l = symbols(src, Dialect::VJass).into_iter().find(|o| o.name == "L").unwrap();
    assert_eq!((l.kind, l.scope), (K::Library, Scope::Project));
}

#[test]
fn functions_without_takes_or_returns_are_indexed() {
    let src = "function F\n    local integer x = 1\n    call G(x)\nendfunction\nfunction G takes integer a\n    set a = a + 1\nendfunction\n";
    let syms: Vec<_> = symbols(src, Dialect::Jass)
        .into_iter()
        .map(|o| (o.name, o.line, o.kind, o.declaration, o.scope))
        .collect();
    let (f, g) = (Scope::Local(1), Scope::Local(2));
    let p = Scope::Project;
    assert_eq!(
        syms,
        vec![
            ("F".to_string(), 1, K::Function, true, p),
            ("x".to_string(), 2, K::Local, true, f),
            ("G".to_string(), 3, K::Function, false, p),
            ("x".to_string(), 3, K::Local, false, f),
            ("G".to_string(), 5, K::Function, true, p),
            ("a".to_string(), 5, K::Parameter, true, g),
            ("a".to_string(), 6, K::Parameter, false, g),
            ("a".to_string(), 6, K::Parameter, false, g),
        ]
    );
}

#[test]
fn set_and_call_left_out_after_locals() {
    // `call` left out: a name after a local's declaration starts its own
    // line, and isn't one more of that declaration.
    let src = "native KillUnit takes unit u returns nothing\nfunction F\n    local unit u\n    KillUnit(u)\n    KillUnit(u)\n    u = null\nendfunction\n";
    let toks = semantic_tokens(src);
    let at = |line: usize, col: usize| toks.iter().find(|t| t.line == line && t.start_col == col).map(|t| (t.ty, t.mods)).unwrap();
    assert_eq!(at(3, 16), (T::Variable, m(&[M::Declaration])));
    assert_eq!(at(4, 5), (T::Function, m(&[M::DefaultLibrary])));
    assert_eq!(at(5, 5), (T::Function, m(&[M::DefaultLibrary])));
    // `set` left out: a write.
    assert_eq!(at(6, 5), (T::Variable, m(&[M::Modification])));
    // And the local is the only one declared: KillUnit isn't a local.
    let mut syms = Vec::new();
    let mut out = Vec::new();
    let lexed = crate::lexer::Lexer::tokenize(src).unwrap();
    Classifier::new(&lexed).run(&mut out, &mut syms);
    assert!(!syms.iter().any(|o| o.name == "KillUnit" && o.kind == K::Local), "{syms:?}");
}

#[test]
fn a_local_without_local() {
    let src = "integer g = 0\nfunction F\n    integer i = g\n    i = i + 1\nendfunction\n";
    let toks = semantic_tokens(src);
    let at = |line: usize, col: usize| toks.iter().find(|t| t.line == line && t.start_col == col).map(|t| (t.ty, t.mods)).unwrap();
    assert_eq!(at(3, 5), (T::Type, m(&[M::DefaultLibrary])));
    assert_eq!(at(3, 13), (T::Variable, m(&[M::Declaration])));
    let mut syms = Vec::new();
    let mut out = Vec::new();
    let lexed = crate::lexer::Lexer::tokenize(src).unwrap();
    Classifier::new(&lexed).run(&mut out, &mut syms);
    assert!(syms.iter().any(|o| o.name == "i" && o.kind == K::Local && o.declaration), "{syms:?}");
    assert!(syms.iter().any(|o| o.name == "g" && o.kind == K::Global && o.declaration), "{syms:?}");
}
