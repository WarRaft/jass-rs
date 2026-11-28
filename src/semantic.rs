//! Semantic highlighting for JASS: the shared lexical layer (keywords,
//! literals, operators, comments) from [`parser_rs::semantic`], plus
//! identifier classification — functions, natives, parameters, locals,
//! globals, constants, types.
//!
//! Classification works on the (lossy) token stream rather than the AST so
//! that half-typed, unparsable code still highlights. JASS's line-oriented
//! grammar makes that reliable: every declaration has a fixed token shape.
//!
//! The same pass records every name as a symbol [`Occurrence`]
//! ([`semantic_tokens_and_symbols`]), so the editor's symbol index agrees
//! with the highlighting. Parameters and locals are scoped to their
//! function ([`Scope::Local`], numbered per file); functions, natives,
//! globals and types are project-wide. Primitive types aren't symbols.
//! An embedded Zinc block's names come from `zinc-rs`, its local scopes
//! numbered on from the file's own, so they never collide.
//!
//! In the vJASS dialect, a `//! zinc` block is one token. Its directive
//! lines are highlighted as `macro`, and its contents are handed to
//! `zinc-rs`'s classifier, the same way the parser hands it the block.

use std::collections::{HashMap, HashSet};

use crate::lexer::Dialect;
use crate::token::{Kw, Punct, Token, TokenKind};
use parser_rs::semantic::{self, comment_tokens, lexical_type};
use parser_rs::{
    Modifiers, Occurrence, Scope, SemanticModifier as M, SemanticToken, SemanticType as T, SymbolKind as K,
};

/// The built-in primitive types (highlighted as `type.defaultLibrary`).
const PRIMITIVES: &[&str] = &["integer", "real", "boolean", "string", "handle", "code"];

/// Every semantic token in `src` (plain JASS), unencoded.
pub fn semantic_tokens(src: &str) -> Vec<SemanticToken> {
    semantic_tokens_with(src, Dialect::Jass)
}

/// Every semantic token in `src` read as `dialect`, unencoded.
pub fn semantic_tokens_with(src: &str, dialect: Dialect) -> Vec<SemanticToken> {
    semantic_tokens_and_symbols(src, dialect).0
}

/// Every name declared or referenced in `src` read as `dialect`.
pub fn symbols(src: &str, dialect: Dialect) -> Vec<Occurrence> {
    semantic_tokens_and_symbols(src, dialect).1
}

/// Semantic tokens and symbol occurrences in one pass: they come from the
/// same classification.
pub fn semantic_tokens_and_symbols(src: &str, dialect: Dialect) -> (Vec<SemanticToken>, Vec<Occurrence>) {
    let lexed = parser_rs::tokenize_lossy(src, dialect.spec());
    let mut out = comment_tokens(&lexed.comments, src);
    let mut symbols = Vec::new();
    Classifier::new(&lexed.tokens).run(&mut out, &mut symbols);
    (out, symbols)
}

fn occurrence(t: &Token, name: &str, kind: K, declaration: bool, scope: Scope) -> Occurrence {
    Occurrence {
        name: name.to_string(),
        line: t.line,
        col: t.col,
        end_col: t.end_col,
        kind,
        declaration,
        scope,
        namespace: String::new(),
        qualifier: None,
    }
}

/// [`semantic_tokens_with`] in the editor's wire format (see
/// [`parser_rs::semantic::encode`]).
pub fn encoded_semantic_tokens(src: &str, dialect: Dialect) -> Vec<u32> {
    semantic::encode(semantic_tokens_with(src, dialect), src)
}

/// An embedded block: its directives as `macro`, its contents (tokens and
/// symbols) by its own language's classifier, which numbers its local
/// scopes on from `next_scope` and advances it.
fn embedded(block: &parser_rs::Embedded, out: &mut Vec<SemanticToken>, syms: &mut Vec<Occurrence>, next_scope: &mut u32) {
    for d in std::iter::once(block.open).chain(block.close) {
        out.push(SemanticToken::new(d.start_line, d.start_col, d.end_col, T::Macro, Modifiers::NONE));
    }
    if block.lang == "zinc" {
        let (tokens, symbols, next) =
            zinc_rs::semantic_tokens_and_symbols_at(&block.source, block.first_line, *next_scope);
        out.extend(tokens);
        syms.extend(symbols);
        *next_scope = next;
    } else {
        // A textmacro's or novjass's text: not code here, shown as a comment.
        for (k, line) in block.source.lines().enumerate() {
            let len = line.chars().count();
            if len > 0 {
                out.push(SemanticToken::new(block.first_line + k, 1, len + 1, T::Comment, Modifiers::NONE));
            }
        }
    }
}

