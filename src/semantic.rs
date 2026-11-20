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

use std::collections::HashSet;

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
    }
}

/// File-wide names, collected up front since JASS globals and natives can
/// be referenced from anywhere below their declaration.
#[derive(Default)]
struct Symbols {
    types: HashSet<String>,
    constants: HashSet<String>,
    natives: HashSet<String>,
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
                _ => {}
            }
        }
        Classifier { toks, symbols }
    }

    fn type_token(&self, t: &Token, name: &str, declaration: bool) -> SemanticToken {
        let mods = Modifiers::NONE
            .with_if(declaration, M::Declaration)
            .with_if(PRIMITIVES.contains(&name), M::DefaultLibrary);
        SemanticToken::at(t, T::Type, mods)
    }

    /// A type name used (not declared): a symbol unless it's primitive.
    fn type_ref(&self, t: &Token, name: &str, syms: &mut Vec<Occurrence>) {
        if !PRIMITIVES.contains(&name) {
            syms.push(occurrence(t, name, K::Type, false, Scope::Project));
        }
    }

    fn run(&self, out: &mut Vec<SemanticToken>, syms: &mut Vec<Occurrence>) {
        let toks = self.toks;
        let mut scope: Option<FunctionScope> = None;
        // Every signature gets its own local scope, numbered from 1, so a
        // native's parameters are distinct symbols too.
        let mut signatures: u32 = 0;
        // The line of a `function`/`native` signature being read, if any,
        // and the index of its keyword.
        let mut sig_line: Option<usize> = None;
        let mut decl_kw: Option<usize> = None;
        let first_on_line = |j: usize| j == 0 || toks[j - 1].line != toks[j].line;

        for (i, t) in toks.iter().enumerate() {
            let prev = i.checked_sub(1).and_then(|j| toks.get(j));
            let next = toks.get(i + 1);
            if sig_line.is_some_and(|l| l != t.line) {
                sig_line = None;
            }
            // A declaration keyword is the first token on its line (after an
            // optional `constant`); `function` elsewhere is a code reference.
            let starts_line =
                first_on_line(i) || (is_kw(prev, Kw::Constant) && first_on_line(i - 1));

            let name = match &t.kind {
                TokenKind::Ident(name) => name.as_str(),
                kind => {
                    match kind {
                        TokenKind::Keyword(Kw::Function | Kw::Native) if starts_line => {
                            sig_line = Some(t.line);
                            decl_kw = Some(i);
                        }
                        TokenKind::Keyword(Kw::EndFunction) => scope = None,
                        TokenKind::Embedded(block) => {
                            let mut next = signatures + 1;
                            embedded(block, out, syms, &mut next);
                            signatures = next - 1;
                        }
                        _ => {}
                    }
                    if let Some(ty) = lexical_type(kind) {
                        out.push(SemanticToken::at(t, ty, Modifiers::NONE));
                    }
                    continue;
                }
            };

            let decl = Modifiers::NONE.with(M::Declaration);
            let token = match prev.map(|p| &p.kind) {
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
                    syms.push(occurrence(t, name, kind, true, Scope::Project));
                    SemanticToken::at(t, T::Function, decl.with_if(native, M::DefaultLibrary))
                }
                // `function Name` as a code value
                Some(TokenKind::Keyword(Kw::Function)) => {
                    syms.push(occurrence(t, name, K::Function, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                Some(TokenKind::Keyword(Kw::Type)) => {
                    syms.push(occurrence(t, name, K::Type, true, Scope::Project));
                    self.type_token(t, name, true)
                }
                Some(TokenKind::Keyword(Kw::Extends | Kw::Returns)) => {
                    self.type_ref(t, name, syms);
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the type part
                _ if matches!(
                    next.map(|n| &n.kind),
                    Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array))
                ) =>
                {
                    self.type_ref(t, name, syms);
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the declared name
                Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array)) => {
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
                    } else {
                        // A global, in `globals` or at the top level.
                        let readonly = self.symbols.constants.contains(name);
                        let kind = if readonly { K::Constant } else { K::Global };
                        syms.push(occurrence(t, name, kind, true, Scope::Project));
                        SemanticToken::at(t, T::Variable, decl.with_if(readonly, M::Readonly))
                    }
                }
                // calls
                _ if matches!(next.map(|n| &n.kind), Some(TokenKind::Punct(Punct::LParen))) => {
                    let native = self.symbols.natives.contains(name);
                    let kind = if native { K::Native } else { K::Function };
                    syms.push(occurrence(t, name, kind, false, Scope::Project));
                    SemanticToken::at(t, T::Function, Modifiers::NONE.with_if(native, M::DefaultLibrary))
                }
                // references
                _ => {
                    let modified = is_kw(prev, Kw::Set);
                    let local = scope.as_ref();
                    let (ty, mods) = if let Some(s) = local.filter(|s| s.params.contains(name)) {
                        syms.push(occurrence(t, name, K::Parameter, false, Scope::Local(s.id)));
                        (T::Parameter, Modifiers::NONE)
                    } else if let Some(s) = local.filter(|s| s.locals.contains(name)) {
                        syms.push(occurrence(t, name, K::Local, false, Scope::Local(s.id)));
                        (T::Variable, Modifiers::NONE)
                    } else if self.symbols.types.contains(name) || PRIMITIVES.contains(&name) {
                        self.type_ref(t, name, syms);
                        out.push(self.type_token(t, name, false));
                        continue;
                    } else {
                        let readonly = self.symbols.constants.contains(name);
                        let kind = if readonly { K::Constant } else { K::Global };
                        syms.push(occurrence(t, name, kind, false, Scope::Project));
                        (T::Variable, Modifiers::NONE.with_if(readonly, M::Readonly))
                    };
                    SemanticToken::at(t, ty, mods.with_if(modified, M::Modification))
                }
            };
            out.push(token);
        }
    }
}

#[cfg(test)]
#[path = "semantic.test.rs"]
mod tests;
