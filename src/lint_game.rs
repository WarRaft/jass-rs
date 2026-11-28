//! The game's rules, as pjass (the community's checker, `../pjass-master`)
//! has them, that a program shows by itself (what needs the game's
//! declarations is `names`'s). Each has its rule id: the group `strict`'s
//! break the game's compiling or running the script; the group
//! `warnings`' are likely mistakes or a patch's limits, each set on its
//! own; `global-maybe-uninitialized` and `string-hash-collision` are off.

use super::Options;
use crate::ast::*;
use parser_rs::{rule, Diagnostic, Edit, Fix, Span};
use std::collections::{HashMap, HashSet};

/// The most arguments the game passes in a call.
const MAX_ARGUMENTS: usize = 31;
/// The longest name the game reads.
const MAX_NAME: usize = 3958;
/// What the game keeps for itself, a name of nothing.
const RESERVED: &[&str] = &["alias"];
/// Calls the game can't make in `globals` yet: crashing it (`true`; on
/// saving, `CreateRegion`), or giving null (`false`). pjass's list, till
/// natives get their extended declarations (native-rs).
const UNSAFE_IN_GLOBALS: &[(&str, bool)] = &[
    ("OrderId", false),
    ("OrderId2String", false),
    ("UnitId2String", false),
    ("GetObjectName", true),
    ("CreateQuest", true),
    ("CreateMultiboard", true),
    ("CreateLeaderboard", true),
    ("CreateRegion", true),
];

pub(super) fn check(program: &Program, options: &Options, diags: &mut Vec<Diagnostic>) {
    let items = top_items(&program.items);
    // Globals declared without a value (not arrays, not constants): read
    // before they're set.
    let mut unset_globals: HashMap<&str, usize> = HashMap::new();
    let mut hashes: HashMap<i32, (String, usize)> = HashMap::new();
    for item in &items {
        match item {
            TopLevel::Globals(decls) => {
                for g in decls {
                    global(g, options, &unset_globals, diags);
                    if g.initializer.is_none() && !g.is_array && !g.is_constant {
                        unset_globals.insert(&g.name, g.line);
                    }
                }
            }
            TopLevel::Global(g) => global(g, options, &unset_globals, diags),
            TopLevel::Native(sig) => names_of_sig(sig, diags),
            TopLevel::TypeDef { name, line, .. } => name_check(name, *line, diags),
            TopLevel::Function(f) => {
                names_of_sig(&f.sig, diags);
                function(f, options, &unset_globals, diags);
            }
            _ => {}
        }
    }
    // Every expression: calls, operators, literals.
    let mut exprs: Vec<&Expr> = Vec::new();
    for item in &items {
        match item {
            TopLevel::Globals(decls) => exprs.extend(decls.iter().filter_map(|g| g.initializer.as_ref())),
            TopLevel::Global(g) => exprs.extend(g.initializer.as_ref()),
            TopLevel::Function(f) => {
                let mut calls = Vec::new();
                super::stmts_exprs(&f.body, &mut exprs, &mut calls);
            }
            _ => {}
        }
    }
    let mut concat_seen: HashSet<(usize, usize)> = HashSet::new();
    while let Some(e) = exprs.pop() {
        expression(e, options, &mut hashes, &mut concat_seen, diags);
        exprs.extend(e.children());
    }
    // Calls written as statements: their arguments are expressions above;
    // their count here.
    for item in &items {
        if let TopLevel::Function(f) = item {
            each_stmt(&f.body, &mut |s| {
                if let Stmt::Call { name, args, line, .. } = s {
                    too_many(name, args.len(), *line, None, diags);
                    string_hash(name, args, options, &mut hashes, diags);
                }
            });
        }
    }
}

/// The top level's declarations, vJASS's libraries' among them.
fn top_items(items: &[TopLevel]) -> Vec<&TopLevel> {
    let mut out = Vec::new();
    for item in items {
        match item {
            TopLevel::Library(l) => out.extend(top_items(&l.items)),
            other => out.push(other),
        }
    }
    out
}

