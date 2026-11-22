//! Names: what a program declares ([`Declarations`]), and every name it
//! uses resolved against that and what the rest of its project declares
//! ([`check`]). The rest is given by the caller (the game's `common.j` and
//! `Blizzard.j`, the project's other files): this crate never reads files.
//!
//! - A name is a parameter or local of its function, else a global, a
//!   function, a native or a type of the program or of the environment.
//!   Locals go anywhere in a function (this toolchain's JASS), so a local
//!   is seen in all of its function.
//! - `unknown-name`: a function, variable or type declared nowhere.
//! - `argument-count`: a call with more or fewer arguments than its
//!   function takes, or `function Foo` (a code value) of one that takes
//!   some.
//!
//! A name the environment knows without a signature ([`Declarations::name`]:
//! a public Zinc function, say) is known, and its calls aren't counted.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use parser_rs::rule;

pub use parser_rs::Diagnostic;

/// The types every program has.
pub const PRIMITIVES: [&str; 6] = ["integer", "real", "boolean", "string", "handle", "code"];

/// A function's or a native's signature: its parameters' types and what it
/// returns (`None`: nothing).
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub params: Vec<String>,
    pub returns: Option<String>,
    pub native: bool,
}

/// A global variable.
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub type_name: String,
    pub is_array: bool,
    pub constant: bool,
}

/// What a program (or several) declares at the top level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Declarations {
    pub functions: HashMap<String, Signature>,
    pub globals: HashMap<String, Global>,
    /// Each type and the one it extends.
    pub types: HashMap<String, String>,
    /// Names known without anything more (another language's).
    pub others: HashSet<String>,
}

impl Declarations {
    /// What `program` declares: its functions, natives, globals (in
    /// `globals` and at the top level) and types. Zinc blocks aren't JASS:
    /// their names come with their own language.
    pub fn of(program: &Program) -> Declarations {
        let mut d = Declarations::default();
        for item in &program.items {
            match item {
                TopLevel::Function(f) => d.add_sig(&f.sig, false),
                TopLevel::Native(sig) => d.add_sig(sig, true),
                TopLevel::Globals(decls) => decls.iter().for_each(|g| d.add_global(g)),
                TopLevel::Global(g) => d.add_global(g),
                TopLevel::TypeDef { name, extends, .. } => {
                    d.types.insert(name.clone(), extends.clone());
                }
                TopLevel::Stmt(_) | TopLevel::Zinc(_) => {}
            }
        }
        d
    }

    fn add_sig(&mut self, sig: &FunctionSig, native: bool) {
        let s = Signature { params: sig.params.iter().map(|p| p.type_name.clone()).collect(), returns: sig.returns.clone(), native };
        // The first one: another of the same name is a duplicate (lint),
        // or a `main` the build merges, which takes nothing as well.
        self.functions.entry(sig.name.clone()).or_insert(s);
    }

    fn add_global(&mut self, g: &GlobalDecl) {
        let v = Global { type_name: g.type_name.clone(), is_array: g.is_array, constant: g.is_constant };
        self.globals.entry(g.name.clone()).or_insert(v);
    }

    /// A name known without its declaration.
    pub fn name(&mut self, name: impl Into<String>) {
        self.others.insert(name.into());
    }

    /// Adds `other`'s, keeping what's here already.
    pub fn extend(&mut self, other: &Declarations) {
        for (k, v) in &other.functions {
            self.functions.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.globals {
            self.globals.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.types {
            self.types.entry(k.clone()).or_insert_with(|| v.clone());
        }
        self.others.extend(other.others.iter().cloned());
    }
}

/// Where names are looked up: the program's, then the environment's.
struct Scope<'a> {
    own: &'a Declarations,
    env: &'a [&'a Declarations],
}

impl Scope<'_> {
    fn all(&self) -> impl Iterator<Item = &Declarations> {
        std::iter::once(self.own).chain(self.env.iter().copied())
    }

    fn function(&self, name: &str) -> Option<Option<&Signature>> {
        self.all().find_map(|d| d.functions.get(name).map(Some).or_else(|| d.others.contains(name).then_some(None)))
    }

    fn variable(&self, name: &str) -> bool {
        self.all().any(|d| d.globals.contains_key(name) || d.others.contains(name))
    }

    fn type_known(&self, name: &str) -> bool {
        PRIMITIVES.contains(&name) || self.all().any(|d| d.types.contains_key(name) || d.others.contains(name))
    }
}

/// Every name `program` uses that isn't declared in it or in `env`, and
/// every call with the wrong number of arguments; sorted by line.
pub fn check(program: &Program, env: &[&Declarations]) -> Vec<Diagnostic> {
    let own = Declarations::of(program);
    let scope = Scope { own: &own, env };
    let mut c = Checker { scope, out: Vec::new(), locals: HashSet::new() };
    for item in &program.items {
        match item {
            TopLevel::Function(f) => c.function(f),
            TopLevel::Native(sig) => c.signature(sig),
            TopLevel::Globals(decls) => decls.iter().for_each(|g| c.global(g)),
            TopLevel::Global(g) => c.global(g),
            TopLevel::TypeDef { extends, line, .. } => c.type_name(extends, *line),
            TopLevel::Stmt(_) | TopLevel::Zinc(_) => {}
        }
    }
    c.out.sort_by_key(|d| d.line);
    c.out
}

