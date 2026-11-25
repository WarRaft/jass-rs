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
//! - `type-mismatch`: a value of the wrong type: an argument, a value set
//!   or returned, a condition, an operator's side. A type goes where it or
//!   a type it extends is wanted (`unit` where `widget`, `handle`), an
//!   `integer` where a `real`, `null` where a handle, a `string` or `code`.
//!   `==` compares numbers with numbers, handles with handles, anything
//!   with its own type or `null`; `+` also joins strings.
//! - `array-access`: an array without its index, an index of what isn't
//!   one. `constant-assignment`: a constant set.
//!
//! A value whose type can't be known (an unknown name, another
//! language's) isn't checked further, so one mistake is reported once.
//!
//! A name the environment knows without a signature ([`Declarations::name`]:
//! a public Zinc function, say) is known, and its calls aren't counted.
//!
//! vJASS: what a library or scope declares is known by its name (a
//! `public` one also as `Scope_name`); a struct, interface, module,
//! function interface or `keyword` is a name known without its type (a
//! struct's values are integers to vJASS: they go anywhere). A member
//! (`a.b`, `a.b()`, `this`) has the names it uses checked, and no type:
//! what `a` is takes vJASS's own resolution (the build's). `thistype` is
//! a type, and `LIBRARY_<name>` a boolean constant (whether the library
//! is there).

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
    /// vJASS: each struct `extends array`, what it has (`array-struct`).
    pub array_structs: HashMap<String, Members>,
    /// vJASS: each module, what it gives a struct that implements it.
    pub modules: HashMap<String, Members>,
}

/// A vJASS struct's or module's methods (static or not), the modules it
/// implements, and what an array struct can't take of it: its instance
/// members with a default value or an array, its `onDestroy`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Members {
    pub methods: HashSet<String>,
    pub implements: Vec<String>,
    /// Instance members: name, has a default value, is an array.
    pub fields: Vec<(String, bool, bool)>,
    pub on_destroy: bool,
}

impl Members {
    fn of(members: &[StructMember]) -> Members {
        let mut m = Members::default();
        for x in members {
            match x {
                StructMember::Method(me) => {
                    m.methods.insert(me.sig.name.clone());
                    m.on_destroy |= !me.sig.modifiers.is_static && me.sig.name == "onDestroy";
                }
                StructMember::Implement { name, .. } => m.implements.push(name.clone()),
                StructMember::Field(g) if !g.modifiers.is_static => m.fields.push((g.name.clone(), g.initializer.is_some(), g.is_array)),
                StructMember::Field(_) => {}
            }
        }
        m
    }
}

impl Declarations {
    /// What `program` declares: its functions, natives, globals (in
    /// `globals` and at the top level) and types. Zinc blocks aren't JASS:
    /// their names come with their own language.
    pub fn of(program: &Program) -> Declarations {
        let mut d = Declarations::default();
        d.add_items(&program.items, &[]);
        d
    }