/// File-wide names, collected up front since JASS globals and natives can
/// be referenced from anywhere below their declaration.
#[derive(Default)]
struct Symbols {
    types: HashSet<String>,
    constants: HashSet<String>,
    natives: HashSet<String>,
    /// vJASS's structs, interfaces and modules: type names.
    structs: HashSet<String>,
    interfaces: HashSet<String>,
    modules: HashSet<String>,
    /// Each struct's, interface's and module's members, by the index of
    /// its name's token: its own, its modules' and its parent's (the ones
    /// this file declares), which its methods name without `this.`.
    members: HashMap<usize, HashSet<String>>,
}

struct FunctionScope {
    /// The function's signature number in the file: its [`Scope::Local`].
    id: u32,
    params: HashSet<String>,
    locals: HashSet<String>,
}

impl FunctionScope {
    fn new(id: u32) -> Self {
        FunctionScope { id, params: HashSet::new(), locals: HashSet::new() }
    }
}

/// A vJASS block the classifier is in.
enum Block {
    /// A library or a scope: its local scope number, which its private
    /// declarations are in.
    Library(u32),
    /// A struct, interface or module: its name, and its members.
    Type { name: String, members: HashSet<String>, interface: bool },
}

struct Classifier<'a> {
    toks: &'a [Token],
    symbols: Symbols,
}

fn ident(t: Option<&Token>) -> Option<&str> {
    match t.map(|t| &t.kind) {
        Some(TokenKind::Ident(name)) => Some(name),
        _ => None,
    }
}

fn is_kw(t: Option<&Token>, kw: Kw) -> bool {
    matches!(t.map(|t| &t.kind), Some(TokenKind::Keyword(k)) if *k == kw)
}

fn is_punct(t: Option<&Token>, p: Punct) -> bool {
    matches!(t.map(|t| &t.kind), Some(TokenKind::Punct(q)) if *q == p)
}

/// A declaration's modifiers (vJASS's, and JASS's `constant`).
fn is_modifier(t: &Token) -> bool {
    matches!(
        t.kind,
        TokenKind::Keyword(
            Kw::Constant | Kw::Private | Kw::Public | Kw::Static | Kw::Stub | Kw::Readonly | Kw::Delegate | Kw::Debug
        )
    )
}

/// Is token `i` a declaration's keyword: the first on its line but for
/// modifiers before it (`private static method`, `constant function`)?
fn starts_declaration(toks: &[Token], i: usize) -> bool {
    let mut j = i;
    while j > 0 && toks[j - 1].line == toks[i].line && is_modifier(&toks[j - 1]) {
        j -= 1;
    }
    j == 0 || toks[j - 1].line != toks[j].line
}

/// The name a `method` (at `i`) declares: the next name, or the one after
/// `operator` (`method operator x=`); none for `method operator []`.
fn method_name(toks: &[Token], i: usize) -> Option<usize> {
    let n = if is_kw(toks.get(i + 1), Kw::Operator) { i + 2 } else { i + 1 };
    ident(toks.get(n)).map(|_| n)
}

impl<'a> Classifier<'a> {
    fn new(toks: &'a [Token]) -> Self {
        let mut symbols = Symbols::default();
        for (i, t) in toks.iter().enumerate() {
            let next = |n: usize| toks.get(i + n);
            match &t.kind {
                TokenKind::Keyword(Kw::Type) => {
                    symbols.types.extend(ident(next(1)).map(String::from));
                }
                TokenKind::Keyword(Kw::Native) => {
                    symbols.natives.extend(ident(next(1)).map(String::from));
                }
                // `constant <type> <name>`, in `globals` or at the top level
                // (`constant native` / `constant function` have a keyword next)
                TokenKind::Keyword(Kw::Constant) if ident(next(1)).is_some() => {
                    symbols.constants.extend(ident(next(2)).map(String::from));
                }
                TokenKind::Keyword(Kw::Struct) => symbols.structs.extend(ident(next(1)).map(String::from)),
                TokenKind::Keyword(Kw::Interface) => symbols.interfaces.extend(ident(next(1)).map(String::from)),
                TokenKind::Keyword(Kw::Module) => symbols.modules.extend(ident(next(1)).map(String::from)),
                _ => {}
            }
        }
        symbols.members = members(toks);
        Classifier { toks, symbols }
    }

