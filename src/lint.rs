use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub use parser_rs::{Diagnostic, Level};
use parser_rs::{rule, Fix};

/// Runs every lint rule over a parsed program and returns the diagnostics
/// found, sorted by source line.
pub fn lint(program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    check_duplicate_top_level_names(program, &mut diags);
    check_declaration_order(program, &mut diags);
    check_duplicate_global_names(program, &mut diags);
    lint_items(&program.items, &mut diags);
    diags.sort_by_key(|d| d.line);
    diags
}

/// The rules of each function, at the top level, in vJASS's libraries and
/// scopes and as its structs' and modules' methods.
fn lint_items(items: &[TopLevel], diags: &mut Vec<Diagnostic>) {
    for item in items {
        match item {
            TopLevel::Function(f) => {
                check_signature(&f.sig, "function", diags);
                check_keywords(&f.body, diags);
                check_function(f, diags);
            }
            TopLevel::Native(sig) => check_signature(sig, "native", diags),
            // Zinc blocks are linted by Zinc's own rules.
            TopLevel::Zinc(block) => diags.extend(zinc_rs::lint(&block.program)),
            TopLevel::Stmt(s) => diags.push(Diagnostic::error(rule::STATEMENT_OUTSIDE_FUNCTION, top_level_stmt_message(s), s.line())),
            TopLevel::Library(l) => lint_items(&l.items, diags),
            TopLevel::StaticIf { .. } => lint_items(&item.static_if_items().cloned().collect::<Vec<_>>(), diags),
            TopLevel::Struct(StructDecl { members, .. }) | TopLevel::Module(TypeDecl { members, .. }) => {
                if let TopLevel::Struct(st) = item
                    && st.extends.as_deref() == Some("array")
                {
                    check_array_struct(st, diags);
                }
                for m in all_members(members) {
                    if let StructMember::Method(MethodDecl { sig, body: Some(body), .. }) = m {
                        let f = FunctionDecl { sig: sig.clone(), body: body.clone() };
                        check_signature(&f.sig, "method", diags);
                        check_keywords(&f.body, diags);
                        check_function(&f, diags);
                    }
                }
            }
            _ => {}
        }
    }
}

/// What a struct `extends array` can't have of its own (`array-struct`;
/// what its modules give it is `names`'s, which sees them): a member's
/// default value, an array member, `onDestroy`. A static member's value is
/// a global's: allowed.
fn check_array_struct(st: &StructDecl, diags: &mut Vec<Diagnostic>) {
    use crate::array_struct as says;
    for m in all_members(&st.members) {
        match m {
            StructMember::Field(g) if !g.modifiers.is_static => {
                if g.initializer.is_some() {
                    diags.push(Diagnostic::error(rule::ARRAY_STRUCT, says::default_value(&st.name, &g.name, None), g.line));
                }
                if g.is_array {
                    diags.push(Diagnostic::error(rule::ARRAY_STRUCT, says::array_member(&st.name, &g.name, None), g.line));
                }
            }
            StructMember::Method(me) if !me.sig.modifiers.is_static && me.sig.name == "onDestroy" => {
                diags.push(Diagnostic::error(rule::ARRAY_STRUCT, says::on_destroy(&st.name, None), me.sig.line));
            }
            _ => {}
        }
    }
}

fn check_duplicate_global_names(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &program.items {
        let decls = match item {
            TopLevel::Globals(decls) => decls.as_slice(),
            TopLevel::Global(d) => std::slice::from_ref(d),
            _ => continue,
        };
        {
            for d in decls {
                if let Some(&first) = seen.get(&d.name) {
                    diags.push(Diagnostic::error(rule::DUPLICATE_DECLARATION, 
                        format!(
                            "duplicate global variable '{}' (first declared at line {first})",
                            d.name
                        ),
                        d.line,
                    ));
                } else {
                    seen.insert(d.name.clone(), d.line);
                }
            }
        }
    }
}