/// `reserved-name` and `long-name` of a declared name.
fn name_check(name: &str, line: usize, diags: &mut Vec<Diagnostic>) {
    if RESERVED.contains(&name) {
        diags.push(Diagnostic::error(rule::RESERVED_NAME, format!("'{name}' is a name the game keeps for itself, not one to declare"), line));
    }
    if name.chars().count() > MAX_NAME {
        diags.push(Diagnostic::warning(
            rule::LONG_NAME,
            format!("a name of {} characters, more than the {MAX_NAME} the game reads", name.chars().count()),
            line,
        ));
    }
}

fn names_of_sig(sig: &FunctionSig, diags: &mut Vec<Diagnostic>) {
    name_check(&sig.name, sig.line, diags);
    for p in &sig.params {
        name_check(&p.name, sig.line, diags);
    }
}

/// A variable's own rules: its name, `code` arrays, an array's value, a
/// constant's.
fn variable(name: &str, type_name: &str, is_array: bool, has_value: bool, line: usize, diags: &mut Vec<Diagnostic>) {
    name_check(name, line, diags);
    if is_array && type_name == "code" {
        diags.push(Diagnostic::error(rule::CODE_ARRAY, format!("'{name}' is an array of code, which the game doesn't have"), line));
    }
    if is_array && has_value {
        diags.push(Diagnostic::error(rule::ARRAY_INITIALIZER, format!("the array '{name}' is given a value where it's declared, which the game doesn't read: set its elements"), line));
    }
}

fn global(g: &GlobalDecl, options: &Options, unset: &HashMap<&str, usize>, diags: &mut Vec<Diagnostic>) {
    let _ = options;
    variable(&g.name, &g.type_name, g.is_array, g.initializer.is_some(), g.line, diags);
    if g.is_constant && g.initializer.is_none() {
        diags.push(Diagnostic::error(rule::CONSTANT_WITHOUT_VALUE, format!("the constant '{}' has no value", g.name), g.line));
    }
    let Some(value) = &g.initializer else { return };
    let mut exprs = vec![value];
    while let Some(e) = exprs.pop() {
        match &e.kind {
            ExprKind::Var(n) if unset.contains_key(n.as_str()) => {
                let d = Diagnostic::error(
                    rule::UNINITIALIZED_VARIABLE,
                    format!("'{n}' is read before it has a value (declared at line {}): the game crashes", unset[n.as_str()]),
                    e.span.start_line,
                );
                diags.push(d.at(e.span));
            }
            ExprKind::Call(name, _) => {
                if let Some(&(_, crashes)) = UNSAFE_IN_GLOBALS.iter().find(|(n, _)| n == name) {
                    let what = match (crashes, name.as_str()) {
                        (true, "CreateRegion") => "crashes the game on saving",
                        (true, _) => "crashes the game",
                        (false, _) => "always gives null",
                    };
                    let d = Diagnostic::error(rule::UNSAFE_IN_GLOBALS, format!("'{name}' called in 'globals' {what}: call it in a function"), e.span.start_line);
                    diags.push(d.at(e.span));
                }
            }
            _ => {}
        }
        exprs.extend(e.children());
    }
}