    fn type_token(&self, t: &Token, name: &str, declaration: bool) -> SemanticToken {
        let mods = Modifiers::NONE
            .with_if(declaration, M::Declaration)
            .with_if(PRIMITIVES.contains(&name), M::DefaultLibrary);
        let ty = if self.symbols.structs.contains(name) {
            T::Struct
        } else if self.symbols.interfaces.contains(name) {
            T::Interface
        } else {
            T::Type
        };
        SemanticToken::at(t, ty, mods)
    }

    /// The kind of a type name used: a struct, interface, module or type.
    fn type_kind(&self, name: &str) -> K {
        if self.symbols.structs.contains(name) {
            K::Struct
        } else if self.symbols.interfaces.contains(name) {
            K::Interface
        } else if self.symbols.modules.contains(name) {
            K::Module
        } else {
            K::Type
        }
    }

    /// Is `name` a vJASS type of the file's: a struct, interface or module?
    fn is_struct_like(&self, name: &str) -> bool {
        self.symbols.structs.contains(name) || self.symbols.interfaces.contains(name) || self.symbols.modules.contains(name)
    }

    fn run(&self, out: &mut Vec<SemanticToken>, syms: &mut Vec<Occurrence>) {
        let toks = self.toks;
        let mut scope: Option<FunctionScope> = None;
        // Every signature gets its own local scope, numbered from 1, so a
        // native's parameters are distinct symbols too; a library's and a
        // scope's are numbered with them.
        let mut signatures: u32 = 0;
        // The line of a `function`/`native`/`method` signature being read,
        // if any, and the index of its keyword.
        let mut sig_line: Option<usize> = None;
        let mut decl_kw: Option<usize> = None;
        // vJASS: the libraries, scopes, structs ... around, innermost last.
        let mut blocks: Vec<Block> = Vec::new();
        // The line of a library's requirements being read
        // (`requires A, optional B`, written over several lines too).
        let mut requires = false;
        // The line a `private` or `public` stands on, and which.
        let mut private_line: Option<usize> = None;
        let mut hook_line: Option<usize> = None;
        // Each library's private names, and the project-scope references
        // made inside libraries (with the libraries around, innermost
        // first): once the file is read, those that name a private
        // declaration of a library around them are in its scope (a name
        // may be used above its declaration).
        let mut privates: HashMap<u32, HashSet<String>> = HashMap::new();
        let mut lib_refs: Vec<(usize, Vec<u32>)> = Vec::new();

        for (i, t) in toks.iter().enumerate() {
            let prev = i.checked_sub(1).and_then(|j| toks.get(j));
            let next = toks.get(i + 1);
            if sig_line.is_some_and(|l| l != t.line) {
                sig_line = None;
            }
            let libraries: Vec<u32> = blocks
                .iter()
                .rev()
                .filter_map(|b| match b {
                    Block::Library(id) => Some(*id),
                    _ => None,
                })
                .collect();
            // The struct (interface, module) whose body this is, outside a
            // method; or whose method this is.
            let in_type = blocks.last().and_then(|b| match b {
                Block::Type { name, members, interface } => Some((name.as_str(), members, *interface)),
                _ => None,
            });
            let declares = starts_declaration(toks, i);

            let name = match &t.kind {
                TokenKind::Ident(name) => name.as_str(),
                kind => {
                    match kind {
                        TokenKind::Keyword(Kw::Function | Kw::Native) if declares => {
                            sig_line = Some(t.line);
                            decl_kw = Some(i);
                        }
                        // A method has a body but an interface's.
                        TokenKind::Keyword(Kw::Method) if declares => {
                            sig_line = Some(t.line);
                            decl_kw = Some(i);
                            signatures += 1;
                            if !in_type.is_some_and(|(_, _, interface)| interface) {
                                scope = Some(FunctionScope::new(signatures));
                            }
                        }
                        TokenKind::Keyword(Kw::EndFunction | Kw::EndMethod) => scope = None,
                        TokenKind::Keyword(Kw::Library | Kw::LibraryOnce | Kw::Scope) if declares => {
                            signatures += 1;
                            blocks.push(Block::Library(signatures));
                        }
                        TokenKind::Keyword(Kw::EndLibrary | Kw::EndScope) => {
                            if let Some(at) = blocks.iter().rposition(|b| matches!(b, Block::Library(_))) {
                                blocks.truncate(at);
                            }
                        }
                        TokenKind::Keyword(kw @ (Kw::Struct | Kw::Interface | Kw::Module)) if declares => {
                            if let Some(n) = ident(next) {
                                let members = self.symbols.members.get(&(i + 1)).cloned().unwrap_or_default();
                                blocks.push(Block::Type { name: n.to_string(), members, interface: *kw == Kw::Interface });
                            }
                        }
                        TokenKind::Keyword(Kw::EndStruct | Kw::EndInterface | Kw::EndModule) => {
                            if let Some(at) = blocks.iter().rposition(|b| matches!(b, Block::Type { .. })) {
                                blocks.truncate(at);
                            }
                            scope = None;
                        }
                        TokenKind::Keyword(Kw::Requires | Kw::Needs | Kw::Uses) => requires = true,
                        TokenKind::Keyword(Kw::Private) => private_line = Some(t.line),
                        TokenKind::Keyword(Kw::Public) => private_line = None,
                        TokenKind::Keyword(Kw::Hook) => hook_line = Some(t.line),
                        TokenKind::Embedded(block) => {
                            let mut next = signatures + 1;
                            embedded(block, out, syms, &mut next);
                            signatures = next - 1;
                        }
                        _ => {}
                    }
                    // Requirements go on through `optional` and commas.
                    if !matches!(kind, TokenKind::Keyword(Kw::Requires | Kw::Needs | Kw::Uses | Kw::Optional) | TokenKind::Punct(Punct::Comma)) {
                        requires = false;
                    }
                    if let Some(ty) = lexical_type(kind) {
                        out.push(SemanticToken::at(t, ty, Modifiers::NONE));
                    }
                    continue;
                }
            };

            let decl = Modifiers::NONE.with(M::Declaration);
            // A declaration (`<type> name`, `function Name` ...) is on one
            // line: a name ending one line and one starting the next aren't
            // one (`local unit u` and then `KillUnit(u)`, `call` left out).
            let prev_here = prev.filter(|p| p.line == t.line);
            let next_here = next.filter(|n| n.line == t.line);
            let private = private_line == Some(t.line);
            // A declaration of a library's (not a member's, not a
            // function's): private ones are the library's alone.
            let library_scope = |privates: &mut HashMap<u32, HashSet<String>>| match libraries.first() {
                Some(id) if private => {
                    privates.entry(*id).or_default().insert(name.to_string());
                    Scope::Local(*id)
                }
                _ => Scope::Project,
            };
            // A project-scope reference: inside a library, maybe one of its
            // private names.
            let mut reference = |syms: &mut Vec<Occurrence>, o: Occurrence| {
                if !libraries.is_empty() && o.scope == Scope::Project {
                    lib_refs.push((syms.len(), libraries.clone()));
                }
                syms.push(o);
            };
            let member = |kind: K, declaration: bool| {
                let mut o = occurrence(t, name, kind, declaration, Scope::Member);
                if declaration {
                    o.namespace = in_type.map_or(String::new(), |(n, _, _)| n.to_string());
                }
                o
            };
            let type_use = |syms: &mut Vec<Occurrence>, reference: &mut dyn FnMut(&mut Vec<Occurrence>, Occurrence)| {
                if !PRIMITIVES.contains(&name) {
                    reference(syms, occurrence(t, name, self.type_kind(name), false, Scope::Project));
                }
            };

            // A library's requirements: names written without commas too
            // (`requires /* */ A /* */ B`), over lines; a name starting a
            // line of a declaration (`integer x`) ends them.
            if requires && !(prev_here.is_none() && matches!(next_here.map(|n| &n.kind), Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array)) | Some(TokenKind::Punct(Punct::Assign)))) {
                reference(syms, occurrence(t, name, K::Library, false, Scope::Project));
                out.push(SemanticToken::at(t, T::Namespace, Modifiers::NONE));
                continue;
            }
            requires = false;

