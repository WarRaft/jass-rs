use super::*;
use crate::parser::Parser;

fn lint_src(src: &str) -> Vec<Diagnostic> {
    let program = Parser::parse_str(src).expect("source should parse");
    lint(&program)
}

#[test]
fn flags_duplicate_global() {
    let diags = lint_src("globals\n    integer x = 1\n    integer x = 2\nendglobals");
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Error && d.message.contains("duplicate global variable 'x'")));
}

#[test]
fn flags_duplicate_function() {
    let src = "function Foo takes nothing returns nothing\nendfunction\nfunction Foo takes nothing returns nothing\nendfunction";
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Error && d.message.contains("duplicate function 'Foo'")));
}

#[test]
fn flags_duplicate_local() {
    let src = r#"
function Foo takes nothing returns nothing
local integer x = 1
local integer x = 2
endfunction
"#;
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Error && d.message.contains("duplicate local variable 'x'")));
}

#[test]
fn flags_unused_local() {
    let src = r#"
function Foo takes nothing returns nothing
local integer x = 1
endfunction
"#;
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Warning && d.message.contains("unused local variable 'x'")));
}

#[test]
fn does_not_flag_used_local() {
    let src = r#"
function Foo takes nothing returns integer
local integer x = 1
return x
endfunction
"#;
    let diags = lint_src(src);
    assert!(!diags.iter().any(|d| d.message.contains("unused local")));
}

#[test]
fn flags_loop_without_exitwhen() {
    let src = r#"
function Foo takes nothing returns nothing
loop
    call DoSomething()
endloop
endfunction
"#;
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Warning && d.message.contains("no 'exitwhen'")));
}

#[test]
fn does_not_flag_loop_with_exitwhen() {
    let src = r#"
function Foo takes nothing returns nothing
local integer i = 0
loop
    exitwhen i > 10
    set i = i + 1
endloop
endfunction
"#;
    let diags = lint_src(src);
    assert!(!diags.iter().any(|d| d.message.contains("no 'exitwhen'")));
}

#[test]
fn flags_empty_then_branch() {
    let src = r#"
function Foo takes nothing returns nothing
if true then
endif
endfunction
"#;
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.message.contains("empty 'then' branch")));
}

#[test]
fn flags_missing_return() {
    let src = "function Foo takes nothing returns integer\nendfunction";
    let diags = lint_src(src);
    assert!(diags
        .iter()
        .any(|d| d.level == Level::Warning && d.message.contains("no 'return' statement")));
}

#[test]
fn clean_function_has_no_diagnostics() {
    let src = r#"
globals
integer counter = 0
endglobals

function Increment takes nothing returns integer
local integer i = 0
loop
    exitwhen i >= counter
    set i = i + 1
endloop
return i
endfunction
"#;
    let diags = lint_src(src);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn top_level_globals_are_fine_and_checked_for_duplicates() {
    let (diags, strict): (Vec<_>, Vec<_>) = lint_src("integer x = 1\nglobals\n    integer y\nendglobals\nreal x")
        .into_iter()
        .partition(|d| d.rule != rule::GLOBAL_OUTSIDE_GLOBALS);
    // Only strict JASS (off by default) objects to them.
    assert_eq!(strict.iter().map(|d| d.line).collect::<Vec<_>>(), vec![1, 5]);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].line, 5);
    assert!(diags[0].message.contains("duplicate global variable 'x' (first declared at line 1)"));
}

#[test]
fn asks_to_move_top_level_statements_into_a_function() {
    let diags = lint_src("if true then\nendif\ncall F()\nlocal integer i\nloop\n    exitwhen true\nendloop");
    let errors: Vec<(usize, &str)> = diags
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| (d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        [
            (1, "'if' statement outside a function: move it into a function"),
            (3, "'call' statement outside a function: move it into a function"),
            (4, "'local' outside a function: drop 'local' to declare a global, or move it into a function"),
            (5, "'loop' statement outside a function: move it into a function"),
        ]
    );
}

#[test]
fn a_variable_read_only_in_debug_code_is_used() {
    let src = "function F takes nothing returns nothing\n    local integer x = 1\n    debug call Print(x)\nendfunction";
    assert!(lint_src(src).iter().all(|d| !d.message.contains("unused")));
}

#[test]
fn debug_code_doesnt_end_a_loop() {
    // Outside debug mode the exitwhen isn't there.
    let src = "function F takes nothing returns nothing\n    loop\n        debug if true then\n            exitwhen true\n        endif\n    endloop\nendfunction";
    assert!(lint_src(src).iter().any(|d| d.message.contains("no 'exitwhen'")));
}

#[test]
fn statements_inside_debug_are_linted() {
    let src = "function F takes nothing returns nothing\n    debug if true then\n    endif\nendfunction";
    assert!(lint_src(src).iter().any(|d| d.message.contains("empty 'then' branch")));
}

#[test]
fn a_debug_statement_at_the_top_level_is_reported_like_its_statement() {
    let program = Parser::parse_str("debug call F()").expect("parses");
    let diags = lint(&program);
    assert!(diags.iter().any(|d| d.message == "'call' statement outside a function: move it into a function"), "{diags:?}");
}