/// Why a statement can't stand at the top level, and what to do instead.
fn top_level_stmt_message(s: &Stmt) -> String {
    let keyword = match s {
        Stmt::Local { .. } => {
            return "'local' outside a function: drop 'local' to declare a global, \
                    or move it into a function"
                .to_string();
        }
        Stmt::Set { .. } => "set",
        Stmt::Call { .. } => "call",
        Stmt::If { .. } => "if",
        Stmt::Loop { .. } => "loop",
        Stmt::ExitWhen { .. } => "exitwhen",
        Stmt::Return { .. } => "return",
        Stmt::Debug { stmt, .. } => return top_level_stmt_message(stmt),
        Stmt::SetMember { .. } => "set",
        Stmt::CallMethod { .. } => "call",
        Stmt::StaticIf { .. } => "static if",
    };
    format!("'{keyword}' statement outside a function: move it into a function")
}

/// The functions the build merges when there are several, as the game
/// knows them: `main` and `config`, each taking and returning nothing.
pub const MERGED_FUNCTIONS: [&str; 2] = ["main", "config"];

/// Is `sig` a function the build merges with the others of its name
/// ([`MERGED_FUNCTIONS`])?
pub fn is_merged(sig: &FunctionSig) -> bool {
    MERGED_FUNCTIONS.contains(&sig.name.as_str()) && sig.params.is_empty() && sig.returns.is_none()
}

/// `duplicate-declaration`, and `merged-function`: `main` and `config`
/// may be declared any number of times, and the build merges them into
/// one, each one's statements in their order; the game reads one.
fn check_duplicate_top_level_names(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in &program.items {
        let (name, line) = match item {
            TopLevel::Function(f) => (&f.sig.name, f.sig.line),
            TopLevel::Native(sig) => (&sig.name, sig.line),
            _ => continue,
        };
        let first = seen.get(name).copied();
        if let (Some(first), TopLevel::Function(f)) = (first, item)
            && is_merged(&f.sig)
            && program.items.iter().any(|i| matches!(i, TopLevel::Function(g) if g.sig.line == first && is_merged(&g.sig)))
        {
            diags.push(Diagnostic::error(
                rule::MERGED_FUNCTION,
                format!("another '{name}' (the first at line {first}): the build merges them into one, the game reads one"),
                line,
            ));
            continue;
        }
        if let Some(first) = first {
            diags.push(Diagnostic::error(rule::DUPLICATE_DECLARATION, 
                format!("duplicate function '{name}' (first declared at line {first})"),
                line,
            ));
        } else {
            seen.insert(name.clone(), line);
        }
    }
}

/// `declaration-after-function` and `global-outside-globals`: the game
/// reads declarations (`type`, `globals`, `native`) only before the first
/// function, and variables only inside `globals`. This toolchain accepts
/// both anywhere at the top level. An empty `globals` block has no line to
/// report and declares nothing, so it isn't reported.
fn check_declaration_order(program: &Program, diags: &mut Vec<Diagnostic>) {
    let mut first_function: Option<usize> = None;
    for item in &program.items {
        let (what, line) = match item {
            TopLevel::Function(f) => {
                first_function.get_or_insert(f.sig.line);
                continue;
            }
            TopLevel::Global(d) => {
                diags.push(Diagnostic::error(
                    rule::GLOBAL_OUTSIDE_GLOBALS,
                    format!("global '{}' is declared outside 'globals ... endglobals', where the game doesn't read it", d.name),
                    d.line,
                ));
                (format!("global '{}'", d.name), d.line)
            }
            TopLevel::Globals(decls) => match decls.first() {
                Some(d) => ("'globals' block".to_string(), d.line),
                None => continue,
            },
            TopLevel::Native(sig) => (format!("native '{}'", sig.name), sig.line),
            TopLevel::TypeDef { name, line, .. } => (format!("type '{name}'"), *line),
            // vJASS's are gathered by JassHelper wherever they are.
            TopLevel::Stmt(_)
            | TopLevel::Zinc(_)
            | TopLevel::Library(_)
            | TopLevel::StaticIf { .. }
            | TopLevel::Struct(_)
            | TopLevel::Interface(_)
            | TopLevel::Module(_)
            | TopLevel::FunctionInterface(_)
            | TopLevel::Keyword { .. }
            | TopLevel::Hook { .. } => continue,
        };
        if let Some(f) = first_function {
            diags.push(Diagnostic::error(
                rule::DECLARATION_AFTER_FUNCTION,
                format!("{what} comes after a function (line {f}): the game reads declarations only before the first function"),
                line,
            ));
        }
    }
}