fn function(f: &FunctionDecl, options: &Options, unset_globals: &HashMap<&str, usize>, diags: &mut Vec<Diagnostic>) {
    let _ = options;
    // `local-after-statement`: a local after the first other statement.
    let mut statement = false;
    for s in &f.body {
        match s {
            Stmt::Local { .. } if statement => local_after(s, diags),
            Stmt::Local { .. } => {}
            other => {
                statement = true;
                each_stmt(std::slice::from_ref(other), &mut |inner| {
                    if matches!(inner, Stmt::Local { .. }) {
                        local_after(inner, diags);
                    }
                });
            }
        }
    }
    // Locals: their own rules, and `recursive-local`.
    each_stmt(&f.body, &mut |s| {
        if let Stmt::Local { name, type_name, is_array, initializer, line, .. } = s {
            variable(name, type_name, *is_array, initializer.is_some(), *line, diags);
            if let Some(value) = initializer {
                let mut exprs = vec![value];
                while let Some(e) = exprs.pop() {
                    if matches!(&e.kind, ExprKind::Call(n, _) if *n == f.sig.name) {
                        let d = Diagnostic::error(rule::RECURSIVE_LOCAL, format!("'{}' calls itself in a local's value, which the game doesn't allow", f.sig.name), e.span.start_line);
                        diags.push(d.at(e.span));
                    }
                    exprs.extend(e.children());
                }
            }
        }
    });
    // `uninitialized-variable`: a local read before it's set, in the
    // order written (as the game, and pjass, take it); a parameter has its
    // value. `global-maybe-uninitialized` likewise of a global without one.
    let params: HashSet<&str> = f.sig.params.iter().map(|p| p.name.as_str()).collect();
    let mut locals: HashMap<&str, bool> = HashMap::new();
    each_stmt(&f.body, &mut |s| {
        if let Stmt::Local { name, is_array, .. } = s {
            locals.insert(name, *is_array);
        }
    });
    let mut set: HashSet<String> = params.iter().map(|p| p.to_string()).collect();
    let mut said: HashSet<String> = HashSet::new();
    let mut read = |name: &str, span: Span, set: &HashSet<String>, diags: &mut Vec<Diagnostic>| {
        if set.contains(name) || said.contains(name) {
            return;
        }
        match locals.get(name) {
            Some(false) => {
                said.insert(name.to_string());
                let d = Diagnostic::error(rule::UNINITIALIZED_VARIABLE, format!("the local '{name}' is read before it has a value: the game stops the thread"), span.start_line);
                diags.push(d.at(span));
            }
            Some(true) => {}
            None if unset_globals.contains_key(name) && !params.contains(name) => {
                said.insert(name.to_string());
                let d = Diagnostic::warning(
                    rule::GLOBAL_MAYBE_UNINITIALIZED,
                    format!("the global '{name}' (declared at line {} without a value) may be read before anything sets it", unset_globals[name]),
                    span.start_line,
                );
                diags.push(d.at(span));
            }
            None => {}
        }
    };
    reads_in_order(&f.body, &mut set, &mut |n, sp, set| read(n, sp, set, diags));
}

fn local_after(s: &Stmt, diags: &mut Vec<Diagnostic>) {
    if let Stmt::Local { name, line, .. } = s {
        diags.push(Diagnostic::error(
            rule::LOCAL_AFTER_STATEMENT,
            format!("the local '{name}' comes after a statement: the game reads locals only at a function's top"),
            *line,
        ));
    }
}

/// Every statement of `body`, nested ones too, in order.
fn each_stmt<'a>(body: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for s in body {
        f(s);
        match s {
            Stmt::If { branches, else_branch, .. } | Stmt::StaticIf { branches, else_branch, .. } => {
                for (_, b) in branches {
                    each_stmt(b, f);
                }
                if let Some(b) = else_branch {
                    each_stmt(b, f);
                }
            }
            Stmt::Loop { body, .. } => each_stmt(body, f),
            Stmt::Debug { stmt, .. } => each_stmt(std::slice::from_ref(stmt), f),
            _ => {}
        }
    }
}