struct Checker<'a> {
    scope: Scope<'a>,
    out: Vec<Diagnostic>,
    /// The parameters and locals of the function being checked.
    locals: HashSet<String>,
}

/// The span of `name` at the start of `span` (a call's, an array
/// access's): the name alone is marked.
fn name_span(span: Span, name: &str) -> Span {
    Span::new(span.start_line, span.start_col, span.start_line, span.start_col + name.chars().count())
}

impl Checker<'_> {
    fn unknown(&mut self, what: &str, name: &str, line: usize, span: Option<Span>) {
        let d = Diagnostic::error(rule::UNKNOWN_NAME, format!("{what} '{name}' is declared nowhere"), line);
        self.out.push(match span {
            Some(s) => d.at(s),
            None => d,
        });
    }

    fn type_name(&mut self, name: &str, line: usize) {
        if !self.scope.type_known(name) {
            self.unknown("the type", name, line, None);
        }
    }

    fn signature(&mut self, sig: &FunctionSig) {
        for p in &sig.params {
            self.type_name(&p.type_name, sig.line);
        }
        if let Some(r) = &sig.returns {
            self.type_name(r, sig.line);
        }
    }

    fn global(&mut self, g: &GlobalDecl) {
        self.type_name(&g.type_name, g.line);
        if let Some(e) = &g.initializer {
            self.expr(e);
        }
    }

    fn function(&mut self, f: &FunctionDecl) {
        self.signature(&f.sig);
        self.locals = f.sig.params.iter().map(|p| p.name.clone()).collect();
        collect_locals(&f.body, &mut self.locals);
        self.stmts(&f.body);
        self.locals.clear();
    }

    fn variable(&mut self, name: &str, line: usize, span: Option<Span>) {
        if !self.locals.contains(name) && !self.scope.variable(name) {
            self.unknown("the variable", name, line, span);
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], line: usize, span: Option<Span>) {
        for a in args {
            self.expr(a);
        }
        match self.scope.function(name) {
            None => self.unknown("the function", name, line, span),
            Some(Some(sig)) if sig.params.len() != args.len() => {
                let takes = match sig.params.len() {
                    0 => "nothing".to_string(),
                    1 => "1 argument".to_string(),
                    n => format!("{n} arguments"),
                };
                let d = Diagnostic::error(rule::ARGUMENT_COUNT, format!("'{name}' takes {takes}, not {}", args.len()), line);
                self.out.push(match span {
                    Some(s) => d.at(s),
                    None => d,
                });
            }
            Some(_) => {}
        }
    }

    fn stmts(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Local { type_name, initializer, line, .. } => {
                self.type_name(type_name, *line);
                if let Some(e) = initializer {
                    self.expr(e);
                }
            }
            Stmt::Set { name, index, value, line, .. } => {
                self.variable(name, *line, None);
                if let Some(i) = index {
                    self.expr(i);
                }
                self.expr(value);
            }
            Stmt::Call { name, args, line, .. } => self.call(name, args, *line, None),
            Stmt::If { branches, else_branch, .. } => {
                for (cond, body) in branches {
                    self.expr(cond);
                    self.stmts(body);
                }
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::Loop { body, .. } => self.stmts(body),
            Stmt::ExitWhen { cond, .. } => self.expr(cond),
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Stmt::Debug { stmt, .. } => self.stmt(stmt),
        }
    }

    fn expr(&mut self, e: &Expr) {
        let line = e.span.start_line;
        match &e.kind {
            ExprKind::Var(name) => self.variable(name, line, Some(e.span)),
            ExprKind::ArrayAccess(name, index) => {
                self.variable(name, line, Some(name_span(e.span, name)));
                self.expr(index);
            }
            ExprKind::Call(name, args) => self.call(name, args, line, Some(name_span(e.span, name))),
            ExprKind::FuncRef(name) => match self.scope.function(name) {
                None => self.unknown("the function", name, line, Some(e.span)),
                Some(Some(sig)) if !sig.params.is_empty() => {
                    let d = Diagnostic::error(
                        rule::ARGUMENT_COUNT,
                        format!("'function {name}' is a code value, and '{name}' takes arguments: a code value's function takes nothing"),
                        line,
                    );
                    self.out.push(d.at(e.span));
                }
                Some(_) => {}
            },
            ExprKind::Unary(_, x) => self.expr(x),
            ExprKind::Binary(a, _, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::IntLiteral(_) | ExprKind::RealLiteral(_) | ExprKind::StringLiteral(_) | ExprKind::BoolLiteral(_) | ExprKind::Null => {}
        }
    }
}

/// The names of the locals declared anywhere in `body`.
fn collect_locals(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::Local { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::If { branches, else_branch, .. } => {
                for (_, b) in branches {
                    collect_locals(b, out);
                }
                if let Some(b) = else_branch {
                    collect_locals(b, out);
                }
            }
            Stmt::Loop { body, .. } => collect_locals(body, out),
            Stmt::Debug { stmt, .. } => collect_locals(std::slice::from_ref(stmt), out),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "names.test.rs"]
mod tests;