    /// The declarations of `items`, in vJASS's library or scope whose path
    /// is `scope` (a scope in a scope: `[Outer, Inner]`).
    fn add_items(&mut self, items: &[TopLevel], scope: &[String]) {
        // A `public` one is everyone's as `Scope_name` too; a nested
        // scope's as `Outer_Inner_name`, and `Inner_name` in `Outer`.
        let public = |name: &str, m: &Modifiers| -> Vec<String> {
            match m.access {
                Access::Public if !scope.is_empty() => (0..scope.len()).map(|k| format!("{}_{name}", scope[k..].join("_"))).collect(),
                _ => Vec::new(),
            }
        };
        for item in items {
            match item {
                TopLevel::Function(f) => {
                    self.add_sig(&f.sig, false);
                    for n in public(&f.sig.name, &f.sig.modifiers) {
                        self.add_sig(&FunctionSig { name: n, ..f.sig.clone() }, false);
                    }
                }
                TopLevel::Native(sig) => self.add_sig(sig, true),
                TopLevel::Globals(decls) => {
                    for g in decls {
                        self.add_global(g);
                        for n in public(&g.name, &g.modifiers) {
                            self.add_global(&GlobalDecl { name: n, ..g.clone() });
                        }
                    }
                }
                TopLevel::Global(g) => self.add_global(g),
                TopLevel::TypeDef { name, extends, size: None, .. } => {
                    self.types.insert(name.clone(), extends.clone());
                }
                TopLevel::Library(l) => {
                    let mut path = scope.to_vec();
                    path.push(l.name.clone());
                    self.add_items(&l.items, &path);
                }
                TopLevel::Struct(st) => {
                    if st.extends.as_deref() == Some("array") {
                        self.array_structs.entry(st.name.clone()).or_insert_with(|| Members::of(&st.members));
                    }
                    self.others.insert(st.name.clone());
                }
                TopLevel::Module(m) => {
                    self.modules.entry(m.name.clone()).or_insert_with(|| Members::of(&m.members));
                    self.others.insert(m.name.clone());
                }
                TopLevel::TypeDef { name, .. }
                | TopLevel::Interface(TypeDecl { name, .. })
                | TopLevel::FunctionInterface(FunctionSig { name, .. })
                | TopLevel::Keyword { name, .. } => {
                    self.others.insert(name.clone());
                }
                TopLevel::Stmt(_) | TopLevel::Zinc(_) | TopLevel::Hook { .. } => {}
            }
        }
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
        for (k, v) in &other.array_structs {
            self.array_structs.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.modules {
            self.modules.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

/// The comment written for the declaration at `line` (from 1) of `src`:
/// the `//` lines right above it (no blank line between), and one after
/// it on its line; without `//`, trimmed, a line each. A line of the
/// comment that's only a rule (`//=====`, `//-----`) isn't one, and a
/// rule above ends the comment (it's a section's).
pub fn comment_of(src: &str, line: usize) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let text = |l: &str| l.trim().strip_prefix("//").map(|c| c.trim().to_string());
    let rule = |c: &str| !c.is_empty() && c.chars().all(|ch| matches!(ch, '=' | '-' | '*' | '/' | '#' | '~' | '_'));
    let mut above = Vec::new();
    for l in lines[..line.saturating_sub(1).min(lines.len())].iter().rev() {
        match text(l) {
            Some(c) if rule(&c) => break,
            Some(c) => above.push(c),
            None => break,
        }
    }
    above.reverse();
    // A trailing comment, outside a string.
    if let Some(own) = lines.get(line.wrapping_sub(1)).filter(|l| !l.trim_start().starts_with("//")) {
        let mut in_string = false;
        let chars: Vec<char> = own.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            match chars[i] {
                '"' => in_string = !in_string,
                '\\' if in_string => i += 1,
                '/' if !in_string && chars[i + 1] == '/' => {
                    let c: String = chars[i + 2..].iter().collect();
                    if !c.trim().is_empty() && !rule(c.trim()) {
                        above.push(c.trim().to_string());
                    }
                    break;
                }
                _ => {}
            }
            i += 1;
        }
    }
    // Empty `//` lines at its ends say nothing.
    while above.first().is_some_and(|c| c.is_empty()) {
        above.remove(0);
    }
    while above.last().is_some_and(|c| c.is_empty()) {
        above.pop();
    }
    above.join("\n")
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

    /// A global: `Some(Some(_))` declared in JASS, `Some(None)` known
    /// without its type (another language's), `None` unknown.
    fn global(&self, name: &str) -> Option<Option<&Global>> {
        self.all().find_map(|d| d.globals.get(name).map(Some).or_else(|| d.others.contains(name).then_some(None)))
    }

    /// Is `name` a struct `extends array`?
    fn array_struct(&self, name: &str) -> bool {
        self.all().any(|d| d.array_structs.contains_key(name))
    }

    /// Has the array struct `name` the method `m`, its own or a module's
    /// it implements (through theirs too)?
    fn array_struct_has(&self, name: &str, m: &str) -> bool {
        fn has(scope: &Scope, members: &Members, m: &str, depth: usize) -> bool {
            members.methods.contains(m)
                || (depth < 8 && members.implements.iter().any(|i| scope.all().find_map(|d| d.modules.get(i)).is_some_and(|x| has(scope, x, m, depth + 1))))
        }
        self.all().find_map(|d| d.array_structs.get(name)).is_some_and(|x| has(self, x, m, 0))
    }

    fn type_known(&self, name: &str) -> bool {
        PRIMITIVES.contains(&name) || name == "thistype" || self.all().any(|d| d.types.contains_key(name) || d.others.contains(name))
    }

    /// The type `name` extends, if it's one of the declared types.
    fn parent(&self, name: &str) -> Option<&str> {
        self.all().find_map(|d| d.types.get(name)).map(String::as_str)
    }

    /// Is `name` `handle` or a type extending it?
    fn is_handle(&self, name: &str) -> bool {
        self.extends(name, "handle")
    }

    /// Is `ty` `to`, or does it extend it (through its parents)?
    fn extends(&self, ty: &str, to: &str) -> bool {
        let mut t = ty;
        // A loop of `extends` mustn't hang the check.
        for _ in 0..64 {
            if t == to {
                return true;
            }
            match self.parent(t) {
                Some(p) => t = p,
                None => return false,
            }
        }
        false
    }

    /// Can a value of type `from` go where `to` is wanted? `null` goes into
    /// a handle, a string or code; an integer into a real; a type into one
    /// it extends.
    fn assignable(&self, from: &Ty, to: &str) -> bool {
        // vJASS's types (a struct's values are integers): anything.
        let unchecked = |t: &str| t == "thistype" || self.all().any(|d| d.others.contains(t));
        if unchecked(to) || matches!(from, Ty::Is(f) if unchecked(f)) {
            return true;
        }
        match from {
            Ty::Null => to == "string" || to == "code" || self.is_handle(to),
            Ty::Nothing => false,
            Ty::Is(f) => f == to || (f == "integer" && to == "real") || self.extends(f, to),
        }
    }
}

/// The type of a value: a type's name, `null`, or nothing (a call of a
/// function that returns nothing).
#[derive(Debug, Clone, PartialEq)]
enum Ty {
    Is(String),
    Null,
    Nothing,
}

impl Ty {
    fn of(name: &str) -> Ty {
        Ty::Is(name.to_string())
    }

    fn name(&self) -> &str {
        match self {
            Ty::Is(n) => n,
            Ty::Null => "null",
            Ty::Nothing => "nothing",
        }
    }

    fn is(&self, name: &str) -> bool {
        matches!(self, Ty::Is(n) if n == name)
    }

    fn numeric(&self) -> bool {
        self.is("integer") || self.is("real")
    }
}

/// Every name `program` uses that isn't declared in it or in `env`, every
/// call with the wrong number of arguments, and every value of the wrong
/// type (`type-mismatch`, `array-access`, `constant-assignment`); sorted by
/// line.
pub fn check(program: &Program, env: &[&Declarations]) -> Vec<Diagnostic> {
    let own = Declarations::of(program);
    let scope = Scope { own: &own, env };
    let mut c = Checker { scope, out: Vec::new(), locals: HashMap::new(), returns: None, current: None };
    c.items(&program.items);
    c.out.sort_by_key(|d| d.line);
    c.out
}

/// A variable as a function sees it: its type, is it an array, is it a
/// constant.
struct Var {
    ty: String,
    array: bool,
    constant: bool,
}

struct Checker<'a> {
    scope: Scope<'a>,
    out: Vec<Diagnostic>,
    /// The parameters and locals of the function being checked: type and
    /// whether an array.
    locals: HashMap<String, (String, bool)>,
    /// What the function being checked returns (`None`: nothing).
    returns: Option<Option<String>>,
    /// vJASS: the struct whose method is being checked (`this`,
    /// `thistype`).
    current: Option<String>,
}

/// The span of `name` at the start of `span` (a call's, an array
/// access's): the name alone is marked.
fn name_span(span: Span, name: &str) -> Span {
    Span::new(span.start_line, span.start_col, span.start_line, span.start_col + name.chars().count())
}

impl Checker<'_> {
    fn items(&mut self, items: &[TopLevel]) {
        for item in items {
            match item {
                TopLevel::Function(f) => self.function(f),
                TopLevel::Native(sig) | TopLevel::FunctionInterface(sig) => self.signature(sig),
                TopLevel::Globals(decls) => decls.iter().for_each(|g| self.global(g)),
                TopLevel::Global(g) => self.global(g),
                TopLevel::TypeDef { extends, line, .. } => self.type_name(extends, *line),
                TopLevel::Library(l) => self.items(&l.items),
                TopLevel::Struct(StructDecl { members, .. }) | TopLevel::Module(TypeDecl { members, .. }) | TopLevel::Interface(TypeDecl { members, .. }) => {
                    self.current = match item {
                        TopLevel::Struct(st) => Some(st.name.clone()),
                        _ => None,
                    };
                    if let TopLevel::Struct(st) = item
                        && st.extends.as_deref() == Some("array")
                    {
                        self.array_struct_modules(st);
                    }
                    for m in members {
                        match m {
                            StructMember::Field(g) => self.global(g),
                            StructMember::Method(MethodDecl { sig, body: Some(body), .. }) => {
                                self.function(&FunctionDecl { sig: sig.clone(), body: body.clone() });
                            }
                            StructMember::Method(MethodDecl { sig, .. }) => self.signature(sig),
                            StructMember::Implement { .. } => {}
                        }
                    }
                    self.current = None;
                }
                TopLevel::Stmt(_) | TopLevel::Zinc(_) | TopLevel::Keyword { .. } | TopLevel::Hook { .. } => {}
            }
        }
    }