/// `implicit-keyword`: an assignment without `set`, a call without
/// `call`, a local without `local`. This toolchain reads them; the game
/// requires the keyword.
fn check_keywords(body: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for s in body {
        match s {
            Stmt::Set { keyword: false, name, line, .. } => diags.push(Diagnostic::error(
                rule::IMPLICIT_KEYWORD,
                format!("the assignment to '{name}' leaves out 'set', which the game requires"),
                *line,
            )),
            Stmt::Call { keyword: false, name, line, .. } => diags.push(Diagnostic::error(
                rule::IMPLICIT_KEYWORD,
                format!("the call of '{name}' leaves out 'call', which the game requires"),
                *line,
            )),
            Stmt::Local { keyword: false, name, line, .. } => diags.push(Diagnostic::error(
                rule::IMPLICIT_KEYWORD,
                format!("the local '{name}' leaves out 'local', which the game requires"),
                *line,
            )),
            Stmt::SetMember { keyword: false, line, .. } => diags.push(Diagnostic::error(
                rule::IMPLICIT_KEYWORD,
                "the assignment to a member leaves out 'set', which the game requires",
                *line,
            )),
            Stmt::CallMethod { keyword: false, line, .. } => diags.push(Diagnostic::error(
                rule::IMPLICIT_KEYWORD,
                "the call of a method leaves out 'call', which the game requires",
                *line,
            )),
            Stmt::If { branches, else_branch, .. } | Stmt::StaticIf { branches, else_branch, .. } => {
                for (_, b) in branches {
                    check_keywords(b, diags);
                }
                if let Some(b) = else_branch {
                    check_keywords(b, diags);
                }
            }
            Stmt::Loop { body, .. } => check_keywords(body, diags),
            Stmt::Debug { stmt, .. } => check_keywords(std::slice::from_ref(stmt), diags),
            _ => {}
        }
    }
}

/// `implicit-signature`: `takes` / `returns` left out. This toolchain
/// reads them as `takes nothing` / `returns nothing`; the game requires
/// them written.
fn check_signature(sig: &FunctionSig, what: &str, diags: &mut Vec<Diagnostic>) {
    let missing = match (sig.has_takes, sig.has_returns) {
        (true, true) => return,
        (false, false) => "'takes nothing returns nothing'",
        (false, true) => "'takes nothing'",
        (true, false) => "'returns nothing'",
    };
    diags.push(Diagnostic::error(
        rule::IMPLICIT_SIGNATURE,
        format!("{what} '{}' leaves out {missing}, which the game requires", sig.name),
        sig.line,
    ));
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
                diags.push(Diagnostic::error(rule::DUPLICATE_DECLARATION, 
                    format!(
                        "duplicate local variable '{name}' (first declared at line {first_line})"
                    ),
                    *line,
                ));
            } else {
                seen.insert(name.clone(), *line);
                if !used.contains(name) {
                    diags.push(Diagnostic::warning(rule::UNUSED_LOCAL, 
                        format!("unused local variable '{name}'"),
                        *line,
                    ));
                }
            }
        }
    }

    check_stmts(&f.body, diags);

    if f.sig.returns.is_some() && !contains_return(&f.body) {
        diags.push(Diagnostic::warning(rule::MISSING_RETURN, 
            format!(
                "function '{}' declares a return type but has no 'return' statement",
                f.sig.name
            ),
            f.sig.line,
        ));
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
            }
            | Stmt::StaticIf {
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
            Stmt::Debug { stmt, .. } => collect_locals(std::slice::from_ref(stmt), out),
            _ => {}
        }
    }
}