/// The variables `body` reads (`read`, with what's set so far) and sets
/// (into `set`), in the order written: a statement's reads, then what it
/// sets.
fn reads_in_order(body: &[Stmt], set: &mut HashSet<String>, read: &mut impl FnMut(&str, Span, &HashSet<String>)) {
    fn expr(e: &Expr, set: &HashSet<String>, read: &mut impl FnMut(&str, Span, &HashSet<String>)) {
        if let ExprKind::Var(n) = &e.kind {
            read(n, e.span, set);
        }
        for c in e.children() {
            expr(c, set, read);
        }
    }
    for s in body {
        match s {
            Stmt::Local { name, initializer, .. } => {
                if let Some(v) = initializer {
                    expr(v, set, read);
                    set.insert(name.clone());
                }
            }
            Stmt::Set { name, index, value, op, .. } => {
                index.iter().for_each(|i| expr(i, set, read));
                if op.is_some() && index.is_none() {
                    let sp = value.span;
                    read(name, Span::new(sp.start_line, 1, sp.start_line, 1), set);
                }
                expr(value, set, read);
                if index.is_none() {
                    set.insert(name.clone());
                }
            }
            Stmt::Call { args, .. } => args.iter().for_each(|a| expr(a, set, read)),
            Stmt::If { branches, else_branch, .. } | Stmt::StaticIf { branches, else_branch, .. } => {
                for (c, b) in branches {
                    expr(c, set, read);
                    reads_in_order(b, set, read);
                }
                if let Some(b) = else_branch {
                    reads_in_order(b, set, read);
                }
            }
            Stmt::Loop { body, .. } => reads_in_order(body, set, read),
            Stmt::ExitWhen { cond, .. } => expr(cond, set, read),
            Stmt::Return { value, .. } => value.iter().for_each(|v| expr(v, set, read)),
            Stmt::Debug { stmt, .. } => reads_in_order(std::slice::from_ref(stmt), set, read),
            Stmt::SetMember { target, index, value, .. } => {
                expr(target, set, read);
                index.iter().for_each(|i| expr(i, set, read));
                expr(value, set, read);
            }
            Stmt::CallMethod { call, .. } => expr(call, set, read),
        }
    }
}

fn too_many(name: &str, count: usize, line: usize, span: Option<Span>, diags: &mut Vec<Diagnostic>) {
    if count > MAX_ARGUMENTS {
        let d = Diagnostic::error(rule::TOO_MANY_ARGUMENTS, format!("'{name}' called with {count} arguments: the game passes {MAX_ARGUMENTS} at most"), line);
        diags.push(match span {
            Some(s) => d.at(s),
            None => d,
        });
    }
}

/// `string-hash-collision`: `StringHash` of two different literals with
/// one hash.
fn string_hash(name: &str, args: &[Expr], _options: &Options, hashes: &mut HashMap<i32, (String, usize)>, diags: &mut Vec<Diagnostic>) {
    let ("StringHash", [arg]) = (name, args) else { return };
    let ExprKind::StringLiteral(text) = &arg.kind else { return };
    let h = crate::string_hash::string_hash(text.as_bytes());
    match hashes.get(&h) {
        Some((other, line)) if other != text => {
            let d = Diagnostic::warning(
                rule::STRING_HASH_COLLISION,
                format!("StringHash(\"{text}\") is StringHash(\"{other}\")'s too (line {line}): {h}"),
                arg.span.start_line,
            );
            diags.push(d.at(arg.span));
        }
        Some(_) => {}
        None => {
            hashes.insert(h, (text.clone(), arg.span.start_line));
        }
    }
}

/// A string concatenation of literals only: their bytes together.
fn literal_concat(e: &Expr) -> Option<usize> {
    match &e.kind {
        ExprKind::StringLiteral(s) => Some(s.len()),
        ExprKind::Binary(a, BinOp::Add, b) => Some(literal_concat(a)? + literal_concat(b)?),
        _ => None,
    }
}