    fn push(&mut self, d: Diagnostic, span: Option<Span>) {
        self.out.push(match span {
            Some(s) => d.at(s),
            None => d,
        });
    }

    fn unknown(&mut self, what: &str, name: &str, line: usize, span: Option<Span>) {
        self.push(Diagnostic::error(rule::UNKNOWN_NAME, format!("{what} '{name}' is declared nowhere"), line), span);
    }

    /// `ty` (a value's) where `want` is wanted, `what` saying where.
    fn expect(&mut self, ty: Option<Ty>, want: &str, what: impl FnOnce() -> String, line: usize, span: Option<Span>) {
        let Some(ty) = ty else { return };
        // A type that isn't known was reported as unknown already.
        if !self.scope.type_known(want) || self.scope.assignable(&ty, want) {
            return;
        }
        let message = match ty {
            Ty::Nothing => format!("{} is a call of a function that returns nothing, where {want} is wanted", what()),
            t => format!("{} is {}, where {want} is wanted", what(), article(t.name())),
        };
        self.push(Diagnostic::error(rule::TYPE_MISMATCH, message, line), span);
    }

    fn mismatch(&mut self, message: String, line: usize, span: Option<Span>) {
        self.push(Diagnostic::error(rule::TYPE_MISMATCH, message, line), span);
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
            let ty = self.expr(e);
            self.expect(ty, &g.type_name, || format!("the value of '{}'", g.name), g.line, Some(e.span));
        }
    }

    fn function(&mut self, f: &FunctionDecl) {
        self.signature(&f.sig);
        self.locals = f.sig.params.iter().map(|p| (p.name.clone(), (p.type_name.clone(), false))).collect();
        collect_locals(&f.body, &mut self.locals);
        self.returns = Some(f.sig.returns.clone());
        self.stmts(&f.body);
        self.locals.clear();
        self.returns = None;
    }

    /// The variable `name`: a parameter or local, else a global. `None`
    /// when it's unknown (reported) or of another language.
    fn var(&mut self, name: &str, line: usize, span: Option<Span>) -> Option<Var> {
        if let Some((ty, array)) = self.locals.get(name) {
            return Some(Var { ty: ty.clone(), array: *array, constant: false });
        }
        // vJASS: whether a library is there, false when it isn't.
        if name.strip_prefix("LIBRARY_").is_some_and(|l| !l.is_empty()) {
            return Some(Var { ty: "boolean".into(), array: false, constant: true });
        }
        match self.scope.global(name) {
            Some(Some(g)) => Some(Var { ty: g.type_name.clone(), array: g.is_array, constant: g.constant }),
            Some(None) => None,
            // vJASS: a function's name as a value is its pointer, for a
            // function interface (`repeater3(2.0, double)`): no variable,
            // and no type here.
            None if self.scope.function(name).is_some() => None,
            None => {
                self.unknown("the variable", name, line, span);
                None
            }
        }
    }

    /// An array's index, an integer.
    fn index(&mut self, index: &Expr) {
        let ty = self.expr(index);
        self.expect(ty, "integer", || "an array's index".to_string(), index.span.start_line, Some(index.span));
    }

    fn call(&mut self, name: &str, args: &[Expr], line: usize, span: Option<Span>) -> Option<Ty> {
        let types: Vec<Option<Ty>> = args.iter().map(|a| self.expr(a)).collect();
        // vJASS: `thistype(x)`, a cast.
        if name == "thistype" {
            return None;
        }
        let sig = match self.scope.function(name) {
            None => {
                self.unknown("the function", name, line, span);
                return None;
            }
            Some(None) => return None,
            Some(Some(sig)) => sig.clone(),
        };
        if sig.params.len() != args.len() {
            let takes = match sig.params.len() {
                0 => "nothing".to_string(),
                1 => "1 argument".to_string(),
                n => format!("{n} arguments"),
            };
            self.push(Diagnostic::error(rule::ARGUMENT_COUNT, format!("'{name}' takes {takes}, not {}", args.len()), line), span);
        } else {
            for (i, ((a, ty), want)) in args.iter().zip(types).zip(&sig.params).enumerate() {
                self.expect(ty, want, || format!("argument {} of '{name}'", i + 1), a.span.start_line, Some(a.span));
            }
        }
        Some(sig.returns.as_deref().map_or(Ty::Nothing, Ty::of))
    }

    fn stmts(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    /// A condition (`if`, `elseif`, `exitwhen`): a boolean.
    fn condition(&mut self, cond: &Expr, what: &str) {
        let ty = self.expr(cond);
        self.expect(ty, "boolean", || format!("the condition of '{what}'"), cond.span.start_line, Some(cond.span));
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Local { type_name, name, initializer, line, .. } => {
                self.type_name(type_name, *line);
                if let Some(e) = initializer {
                    let ty = self.expr(e);
                    self.expect(ty, type_name, || format!("the value of '{name}'"), *line, Some(e.span));
                }
            }
            Stmt::Set { name, index, value, line, .. } => {
                let var = self.var(name, *line, None);
                if let Some(i) = index {
                    self.index(i);
                }
                let ty = self.expr(value);
                let Some(var) = var else { return };
                if var.constant {
                    let d = Diagnostic::error(rule::CONSTANT_ASSIGNMENT, format!("'{name}' is a constant: it can't be set"), *line);
                    self.push(d, None);
                }
                match (var.array, index.is_some()) {
                    (true, false) => self.array(format!("'{name}' is an array: it's set by an index ({name}[i] = ...)"), *line, None),
                    (false, true) => self.array(format!("'{name}' isn't an array: it has no index"), *line, None),
                    _ => self.expect(ty, &var.ty, || format!("the value set to '{name}'"), *line, Some(value.span)),
                }
            }
            Stmt::Call { name, args, line, .. } => {
                self.call(name, args, *line, None);
            }
            Stmt::If { branches, else_branch, .. } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    self.condition(cond, if i == 0 { "if" } else { "elseif" });
                    self.stmts(body);
                }
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::Loop { body, .. } => self.stmts(body),
            Stmt::ExitWhen { cond, .. } => self.condition(cond, "exitwhen"),
            Stmt::Return { value, line } => {
                let ty = value.as_ref().map(|v| self.expr(v));
                match (self.returns.clone().flatten(), value) {
                    (Some(want), Some(v)) => self.expect(ty.flatten(), &want, || "the value returned".to_string(), *line, Some(v.span)),
                    (None, Some(v)) => {
                        self.mismatch("the function returns nothing: 'return' takes no value here".to_string(), *line, Some(v.span))
                    }
                    // A missing value: `missing-return`'s, or nothing to return.
                    (_, None) => {}
                }
            }
            Stmt::Debug { stmt, .. } => self.stmt(stmt),
            Stmt::SetMember { target, index, value, .. } => {
                self.expr(target);
                if let Some(i) = index {
                    self.index(i);
                }
                self.expr(value);
            }
            Stmt::CallMethod { call, .. } => {
                self.expr(call);
            }
            Stmt::StaticIf { branches, else_branch, .. } => {
                for (cond, body) in branches {
                    self.condition(cond, "static if");
                    self.stmts(body);
                }
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
        }
    }

    fn array(&mut self, message: String, line: usize, span: Option<Span>) {
        self.push(Diagnostic::error(rule::ARRAY_ACCESS, message, line), span);
    }

    /// The type of `e`, its parts checked; `None` when it can't be known
    /// (an unknown name, reported, or another language's).
    fn expr(&mut self, e: &Expr) -> Option<Ty> {
        let line = e.span.start_line;
        match &e.kind {
            ExprKind::IntLiteral(_) => Some(Ty::of("integer")),
            ExprKind::RealLiteral(_) => Some(Ty::of("real")),
            ExprKind::StringLiteral(_) => Some(Ty::of("string")),
            ExprKind::BoolLiteral(_) => Some(Ty::of("boolean")),
            ExprKind::Null => Some(Ty::Null),
            ExprKind::Var(name) => {
                let var = self.var(name, line, Some(e.span))?;
                if var.array {
                    self.array(format!("'{name}' is an array: a value of it needs an index ({name}[i])"), line, Some(e.span));
                    return None;
                }
                Some(Ty::Is(var.ty))
            }
            ExprKind::ArrayAccess(name, index) => {
                let var = self.var(name, line, Some(name_span(e.span, name)));
                self.index(index);
                let var = var?;
                if !var.array {
                    self.array(format!("'{name}' isn't an array: it has no index"), line, Some(name_span(e.span, name)));
                    return None;
                }
                Some(Ty::Is(var.ty))
            }
            ExprKind::Call(name, args) => self.call(name, args, line, Some(name_span(e.span, name))),
            // vJASS: `function Data.onTimer`, a method.
            ExprKind::FuncRef(name) if name.contains('.') => Some(Ty::of("code")),
            ExprKind::FuncRef(name) => {
                match self.scope.function(name) {
                    None => self.unknown("the function", name, line, Some(e.span)),
                    Some(Some(sig)) if !sig.params.is_empty() => {
                        let d = Diagnostic::error(
                            rule::ARGUMENT_COUNT,
                            format!("'function {name}' is a code value, and '{name}' takes arguments: a code value's function takes nothing"),
                            line,
                        );
                        self.push(d, Some(e.span));
                    }
                    Some(_) => {}
                }
                Some(Ty::of("code"))
            }
            ExprKind::Unary(op, x) => {
                let ty = self.expr(x)?;
                match op {
                    UnaryOp::Not => {
                        self.expect(Some(ty), "boolean", || "the value of 'not'".to_string(), line, Some(x.span));
                        Some(Ty::of("boolean"))
                    }
                    UnaryOp::Neg | UnaryOp::Plus => {
                        if !ty.numeric() {
                            self.mismatch(format!("a sign takes a number, not {}", article(ty.name())), line, Some(x.span));
                            return None;
                        }
                        Some(ty)
                    }
                }
            }
            ExprKind::Binary(a, op, b) => {
                let (ta, tb) = (self.expr(a), self.expr(b));
                self.binary(*op, ta, tb, e)
            }
            // vJASS: a sized array's `.size`, a 2D one's `.width` and
            // `.height`: integers, the array named without its index.
            ExprKind::Member(o, f)
                if matches!(f.as_str(), "size" | "width" | "height")
                    && matches!(&o.kind, ExprKind::Var(n) if self.locals.get(n).map(|l| l.1).or_else(|| self.scope.global(n).flatten().map(|g| g.is_array)) == Some(true)) =>
            {
                Some(Ty::of("integer"))
            }
            // vJASS: its parts checked, its type unknown here.
            ExprKind::Member(..) | ExprKind::MethodCall(..) | ExprKind::Index(..) => {
                if let ExprKind::MethodCall(o, m, _) = &e.kind {
                    self.array_struct_call(o, m, e);
                }
                for c in e.children() {
                    self.expr(c);
                }
                None
            }
            ExprKind::This | ExprKind::ThisType | ExprKind::Super => None,
        }
    }

    /// `array-struct`: `S.allocate()`, `S.create()` without one of its own,
    /// `x.destroy()`, `x.deallocate()` of a struct `extends array` (told by
    /// its name, `thistype`, `this` in its methods, or a variable of its
    /// type), unless a method of its own (or a module's) is so named.
    fn array_struct_call(&mut self, o: &Expr, m: &str, e: &Expr) {
        let (s, statics) = match &o.kind {
            ExprKind::ThisType => (self.current.clone(), true),
            ExprKind::This => (self.current.clone(), false),
            ExprKind::Var(n) if self.locals.contains_key(n) => (Some(self.locals[n].0.clone()), false),
            ExprKind::Var(n) if self.scope.array_struct(n) && !self.scope.all().any(|d| d.globals.contains_key(n)) => (Some(n.clone()), true),
            ExprKind::Var(n) => (self.scope.global(n).flatten().filter(|g| !g.is_array).map(|g| g.type_name.clone()), false),
            _ => (None, false),
        };
        let s = s.and_then(|s| if s == "thistype" { self.current.clone() } else { Some(s) });
        let Some(s) = s.filter(|s| self.scope.array_struct(s)) else { return };
        let refused = if statics { matches!(m, "allocate" | "create") } else { matches!(m, "destroy" | "deallocate") };
        if !refused || self.scope.array_struct_has(&s, m) {
            return;
        }
        let d = Diagnostic::error(rule::ARRAY_STRUCT, crate::array_struct::call(&s, m), e.span.start_line);
        self.push(d, Some(e.span));
    }

    /// `array-struct`: what the modules an array struct implements (and
    /// theirs) would give it that it can't have, at each `implement`.
    fn array_struct_modules(&mut self, st: &StructDecl) {
        use crate::array_struct as says;
        for m in &st.members {
            let StructMember::Implement { name, line, .. } = m else { continue };
            // The module, the modules it implements ... each once.
            let mut queue = vec![name.clone()];
            let mut seen = HashSet::new();
            while let Some(module) = queue.pop() {
                if !seen.insert(module.clone()) || seen.len() > 16 {
                    continue;
                }
                let Some(x) = self.scope.all().find_map(|d| d.modules.get(&module)).cloned() else { continue };
                for (field, init, array) in &x.fields {
                    if *init {
                        self.push(Diagnostic::error(rule::ARRAY_STRUCT, says::default_value(&st.name, field, Some(&module)), *line), None);
                    }
                    if *array {
                        self.push(Diagnostic::error(rule::ARRAY_STRUCT, says::array_member(&st.name, field, Some(&module)), *line), None);
                    }
                }
                if x.on_destroy {
                    self.push(Diagnostic::error(rule::ARRAY_STRUCT, says::on_destroy(&st.name, Some(&module)), *line), None);
                }
                queue.extend(x.implements.iter().cloned());
            }
        }
    }

    fn binary(&mut self, op: BinOp, ta: Option<Ty>, tb: Option<Ty>, e: &Expr) -> Option<Ty> {
        let line = e.span.start_line;
        let span = Some(e.span);
        match op {
            BinOp::And | BinOp::Or => {
                let word = if op == BinOp::And { "and" } else { "or" };
                let ExprKind::Binary(a, _, b) = &e.kind else { unreachable!("a binary expression") };
                self.expect(ta, "boolean", || format!("a side of '{word}'"), line, Some(a.span));
                self.expect(tb, "boolean", || format!("a side of '{word}'"), line, Some(b.span));
                Some(Ty::of("boolean"))
            }
            BinOp::Eq | BinOp::NotEq => {
                let (Some(x), Some(y)) = (ta, tb) else { return Some(Ty::of("boolean")) };
                if !self.comparable(&x, &y) {
                    self.mismatch(format!("{} and {} can't be compared", article(x.name()), article(y.name())), line, span);
                }
                Some(Ty::of("boolean"))
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                let (Some(x), Some(y)) = (ta, tb) else { return Some(Ty::of("boolean")) };
                if !x.numeric() || !y.numeric() {
                    self.mismatch(format!("{} and {} can't be compared by size: numbers can", article(x.name()), article(y.name())), line, span);
                }
                Some(Ty::of("boolean"))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let (x, y) = (ta?, tb?);
                if op == BinOp::Add && (x.is("string") || y.is("string")) {
                    // A string and a string (or null).
                    let ok = |t: &Ty| t.is("string") || *t == Ty::Null;
                    if !ok(&x) || !ok(&y) {
                        self.mismatch(format!("'+' joins a string to a string, not {}", article(if ok(&x) { y.name() } else { x.name() })), line, span);
                        // Reported once: what it makes isn't known.
                        return None;
                    }
                    return Some(Ty::of("string"));
                }
                if !x.numeric() || !y.numeric() {
                    let sign = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        _ => "/",
                    };
                    let bad = if x.numeric() { &y } else { &x };
                    self.mismatch(format!("'{sign}' takes numbers, not {}", article(bad.name())), line, span);
                    return None;
                }
                Some(if x.is("real") || y.is("real") { Ty::of("real") } else { Ty::of("integer") })
            }
        }
    }

    /// Can `==` compare these? Numbers with numbers, handles with handles
    /// (any two: they're all handles), `null` with anything (the game
    /// takes `x != null` of a boolean too, as map optimizers write it to
    /// make a boolean 0 or 1), and anything with its own type.
    fn comparable(&self, x: &Ty, y: &Ty) -> bool {
        match (x, y) {
            (Ty::Nothing, _) | (_, Ty::Nothing) => false,
            (Ty::Null, _) | (_, Ty::Null) => true,
            (Ty::Is(a), Ty::Is(b)) => {
                a == b || (x.numeric() && y.numeric()) || (self.scope.is_handle(a) && self.scope.is_handle(b))
            }
        }
    }
}

/// `a unit`, `an integer`, `null`, `nothing`. (No type of the game's
/// starting with `u` is said with `an`: unit, ubersplat, unitpool.)
fn article(name: &str) -> String {
    match name {
        "null" | "nothing" => name.to_string(),
        n if n.starts_with(['a', 'e', 'i', 'o']) => format!("an {n}"),
        n => format!("a {n}"),
    }
}

/// The locals declared anywhere in `body`: type and whether an array.
fn collect_locals(body: &[Stmt], out: &mut HashMap<String, (String, bool)>) {
    for s in body {
        match s {
            Stmt::Local { name, type_name, is_array, .. } => {
                out.insert(name.clone(), (type_name.clone(), *is_array));
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
