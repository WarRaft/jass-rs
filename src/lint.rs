use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub line: usize,
}

/// Runs every lint rule over a parsed program and returns the diagnostics
/// found, sorted by source line.
pub fn lint(program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    check_duplicate_top_level_names(program, &mut diags);
    check_duplicate_global_names(program, &mut diags);
    for item in &program.items {
        if let TopLevel::Function(f) = item {
            check_function(f, &mut diags);
        }
    }
    diags.sort_by_key(|d| d.line);
    diags
}

fn check_duplicate_global_names(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &program.items {
        if let TopLevel::Globals(decls) = item {
            for d in decls {
                if let Some(&first) = seen.get(&d.name) {
                    diags.push(Diagnostic {
                        level: Level::Error,
                        message: format!(
                            "duplicate global variable '{}' (first declared at line {first})",
                            d.name
                        ),
                        line: d.line,
                    });
                } else {
                    seen.insert(d.name.clone(), d.line);
                }
            }
        }
    }
}

fn check_duplicate_top_level_names(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &program.items {
        let (name, line) = match item {
            TopLevel::Function(f) => (&f.sig.name, f.sig.line),
            TopLevel::Native(sig) => (&sig.name, sig.line),
            _ => continue,
        };
        if let Some(&first) = seen.get(name) {
            diags.push(Diagnostic {
                level: Level::Error,
                message: format!("duplicate function '{name}' (first declared at line {first})"),
                line,
            });
        } else {
            seen.insert(name.clone(), line);
        }
    }
}

fn check_function(f: &FunctionDecl, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for p in &f.sig.params {
        seen.insert(p.name.clone(), f.sig.line);
    }

    let mut locals = Vec::new();
    collect_locals(&f.body, &mut locals);

    let mut used = HashSet::new();
    for s in &f.body {
        collect_used_vars_stmt(s, &mut used);
    }

    for local in locals {
        if let Stmt::Local { name, line, .. } = local {
            if let Some(&first_line) = seen.get(name) {
                diags.push(Diagnostic {
                    level: Level::Error,
                    message: format!(
                        "duplicate local variable '{name}' (first declared at line {first_line})"
                    ),
                    line: *line,
                });
            } else {
                seen.insert(name.clone(), *line);
                if !used.contains(name) {
                    diags.push(Diagnostic {
                        level: Level::Warning,
                        message: format!("unused local variable '{name}'"),
                        line: *line,
                    });
                }
            }
        }
    }

    check_stmts(&f.body, diags);

    if f.sig.returns.is_some() && !contains_return(&f.body) {
        diags.push(Diagnostic {
            level: Level::Warning,
            message: format!(
                "function '{}' declares a return type but has no 'return' statement",
                f.sig.name
            ),
            line: f.sig.line,
        });
    }
}

fn collect_locals<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
    for s in stmts {
        match s {
            Stmt::Local { .. } => out.push(s),
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                for (_, body) in branches {
                    collect_locals(body, out);
                }
                if let Some(body) = else_branch {
                    collect_locals(body, out);
                }
            }
            Stmt::Loop { body, .. } => collect_locals(body, out),
            _ => {}
        }
    }
}

fn check_stmts(stmts: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for s in stmts {
        match s {
            Stmt::Loop { body, line } => {
                if !contains_exitwhen(body) {
                    diags.push(Diagnostic {
                        level: Level::Warning,
                        message: "loop has no 'exitwhen' and may never terminate".to_string(),
                        line: *line,
                    });
                }
                check_stmts(body, diags);
            }
            Stmt::If {
                branches,
                else_branch,
                line,
            } => {
                for (_, body) in branches {
                    if body.is_empty() {
                        diags.push(Diagnostic {
                            level: Level::Warning,
                            message: "empty 'then' branch".to_string(),
                            line: *line,
                        });
                    }
                    check_stmts(body, diags);
                }
                if let Some(body) = else_branch {
                    if body.is_empty() {
                        diags.push(Diagnostic {
                            level: Level::Warning,
                            message: "empty 'else' branch".to_string(),
                            line: *line,
                        });
                    }
                    check_stmts(body, diags);
                }
            }
            _ => {}
        }
    }
}

fn contains_exitwhen(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::ExitWhen { .. } => true,
        Stmt::If {
            branches,
            else_branch,
            ..
        } => {
            branches.iter().any(|(_, body)| contains_exitwhen(body))
                || else_branch
                    .as_ref()
                    .map(|b| contains_exitwhen(b))
                    .unwrap_or(false)
        }
        _ => false,
    })
}

fn contains_return(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return { .. } => true,
        Stmt::If {
            branches,
            else_branch,
            ..
        } => {
            branches.iter().any(|(_, body)| contains_return(body))
                || else_branch
                    .as_ref()
                    .map(|b| contains_return(b))
                    .unwrap_or(false)
        }
        Stmt::Loop { body, .. } => contains_return(body),
        _ => false,
    })
}

fn collect_used_vars(e: &Expr, used: &mut HashSet<String>) {
    match e {
        Expr::Var(name) => {
            used.insert(name.clone());
        }
        Expr::ArrayAccess(name, idx) => {
            used.insert(name.clone());
            collect_used_vars(idx, used);
        }
        Expr::Call(_, args) => {
            for a in args {
                collect_used_vars(a, used);
            }
        }
        Expr::Unary(_, inner) => collect_used_vars(inner, used),
        Expr::Binary(l, _, r) => {
            collect_used_vars(l, used);
            collect_used_vars(r, used);
        }
        Expr::IntLiteral(_)
        | Expr::RealLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::FuncRef(_) => {}
    }
}

fn collect_used_vars_stmt(s: &Stmt, used: &mut HashSet<String>) {
    match s {
        Stmt::Local { initializer, .. } => {
            if let Some(e) = initializer {
                collect_used_vars(e, used);
            }
        }
        Stmt::Set { index, value, .. } => {
            if let Some(i) = index {
                collect_used_vars(i, used);
            }
            collect_used_vars(value, used);
        }
        Stmt::Call { args, .. } => {
            for a in args {
                collect_used_vars(a, used);
            }
        }
        Stmt::If {
            branches,
            else_branch,
            ..
        } => {
            for (cond, body) in branches {
                collect_used_vars(cond, used);
                for s in body {
                    collect_used_vars_stmt(s, used);
                }
            }
            if let Some(body) = else_branch {
                for s in body {
                    collect_used_vars_stmt(s, used);
                }
            }
        }
        Stmt::Loop { body, .. } => {
            for s in body {
                collect_used_vars_stmt(s, used);
            }
        }
        Stmt::ExitWhen { cond, .. } => collect_used_vars(cond, used),
        Stmt::Return { value, .. } => {
            if let Some(e) = value {
                collect_used_vars(e, used);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn lint_src(src: &str) -> Vec<Diagnostic> {
        let program = Parser::parse_str(src).expect("source should parse");
        lint(&program)
    }

    #[test]
    fn flags_duplicate_global() {
        let diags = lint_src("globals\n    integer x = 1\n    integer x = 2\nendglobals");
        assert!(diags.iter().any(
            |d| d.level == Level::Error && d.message.contains("duplicate global variable 'x'")
        ));
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
        assert!(
            diags
                .iter()
                .any(|d| d.level == Level::Error
                    && d.message.contains("duplicate local variable 'x'"))
        );
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
}