fn expression(e: &Expr, options: &Options, hashes: &mut HashMap<i32, (String, usize)>, concat_seen: &mut HashSet<(usize, usize)>, diags: &mut Vec<Diagnostic>) {
    let line = e.span.start_line;
    match &e.kind {
        ExprKind::Call(name, args) => {
            too_many(name, args.len(), line, Some(e.span), diags);
            string_hash(name, args, options, hashes, diags);
        }
        ExprKind::Binary(_, BinOp::Mod, _) => {
            diags.push(Diagnostic::warning(rule::MODULO_OPERATOR, "the '%' operator, which patches before 1.29 don't have (ModuloInteger does it)", line).at(e.span));
        }
        ExprKind::StringLiteral(s) if options.string_limit > 0 && s.len() > options.string_limit => {
            diags.push(
                Diagnostic::warning(
                    rule::LONG_STRING,
                    format!("a string of {} bytes, more than the {} the game keeps when a saved game is loaded: loading it crashes", s.len(), options.string_limit),
                    line,
                )
                .at(e.span),
            );
        }
        ExprKind::Binary(_, BinOp::Add, _) if options.concat_limit > 0 && !concat_seen.contains(&(e.span.start_line, e.span.start_col)) => {
            if let Some(bytes) = literal_concat(e) {
                // Its parts are this concatenation's, not ones of their own.
                let mut parts = vec![e];
                while let Some(p) = parts.pop() {
                    concat_seen.insert((p.span.start_line, p.span.start_col));
                    parts.extend(p.children());
                }
                if bytes > options.concat_limit {
                    diags.push(
                        Diagnostic::warning(
                            rule::LONG_STRING,
                            format!("strings joined into {bytes} bytes, more than the {} the game keeps when a saved game is loaded: loading it crashes", options.concat_limit),
                            line,
                        )
                        .at(e.span),
                    );
                }
            }
        }
        ExprKind::IntLiteral(_) if options.source.and_then(|src| text_of(src, e.span)).is_some_and(|t| t.starts_with('\'')) => {
            let text = options.source.and_then(|src| text_of(src, e.span)).unwrap_or_default();
            if let Some(d) = unsafe_rawcode(text, e.span) {
                diags.push(d);
            }
        }
        ExprKind::IntLiteral(v) if *v > i64::from(i32::MAX) => {
            let written = options.source.and_then(|src| text_at(src, e.span));
            if written.is_some() {
                diags.push(Diagnostic::warning(rule::NUMBER_OVERFLOW, format!("{} is more than an integer holds (2147483647): the game cuts it", written.unwrap_or_default()), line).at(e.span));
            }
        }
        ExprKind::RealLiteral(_) => {
            // World Editor's "default" of a sound's real (`war3map.w3s`:
            // 2^32, `SetSoundPitch(s, 4294967296.0)`): as it writes it.
            if let Some(t) = options.source.and_then(|src| text_at(src, e.span))
                && t != "4294967296.0"
                && !real_fits(t)
            {
                diags.push(Diagnostic::warning(rule::NUMBER_OVERFLOW, format!("{t} has more digits than the game's parser holds: it cuts it"), line).at(e.span));
            }
        }
        _ => {}
    }
}

/// The text at `span` (columns from 1, lines too; several lines), in `src`.
fn text_of(src: &str, span: Span) -> Option<&str> {
    let offset = |line: usize, col: usize| -> Option<usize> {
        let start = if line == 1 { 0 } else { src.match_indices('\n').nth(line.checked_sub(2)?).map(|(i, _)| i + 1)? };
        let rest = &src[start..];
        Some(start + rest.char_indices().nth(col.checked_sub(1)?).map_or(rest.len(), |(i, _)| i))
    };
    src.get(offset(span.start_line, span.start_col)?..offset(span.end_line, span.end_col)?)
}

/// Is `c` one an editor can't show or keep in a rawcode: a control
/// character (a line break among them), an invisible one, or `U+FFFD`,
/// a byte the text lost?
fn unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{FFFD}' | '\u{FEFF}' | '\u{200B}'..='\u{200F}' | '\u{2028}' | '\u{2029}' | '\u{2060}')
}

