use super::*;

const COMMON: &str = "type unit extends handle\nnative KillUnit takes unit u returns nothing\n";

fn env() -> Declarations {
    Declarations::of(&crate::parse(COMMON).unwrap())
}

fn said(found: &[Found]) -> Vec<(usize, usize, &'static str)> {
    found.iter().map(|f| (f.file, f.diagnostic.line, f.diagnostic.rule)).collect()
}

/// Files glued in their order, as one: a call goes only up across them,
/// each problem said at its own file and line.
#[test]
fn files_glued_in_order_are_one_script() {
    let a = "globals\n    integer g = 0\nendglobals\nfunction A takes nothing returns nothing\n    call KillUnit(null)\nendfunction";
    let b = "function B takes nothing returns nothing\n    call A()\n    set g = 1\nendfunction\n";
    let env = env();
    assert!(check(&[("a.j", a), ("b.j", b)], &[&env]).is_empty());
    // The other way round: B's call of A is above A, its `g` above the globals
    // (a global after a function too).
    let found = check(&[("b.j", b), ("a.j", a)], &[&env]);
    let rules = said(&found);
    assert!(rules.contains(&(0, 2, parser_rs::rule::CALL_BEFORE_DECLARATION)), "{found:?}");
    assert!(rules.contains(&(1, 2, parser_rs::rule::DECLARATION_AFTER_FUNCTION)), "{found:?}");
    // A line in a message is its file's.
    assert!(found[0].diagnostic.message.contains("(a.j:4)") && found[1].diagnostic.message.contains("(b.j:1)"), "{found:?}");
    assert!(found.iter().all(|f| f.diagnostic.level == Level::Error || !in_strict_group(f.diagnostic.rule)));
    // A span moves with its line.
    let call = found.iter().find(|f| f.diagnostic.rule == parser_rs::rule::CALL_BEFORE_DECLARATION).unwrap();
    assert!(call.diagnostic.span.is_none_or(|s| s.start_line == 2));
}

/// A parse error: at its file and line; what the game would stop at.
#[test]
fn a_parse_error_is_at_its_file() {
    let found = check(&[("a.j", "function A takes nothing returns nothing\nendfunction\n"), ("b.j", "\nfunction B takes nothing returns boolean\n    return !true\nendfunction\n")], &[]);
    assert_eq!(said(&found), [(1, 3, "")], "{found:?}");
}

/// A lone carriage return ends a `//` comment, as it ends any line
/// (`parser_rs::lines`): what follows it is code.
#[test]
fn a_lone_carriage_return_ends_a_comment() {
    let found = check(&[("x.j", "function f takes nothing returns nothing\r    // c\rreturn 1337\rendfunction\r")], &[]);
    let errors: Vec<_> = found.iter().filter(|f| f.diagnostic.level == Level::Error).map(|f| (f.diagnostic.line, f.diagnostic.message.as_str())).collect();
    assert!(errors.iter().any(|(l, m)| *l == 3 && m.contains("returns nothing")), "{errors:?}");
}
