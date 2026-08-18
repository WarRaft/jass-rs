use jass_rs::ast::TopLevel;
use jass_rs::{lint, parse};

const SAMPLE: &str = include_str!("fixtures/sample.j");

#[test]
fn sample_file_parses_successfully() {
    let program = parse(SAMPLE).expect("sample.j should parse without errors");
    assert_eq!(program.items.len(), 7);

    let function_names: Vec<&str> = program
        .items
        .iter()
        .filter_map(|item| match item {
            TopLevel::Function(f) => Some(f.sig.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        function_names,
        vec!["IsEven", "InitCounters", "SumCounters", "Main"]
    );
}

#[test]
fn sample_file_has_no_lint_diagnostics() {
    let program = parse(SAMPLE).expect("sample.j should parse without errors");
    let diagnostics = lint(&program);
    assert!(
        diagnostics.is_empty(),
        "expected sample.j to be clean, got {diagnostics:?}"
    );
}

#[test]
fn reports_parse_error_with_location() {
    let src = "function Broken takes nothing returns nothing\n    set x = \nendfunction";
    let err = parse(src).unwrap_err();
    assert!(err.line >= 1);
}

#[test]
fn lint_catches_multiple_issues_at_once() {
    let src = r#"
globals
    integer x = 1
    integer x = 2
endglobals

function Foo takes nothing returns integer
    local integer unused = 5
    loop
        call DoSomething()
    endloop
endfunction
"#;
    let program = parse(src).expect("should still parse despite lint issues");
    let diagnostics = lint(&program);

    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate global variable 'x'")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("unused local variable 'unused'")));
    assert!(diagnostics.iter().any(|d| d.message.contains("exitwhen")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("no 'return' statement")));
}