            let token = match prev_here.map(|p| &p.kind) {
                // `function Name` / `native Name` declarations
                Some(TokenKind::Keyword(kw @ (Kw::Function | Kw::Native)))
                    if decl_kw == Some(i - 1) =>
                {
                    signatures += 1;
                    if *kw == Kw::Function {
                        scope = Some(FunctionScope::new(signatures));
                    }
                    let native = *kw == Kw::Native;
                    let kind = if native { K::Native } else { K::Function };
                    syms.push(occurrence(t, name, kind, true, library_scope(&mut privates)));
                    SemanticToken::at(t, T::Function, decl.with_if(native, M::DefaultLibrary))
                }
                // A method's name (`method name`, `method operator name=`).
                Some(TokenKind::Keyword(Kw::Method | Kw::Operator)) if decl_kw.is_some_and(|k| method_name(toks, k) == Some(i)) => {
                    syms.push(member(K::Method, true));
                    let is_static = toks[..i].iter().rev().take_while(|p| p.line == t.line).any(|p| is_kw(Some(p), Kw::Static));
                    SemanticToken::at(t, T::Method, decl.with_if(is_static, M::Static))
                }
                // `library Name`, `scope Name`
                Some(TokenKind::Keyword(kw @ (Kw::Library | Kw::LibraryOnce | Kw::Scope))) => {
                    let kind = if *kw == Kw::Scope { K::Namespace } else { K::Library };
                    syms.push(occurrence(t, name, kind, true, Scope::Project));
                    SemanticToken::at(t, T::Namespace, decl)
                }
                // `struct Name`, `interface Name`, `module Name`, and
                // `function interface Name`
                Some(TokenKind::Keyword(kw @ (Kw::Struct | Kw::Interface | Kw::Module))) => {
                    let kind = match kw {
                        Kw::Struct => K::Struct,
                        Kw::Interface => K::Interface,
                        _ => K::Module,
                    };
                    if decl_kw.is_some_and(|k| k + 1 == i - 1) {
                        // A function interface: its parameters a scope of their own.
                        signatures += 1;
                    }
                    syms.push(occurrence(t, name, kind, true, library_scope(&mut privates)));
                    let ty = match kw {
                        Kw::Struct => T::Struct,
                        Kw::Interface => T::Interface,
                        _ => T::Type,
                    };
                    SemanticToken::at(t, ty, decl)
                }
                // `implement [optional] Module`
                Some(TokenKind::Keyword(Kw::Implement)) => {
                    reference(syms, occurrence(t, name, K::Module, false, Scope::Project));
                    SemanticToken::at(t, T::Type, Modifiers::NONE)
                }
                Some(TokenKind::Keyword(Kw::Optional)) if is_kw(i.checked_sub(2).and_then(|j| toks.get(j)), Kw::Implement) => {
                    reference(syms, occurrence(t, name, K::Module, false, Scope::Project));
                    SemanticToken::at(t, T::Type, Modifiers::NONE)
                }
                // `initializer Init`, `keyword Name` (declared ahead),
                // `hook Native Function`
                Some(TokenKind::Keyword(Kw::Initializer)) => {
                    reference(syms, occurrence(t, name, K::Function, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                Some(TokenKind::Keyword(Kw::Keyword)) => {
                    reference(syms, occurrence(t, name, self.type_kind(name), false, Scope::Project));
                    self.type_token(t, name, false)
                }
                // A member: `a.b`, `.b`, `a.b()`.
                Some(TokenKind::Punct(Punct::Dot)) => {
                    let method = is_punct(next, Punct::LParen);
                    syms.push(member(if method { K::Method } else { K::Field }, false));
                    let modified = matches!(next.map(|n| &n.kind), Some(TokenKind::Punct(Punct::Assign | Punct::PlusAssign | Punct::MinusAssign)));
                    let ty = if method { T::Method } else { T::Property };
                    SemanticToken::at(t, ty, Modifiers::NONE.with_if(modified, M::Modification))
                }
                // `Struct.member`, `function Struct.method`: the struct.
                _ if is_punct(next, Punct::Dot) && self.is_struct_like(name) => {
                    type_use(syms, &mut reference);
                    self.type_token(t, name, false)
                }
                // `F.evaluate(...)`, `F.execute(...)`, `F.name`: a function.
                _ if is_punct(next, Punct::Dot) && matches!(ident(toks.get(i + 2)), Some("evaluate" | "execute" | "name")) && !scope.as_ref().is_some_and(|s| s.params.contains(name) || s.locals.contains(name)) => {
                    reference(syms, occurrence(t, name, K::Function, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                _ if hook_line == Some(t.line) => {
                    reference(syms, occurrence(t, name, K::Function, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                // `function Name` as a code value
                Some(TokenKind::Keyword(Kw::Function)) => {
                    reference(syms, occurrence(t, name, K::Function, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                Some(TokenKind::Keyword(Kw::Type)) => {
                    syms.push(occurrence(t, name, K::Type, true, library_scope(&mut privates)));
                    self.type_token(t, name, true)
                }
                Some(TokenKind::Keyword(Kw::Extends | Kw::Returns)) => {
                    type_use(syms, &mut reference);
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the type part
                _ if matches!(
                    next_here.map(|n| &n.kind),
                    Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array))
                ) =>
                {
                    type_use(syms, &mut reference);
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the declared name
                Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array | Kw::ThisType)) => {
                    if sig_line == Some(t.line) {
                        if let Some(s) = scope.as_mut() {
                            s.params.insert(name.to_string());
                        }
                        let id = signatures;
                        syms.push(occurrence(t, name, K::Parameter, true, Scope::Local(id)));
                        SemanticToken::at(t, T::Parameter, decl)
                    } else if let Some(s) = scope.as_mut() {
                        s.locals.insert(name.to_string());
                        syms.push(occurrence(t, name, K::Local, true, Scope::Local(s.id)));
                        SemanticToken::at(t, T::Variable, decl)
                    } else if in_type.is_some() {
                        // A struct's field.
                        syms.push(member(K::Field, true));
                        let readonly = self.symbols.constants.contains(name);
                        let is_static = toks[..i].iter().rev().take_while(|p| p.line == t.line).any(|p| is_kw(Some(p), Kw::Static));
                        SemanticToken::at(t, T::Property, decl.with_if(readonly, M::Readonly).with_if(is_static, M::Static))
                    } else {
                        // A global, in `globals` or at the top level.
                        let readonly = self.symbols.constants.contains(name);
                        let kind = if readonly { K::Constant } else { K::Global };
                        syms.push(occurrence(t, name, kind, true, library_scope(&mut privates)));
                        SemanticToken::at(t, T::Variable, decl.with_if(readonly, M::Readonly))
                    }
                }
                // calls
                _ if is_punct(next, Punct::LParen) => {
                    if in_type.is_some_and(|(_, m, _)| scope.is_some() && m.contains(name)) {
                        // A method of its struct's, `this.` left out.
                        syms.push(member(K::Method, false));
                        SemanticToken::at(t, T::Method, Modifiers::NONE)
                    } else {
                        let native = self.symbols.natives.contains(name);
                        let kind = if native { K::Native } else { K::Function };
                        reference(syms, occurrence(t, name, kind, false, Scope::Project));
                        SemanticToken::at(t, T::Function, Modifiers::NONE.with_if(native, M::DefaultLibrary))
                    }
                }
                // references
                _ => {
                    // After `set`, or first on its line in a function with
                    // `=` or `[` after it: an assignment, `set` left out.
                    let bare = scope.is_some()
                        && prev.is_none_or(|p| p.line != t.line)
                        && matches!(next.map(|n| &n.kind), Some(TokenKind::Punct(Punct::Assign | Punct::LBracket)));
                    let modified = is_kw(prev, Kw::Set) || bare;
                    let local = scope.as_ref();
                    let (ty, mods) = if let Some(s) = local.filter(|s| s.params.contains(name)) {
                        syms.push(occurrence(t, name, K::Parameter, false, Scope::Local(s.id)));
                        (T::Parameter, Modifiers::NONE)
                    } else if let Some(s) = local.filter(|s| s.locals.contains(name)) {
                        syms.push(occurrence(t, name, K::Local, false, Scope::Local(s.id)));
                        (T::Variable, Modifiers::NONE)
                    } else if local.is_some() && in_type.is_some_and(|(_, m, _)| m.contains(name)) {
                        // A field of its struct's, `this.` left out.
                        syms.push(member(K::Field, false));
                        (T::Property, Modifiers::NONE)
                    } else if self.symbols.types.contains(name) || self.is_struct_like(name) || PRIMITIVES.contains(&name) {
                        type_use(syms, &mut reference);
                        out.push(self.type_token(t, name, false));
                        continue;
                    } else {
                        let readonly = self.symbols.constants.contains(name);
                        let kind = if readonly { K::Constant } else { K::Global };
                        reference(syms, occurrence(t, name, kind, false, Scope::Project));
                        (T::Variable, Modifiers::NONE.with_if(readonly, M::Readonly))
                    };
                    SemanticToken::at(t, ty, mods.with_if(modified, M::Modification))
                }
            };
            out.push(token);
        }

        // A reference inside a library to a private name of it (or of a
        // library around it) is that name.
        for (index, libraries) in lib_refs {
            if let Some(id) = libraries.iter().find(|id| privates.get(id).is_some_and(|names| names.contains(&syms[index].name))) {
                syms[index].scope = Scope::Local(*id);
            }
        }
    }
}

/// Each struct's, interface's and module's members (fields and methods),
/// by the index of its name's token: its own, then those of the modules
/// it implements and of the struct it extends, when this file declares
/// them (a module's own modules, a parent's parent, too).
fn members(toks: &[Token]) -> HashMap<usize, HashSet<String>> {
    struct Body {
        name: String,
        own: HashSet<String>,
        uses: Vec<String>,
    }
    let mut bodies: Vec<(usize, Body)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut in_method = false;
    let mut interface = false;
    for (i, t) in toks.iter().enumerate() {
        let declares = starts_declaration(toks, i);
        match &t.kind {
            TokenKind::Keyword(kw @ (Kw::Struct | Kw::Interface | Kw::Module)) if declares => {
                if let Some(n) = ident(toks.get(i + 1)) {
                    let mut uses = Vec::new();
                    // `struct S extends P`: the parent's members too.
                    let line = &toks[i..];
                    if let Some(p) = line.iter().take_while(|x| x.line == t.line).position(|x| is_kw(Some(x), Kw::Extends)) {
                        uses.extend(ident(line.get(p + 1)).map(String::from));
                    }
                    bodies.push((i + 1, Body { name: n.to_string(), own: HashSet::new(), uses }));
                    current = Some(bodies.len() - 1);
                    interface = *kw == Kw::Interface;
                    in_method = false;
                }
            }
            TokenKind::Keyword(Kw::EndStruct | Kw::EndInterface | Kw::EndModule) => {
                current = None;
                in_method = false;
            }
            TokenKind::Keyword(Kw::Method) if declares => {
                if let (Some(c), Some(n)) = (current, method_name(toks, i)) {
                    bodies[c].1.own.extend(ident(toks.get(n)).map(String::from));
                }
                in_method = !interface;
            }
            TokenKind::Keyword(Kw::EndMethod) => in_method = false,
            TokenKind::Keyword(Kw::Implement) => {
                let n = if is_kw(toks.get(i + 1), Kw::Optional) { i + 2 } else { i + 1 };
                if let (Some(c), Some(m)) = (current, ident(toks.get(n))) {
                    bodies[c].1.uses.push(m.to_string());
                }
            }
            // A field: `[modifiers] <type> [array] name`, the name after a
            // type on its line and no name after it.
            TokenKind::Ident(name) if current.is_some() && !in_method => {
                let prev = i.checked_sub(1).and_then(|j| toks.get(j)).filter(|p| p.line == t.line);
                let next = toks.get(i + 1).filter(|n| n.line == t.line);
                let after_type = matches!(prev.map(|p| &p.kind), Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array | Kw::ThisType)));
                let name_next = matches!(next.map(|n| &n.kind), Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array)));
                let header = toks[..i].iter().rev().take_while(|p| p.line == t.line).any(|p| matches!(p.kind, TokenKind::Keyword(Kw::Struct | Kw::Interface | Kw::Module | Kw::Implement | Kw::Extends)));
                if after_type && !name_next && !header {
                    bodies[current.expect("just checked")].1.own.insert(name.clone());
                }
            }
            _ => {}
        }
    }
    // Each body's members with those it takes from others, by name.
    let by_name: HashMap<&str, usize> = bodies.iter().enumerate().map(|(k, (_, b))| (b.name.as_str(), k)).collect();
    let mut out = HashMap::new();
    for (k, (at, _)) in bodies.iter().enumerate() {
        let mut all = HashSet::new();
        let mut seen = HashSet::new();
        let mut todo = vec![k];
        while let Some(b) = todo.pop() {
            if !seen.insert(b) {
                continue;
            }
            all.extend(bodies[b].1.own.iter().cloned());
            todo.extend(bodies[b].1.uses.iter().filter_map(|u| by_name.get(u.as_str()).copied()));
        }
        out.insert(*at, all);
    }
    out
}

#[cfg(test)]
#[path = "semantic.test.rs"]
mod tests;
