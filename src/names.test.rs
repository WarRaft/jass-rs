use super::*;
use crate::parser::Parser;

const COMMON: &str = "\
type agent extends handle
type widget extends agent
type unit extends widget
type timer extends agent
native KillUnit takes unit u returns nothing
native GetTriggerUnit takes nothing returns unit
native GetWidgetLife takes widget w returns real
native Condition takes code func returns handle
native I2S takes integer i returns string
constant integer bj_MAX = 12
integer array counts
";

fn env() -> Declarations {
    Declarations::of(&Parser::parse_str(COMMON).unwrap())
}

fn found(src: &str, env: &[&Declarations]) -> Vec<(usize, &'static str, String)> {
    let p = Parser::parse_str(src).unwrap_or_else(|e| panic!("{}: {}", e.line, e.message));
    check(&p, env).into_iter().map(|d| (d.line, d.rule, d.message)).collect()
}

#[test]
fn declarations_of_a_program() {
    let d = env();
    assert_eq!(d.types.get("unit").map(String::as_str), Some("widget"));
    assert_eq!(d.functions["KillUnit"], Signature { params: vec!["unit".into()], returns: None, native: true });
    assert!(d.globals["bj_MAX"].constant);
}

#[test]
fn names_resolve_in_the_file_and_its_environment() {
    let common = env();
    let src = "\
unit u = null
function Helper takes integer n returns integer
    return n + bj_MAX
endfunction
function F
    integer k = Helper(1)
    KillUnit(GetTriggerUnit())
    u = GetTriggerUnit()
    if k > 0 then
        timer t
        t = null
    endif
    Condition(function F)
endfunction
";
    assert_eq!(found(src, &[&common]), []);
}

#[test]
fn unknown_names_and_argument_counts() {
    let common = env();
    let src = "\
function G takes unit x returns nothing
endfunction
function F takes footman f
    Nope()
    KillUnit()
    KillUnit(GetTriggerUnit(), 1)
    missing = 1
    G(arr[2])
    Condition(function G)
    Condition(function Gone)
endfunction
";
    assert_eq!(
        found(src, &[&common]),
        [
            (3, rule::UNKNOWN_NAME, "the type 'footman' is declared nowhere".to_string()),
            (4, rule::UNKNOWN_NAME, "the function 'Nope' is declared nowhere".to_string()),
            (5, rule::ARGUMENT_COUNT, "'KillUnit' takes 1 argument, not 0".to_string()),
            (6, rule::ARGUMENT_COUNT, "'KillUnit' takes 1 argument, not 2".to_string()),
            (7, rule::UNKNOWN_NAME, "the variable 'missing' is declared nowhere".to_string()),
            (8, rule::UNKNOWN_NAME, "the variable 'arr' is declared nowhere".to_string()),
            (9, rule::ARGUMENT_COUNT, "'function G' is a code value, and 'G' takes arguments: a code value's function takes nothing".to_string()),
            (10, rule::UNKNOWN_NAME, "the function 'Gone' is declared nowhere".to_string()),
        ]
    );
}

#[test]
fn a_name_of_another_language_is_known() {
    let mut zinc = Declarations::default();
    zinc.name("ZincHelper");
    assert_eq!(found("function F\n    ZincHelper(1, 2)\nendfunction\n", &[&zinc]), []);
}

#[test]
fn the_name_alone_is_marked() {
    let p = Parser::parse_str("function F\n    integer x = Nope(1)\nendfunction\n").unwrap();
    let d = check(&p, &[]);
    let s = d.iter().find(|d| d.message.contains("Nope")).unwrap().span.unwrap();
    assert_eq!((s.start_line, s.start_col, s.end_col), (2, 17, 21));
}