fn check_stmts(stmts: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for s in stmts {
        match s {
            Stmt::Loop { body, line } => {
                if !contains_exitwhen(body) {
                    diags.push(Diagnostic::warning(rule::INFINITE_LOOP, 
                        "loop has no 'exitwhen' and may never terminate",
                        *line,
                    ));
                }
                check_stmts(body, diags);
            }
            Stmt::If {
                branches,
                else_branch,
                line,
                else_span,
                end_line,
            } => {
                for (_, body) in branches {
                    if body.is_empty() {
                        diags.push(Diagnostic::warning(rule::EMPTY_BRANCH, "empty 'then' branch", *line));
                    }
                    check_stmts(body, diags);
                }
                if let Some(body) = else_branch {
                    if body.is_empty() {
                        diags.push(empty_else(*else_span, *line, *end_line));
                    }
                    check_stmts(body, diags);
                }
            }
            Stmt::Debug { stmt, .. } => check_stmts(std::slice::from_ref(stmt), diags),
            Stmt::StaticIf { branches, else_branch, .. } => {
                for (_, body) in branches {
                    check_stmts(body, diags);
                }
                if let Some(body) = else_branch {
                    check_stmts(body, diags);
                }
            }
            _ => {}
        }
    }
}

/// An empty `else`, marked on the keyword, with a fix that deletes it:
/// every line from `else` up to `endif`.
fn empty_else(else_span: Option<Span>, if_line: usize, end_line: usize) -> Diagnostic {
    let d = Diagnostic::warning(rule::EMPTY_BRANCH, "empty 'else' branch", if_line);
    match else_span {
        Some(kw) => d.at(kw).with_fix(Fix::delete("Remove empty 'else'", Span::new(kw.start_line, 1, end_line, 1))),
        None => d,
    }
}

/// A `debug` statement doesn't count: outside debug mode it's skipped, so
/// it can't be what ends a loop.
fn contains_exitwhen(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::ExitWhen { .. } => true,
        Stmt::If {
            branches,
            else_branch,
            ..
        }
        | Stmt::StaticIf {
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

/// A `debug` statement doesn't count, as in [`contains_exitwhen`].
fn contains_return(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return { .. } => true,
        Stmt::If {
            branches,
            else_branch,
            ..
        }
        | Stmt::StaticIf {
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
    match &e.kind {
        ExprKind::Var(name) => {
            used.insert(name.clone());
        }
        ExprKind::ArrayAccess(name, idx) => {
            used.insert(name.clone());
            collect_used_vars(idx, used);
        }
        ExprKind::Call(_, args) => {
            for a in args {
                collect_used_vars(a, used);
            }
        }
        ExprKind::Unary(_, inner) => collect_used_vars(inner, used),
        ExprKind::Binary(l, _, r) => {
            collect_used_vars(l, used);
            collect_used_vars(r, used);
        }
        ExprKind::Member(..) | ExprKind::MethodCall(..) | ExprKind::Index(..) => {
            for c in e.children() {
                collect_used_vars(c, used);
            }
        }
        ExprKind::IntLiteral(_)
        | ExprKind::RealLiteral(_)
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Null
        | ExprKind::FuncRef(_)
        | ExprKind::This
        | ExprKind::ThisType
        | ExprKind::Super => {}
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
        }
        | Stmt::StaticIf {
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
        // Reading a variable in debug code is still a use.
        Stmt::Debug { stmt, .. } => collect_used_vars_stmt(stmt, used),
        Stmt::SetMember { target, index, value, .. } => {
            collect_used_vars(target, used);
            if let Some(i) = index {
                collect_used_vars(i, used);
            }
            collect_used_vars(value, used);
        }
        Stmt::CallMethod { call, .. } => collect_used_vars(call, used),
    }
}

#[cfg(test)]
#[path = "lint.test.rs"]
mod tests;