#[test]
fn implicit_signature_names_what_is_left_out() {
    let src = "\
native N
native M takes integer i
function A
endfunction
function B takes integer x
endfunction
constant function C returns integer
    return 1
endfunction
function D takes nothing returns nothing
endfunction
";
    let found: Vec<(usize, String)> = lint_src(src)
        .into_iter()
        .filter(|d| d.rule == rule::IMPLICIT_SIGNATURE)
        .map(|d| {
            assert_eq!(d.level, Level::Error);
            (d.line, d.message)
        })
        .collect();
    assert_eq!(
        found,
        vec![
            (1, "native 'N' leaves out 'takes nothing returns nothing', which the game requires".to_string()),
            (2, "native 'M' leaves out 'returns nothing', which the game requires".to_string()),
            (3, "function 'A' leaves out 'takes nothing returns nothing', which the game requires".to_string()),
            (5, "function 'B' leaves out 'returns nothing', which the game requires".to_string()),
            (7, "function 'C' leaves out 'takes nothing', which the game requires".to_string()),
        ]
    );
}

#[test]
fn an_implicit_signature_is_still_checked_like_a_written_one() {
    // `returns integer` without `takes`: still needs a return.
    let diags = lint_src("function F returns integer
endfunction
");
    assert!(diags.iter().any(|d| d.rule == rule::MISSING_RETURN), "{diags:?}");
    // Parameters without `returns` are still parameters.
    let diags = lint_src("function G takes integer x
    local integer x
endfunction
");
    assert!(diags.iter().any(|d| d.message.contains("duplicate")), "{diags:?}");
}

#[test]
fn strict_jass_declaration_order() {
    let src = "\
type mytype extends handle
integer early = 1
globals
    integer g
endglobals
function F takes nothing returns nothing
endfunction
integer late
globals
    real r
endglobals
globals
endglobals
native N takes nothing returns nothing
type other extends handle
function G takes nothing returns nothing
endfunction
";
    let found: Vec<(&str, usize, String)> = lint_src(src)
        .into_iter()
        .filter(|d| d.rule == rule::DECLARATION_AFTER_FUNCTION || d.rule == rule::GLOBAL_OUTSIDE_GLOBALS)
        .map(|d| {
            assert_eq!(d.level, Level::Error);
            (d.rule, d.line, d.message)
        })
        .collect();
    let after = |what: &str| format!("{what} comes after a function (line 6): the game reads declarations only before the first function");
    let outside = |name: &str| format!("global '{name}' is declared outside 'globals ... endglobals', where the game doesn't read it");
    assert_eq!(
        found,
        vec![
            (rule::GLOBAL_OUTSIDE_GLOBALS, 2, outside("early")),
            (rule::GLOBAL_OUTSIDE_GLOBALS, 8, outside("late")),
            (rule::DECLARATION_AFTER_FUNCTION, 8, after("global 'late'")),
            (rule::DECLARATION_AFTER_FUNCTION, 10, after("'globals' block")),
            (rule::DECLARATION_AFTER_FUNCTION, 14, after("native 'N'")),
            (rule::DECLARATION_AFTER_FUNCTION, 15, after("type 'other'")),
        ]
    );
}

#[test]
fn an_empty_else_is_marked_on_else_and_its_fix_deletes_it() {
    let src = "function F takes nothing returns nothing\n    if true then\n        call F()\n    else\n        // nothing\n    endif\nendfunction";
    let d = lint_src(src).into_iter().find(|d| d.message == "empty 'else' branch").expect("reported");
    assert_eq!(d.line, 4);
    assert_eq!(d.span, Some(Span::new(4, 5, 4, 9)));
    let fix = d.fix.expect("a fix");
    assert_eq!(fix.edits.len(), 1);
    // From the start of the `else` line up to the `endif` line.
    assert_eq!(fix.edits[0].span, Span::new(4, 1, 6, 1));
    assert_eq!(fix.edits[0].text, "");
}

#[test]
fn implicit_keyword_names_what_is_left_out() {
    let src = "\
function F
    x = 1
    if x == 1 then
        Foo()
    endif
    set y = 2
    call Bar()
    loop
        debug a[1] = 2
    endloop
endfunction
";
    let found: Vec<(usize, String)> = lint_src(src)
        .into_iter()
        .filter(|d| d.rule == rule::IMPLICIT_KEYWORD)
        .map(|d| (d.line, d.message))
        .collect();
    assert_eq!(
        found,
        vec![
            (2, "the assignment to 'x' leaves out 'set', which the game requires".to_string()),
            (4, "the call of 'Foo' leaves out 'call', which the game requires".to_string()),
            (9, "the assignment to 'a' leaves out 'set', which the game requires".to_string()),
        ]
    );
}

#[test]
fn main_and_config_may_be_declared_again() {
    let src = "\
function main
endfunction
function config
endfunction
function main
    Foo()
endfunction
function main takes integer x
endfunction
function Other
endfunction
function Other
endfunction
";
    let diags = lint_src(src);
    let of = |r: &str| diags.iter().filter(|d| d.rule == r).map(|d| d.line).collect::<Vec<_>>();
    // Another main merges with the first; one that takes something is a duplicate.
    assert_eq!(of(rule::MERGED_FUNCTION), [5]);
    assert_eq!(of(rule::DUPLICATE_DECLARATION), [8, 12]);
}

#[test]
fn implicit_keyword_names_a_local_too() {
    let diags = lint_src("function F\n    integer i = 0\n    local integer j = i\n    j = j + 1\nendfunction\n");
    let found: Vec<(usize, String)> = diags.iter().filter(|d| d.rule == rule::IMPLICIT_KEYWORD).map(|d| (d.line, d.message.clone())).collect();
    assert_eq!(
        found,
        vec![
            (2, "the local 'i' leaves out 'local', which the game requires".to_string()),
            (4, "the assignment to 'j' leaves out 'set', which the game requires".to_string()),
        ]
    );
    // A local all the same: unused, it's reported as one.
    let diags = lint_src("function F\n    integer k = 0\nendfunction\n");
    assert!(diags.iter().any(|d| d.rule == rule::UNUSED_LOCAL), "{diags:?}");
}