/// `unsafe-rawcode`: `text` (`'...'`, at `span`) with a character an
/// editor can't keep; its fix, the same number as `0x...`, when its bytes
/// are all in the text (a lost byte's is the server's to find, on disk).
pub fn unsafe_rawcode(text: &str, span: Span) -> Option<Diagnostic> {
    let inner = text.strip_prefix('\'')?.strip_suffix('\'')?;
    if !inner.chars().any(unsafe_char) {
        return None;
    }
    let lost = inner.contains('\u{FFFD}');
    let message = if lost {
        "a rawcode with bytes that aren't UTF-8, lost reading the file: an editor saving it changes them; written as 0x..., from the file's bytes, it's safe"
    } else {
        "a rawcode with a character an editor can't show or keep: written as 0x..., the same number, it's safe"
    };
    let d = Diagnostic::warning(rule::UNSAFE_RAWCODE, message, span.start_line).at(span);
    if lost {
        return Some(d);
    }
    Some(d.with_fix(Fix { title: format!("Write it as {}", hex_rawcode(&parser_rs::literal::rawcode_bytes(inner))), edits: vec![Edit { span, text: hex_rawcode(&parser_rs::literal::rawcode_bytes(inner)) }] }))
}

/// A rawcode's bytes as the number they make, `0x...` (two digits a byte).
pub fn hex_rawcode(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("0x{hex}")
}

/// The text at `span` (one line, columns from 1), in `src`.
fn text_at(src: &str, span: Span) -> Option<&str> {
    if span.start_line != span.end_line {
        return None;
    }
    let line = src.lines().nth(span.start_line.checked_sub(1)?)?;
    let start = line.char_indices().nth(span.start_col.checked_sub(1)?).map(|(i, _)| i)?;
    let end = line.char_indices().nth(span.end_col.checked_sub(1)?).map_or(line.len(), |(i, _)| i);
    line.get(start..end)
}

/// Does a real literal fit the game's parser (pjass's `validate_real_lit`):
/// its integer part and its fraction's digits in 32-bit integers?
fn real_fits(text: &str) -> bool {
    let (int, frac) = text.split_once('.').unwrap_or((text, ""));
    let mut result: i32 = 0;
    for c in int.bytes() {
        let Some(r) = result.checked_mul(10).and_then(|r| r.checked_add(i32::from(c - b'0'))) else { return false };
        result = r;
    }
    let (mut f, mut pow10, mut n): (i32, i32, usize) = (0, 1, 0);
    for c in frac.bytes() {
        n += 1;
        let Some(next) = f.checked_mul(10).and_then(|r| r.checked_add(i32::from(c - b'0'))) else { return false };
        f = next;
        match pow10.checked_mul(10) {
            Some(p) => pow10 = p,
            None if f != 0 || n == 32 => return false,
            None => {}
        }
    }
    true
}

/// The bytes of the rawcode at `line`, `col` (columns of the text as it's
/// read: a file's invalid bytes each run one `U+FFFD`, a leading
/// byte-order mark left out) in `file`, the file's own bytes: what the
/// text lost. `None` when there's no rawcode there.
pub fn rawcode_bytes_at(file: &[u8], line: usize, col: usize) -> Option<Vec<u8>> {
    let file = file.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(file);
    let start = if line == 1 { 0 } else { file.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(line.checked_sub(2)?)?.0 + 1 };
    let mut at = start;
    let mut chars = 0;
    // The byte of the `col`-th character, as lossy reading counts them.
    'find: for chunk in file[start..].utf8_chunks() {
        for c in chunk.valid().chars() {
            if chars + 1 == col {
                break 'find;
            }
            chars += 1;
            at += c.len_utf8();
        }
        if !chunk.invalid().is_empty() {
            if chars + 1 == col {
                break 'find;
            }
            chars += 1;
            at += chunk.invalid().len();
        }
    }
    if file.get(at) != Some(&b'\'') {
        return None;
    }
    let end = file[at + 1..].iter().position(|&b| b == b'\'')? + at + 1;
    Some(file[at + 1..end].to_vec())
}