/// The game's scripts and maps' scripts read as the game reads them:
/// `JASS_GAME` is a folder with `common.j` and `Blizzard.j` (`common.j`
/// first, the last given wins), `JASS_FILES` the scripts to check, as a
/// path list (as PATH). The game's scripts must check clean; what the
/// others have is printed (a map's script may well have mistakes the game
/// lets through: `null` for an integer).
#[test]
#[ignore]
fn real_scripts() {
    let read = |p: &std::path::Path| String::from_utf8_lossy(&std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).into_owned();
    let game: Vec<std::path::PathBuf> = std::env::split_paths(&std::env::var_os("JASS_GAME").expect("JASS_GAME")).collect();
    let mut env = Declarations::default();
    for name in ["common.j", "Blizzard.j"] {
        // The last folder that has it.
        let Some(p) = game.iter().rev().map(|g| g.join(name)).find(|p| p.is_file()) else { panic!("no {name}") };
        let src = read(&p);
        let program = crate::parse(&src).unwrap_or_else(|e| panic!("{}: {}: {}", p.display(), e.line, e.message));
        let problems = check(&program, &[&env]);
        assert!(problems.is_empty(), "{}: {:?}", p.display(), &problems[..problems.len().min(5)]);
        env.extend(&Declarations::of(&program));
    }
    for p in std::env::split_paths(&std::env::var_os("JASS_FILES").expect("JASS_FILES")) {
        let src = read(&p);
        let program = crate::parse(&src).unwrap_or_else(|e| panic!("{}: {}: {}", p.display(), e.line, e.message));
        let problems: Vec<String> = check(&program, &[&env]).iter().map(|d| format!("{}: {}", d.line, d.message)).collect();
        println!("{}: {} problems", p.display(), problems.len());
        for problem in &problems {
            println!("    {problem}");
        }
    }
}

#[test]
fn types_that_fit() {
    let common = env();
    // A subtype where its parent is wanted, an integer where a real,
    // null where a handle, a string or code, numbers mixed, strings joined,
    // handles compared, anything compared with null.
    let src = "\
string s = null
function Life takes unit u returns real
    return GetWidgetLife(u) + 1
endfunction
function F returns boolean
    real r = 1
    unit u = GetTriggerUnit()
    handle h = u
    code c = null
    timer t = null
    counts[2] = counts[1] * 2 + 1
    r = r / 2 - Life(u)
    s = \"a\" + I2S(counts[0]) + null
    if u == h and t != null and r >= 1 and (1 < 2.5) != null then
        return not (s == \"x\")
    endif
    exitwhen_ok()
    return u != null
endfunction
function exitwhen_ok
    loop
        exitwhen true
    endloop
endfunction
";
    assert_eq!(found(src, &[&common]), []);
}

#[test]
fn types_that_dont() {
    let common = env();
    let src = "\
constant integer MAX = 1
integer n = \"text\"
function G takes unit u returns integer
    return GetWidgetLife(u)
endfunction
function H
    return 1
endfunction
function F
    widget w = GetTriggerUnit()
    KillUnit(w)
    MAX = 2
    counts = 1
    n[1] = 2
    integer i = counts
    if 1 then
    endif
    i = H()
    i = 1 + true
    i = n[0]
    counts[\"a\"] = 1
    i = \"a\" + 1
    boolean b = 1.5 < GetTriggerUnit()
endfunction
";
    let got: Vec<(usize, &str, String)> = found(src, &[&common]);
    assert_eq!(
        got,
        [
            (2, rule::TYPE_MISMATCH, "the value of 'n' is a string, where integer is wanted".to_string()),
            (4, rule::TYPE_MISMATCH, "the value returned is a real, where integer is wanted".to_string()),
            (7, rule::TYPE_MISMATCH, "the function returns nothing: 'return' takes no value here".to_string()),
            (11, rule::TYPE_MISMATCH, "argument 1 of 'KillUnit' is a widget, where unit is wanted".to_string()),
            (12, rule::CONSTANT_ASSIGNMENT, "'MAX' is a constant: it can't be set".to_string()),
            (13, rule::ARRAY_ACCESS, "'counts' is an array: it's set by an index (counts[i] = ...)".to_string()),
            (14, rule::ARRAY_ACCESS, "'n' isn't an array: it has no index".to_string()),
            (15, rule::ARRAY_ACCESS, "'counts' is an array: a value of it needs an index (counts[i])".to_string()),
            (16, rule::TYPE_MISMATCH, "the condition of 'if' is an integer, where boolean is wanted".to_string()),
            (18, rule::TYPE_MISMATCH, "the value set to 'i' is a call of a function that returns nothing, where integer is wanted".to_string()),
            (19, rule::TYPE_MISMATCH, "'+' takes numbers, not a boolean".to_string()),
            (20, rule::ARRAY_ACCESS, "'n' isn't an array: it has no index".to_string()),
            (21, rule::TYPE_MISMATCH, "an array's index is a string, where integer is wanted".to_string()),
            (22, rule::TYPE_MISMATCH, "'+' joins a string to a string, not an integer".to_string()),
            (23, rule::TYPE_MISMATCH, "a real and a unit can't be compared by size: numbers can".to_string()),
        ]
    );
}
