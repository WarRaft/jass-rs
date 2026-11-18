//! Semantic highlighting for JASS: the shared lexical layer (keywords,
//! literals, operators, comments) from [`parser_rs::semantic`], plus
//! identifier classification — functions, natives, parameters, locals,
//! globals, constants, types.
//!
//! Classification works on the (lossy) token stream rather than the AST so
//! that half-typed, unparsable code still highlights. JASS's line-oriented
//! grammar makes that reliable: every declaration has a fixed token shape.
//!
//! In the vJASS dialect, a `//! zinc` block is one token. Its directive
//! lines are highlighted as `macro`, and its contents are handed to
//! `zinc-rs`'s classifier, the same way the parser hands it the block.

use std::collections::HashSet;

use crate::lexer::Dialect;
use crate::token::{Kw, Punct, Token, TokenKind};
use parser_rs::semantic::{self, comment_tokens, lexical_type};
use parser_rs::{Modifiers, SemanticModifier as M, SemanticToken, SemanticType as T};

/// The built-in primitive types (highlighted as `type.defaultLibrary`).
const PRIMITIVES: &[&str] = &["integer", "real", "boolean", "string", "handle", "code"];

/// Every semantic token in `src` (plain JASS), unencoded.
pub fn semantic_tokens(src: &str) -> Vec<SemanticToken> {
    semantic_tokens_with(src, Dialect::Jass)
}

/// Every semantic token in `src` read as `dialect`, unencoded.
pub fn semantic_tokens_with(src: &str, dialect: Dialect) -> Vec<SemanticToken> {
    let lexed = parser_rs::tokenize_lossy(src, dialect.spec());
    let mut out = comment_tokens(&lexed.comments, src);
    Classifier::new(&lexed.tokens).run(&mut out);
    out
}

/// [`semantic_tokens_with`] in the editor's wire format (see
/// [`parser_rs::semantic::encode`]).
pub fn encoded_semantic_tokens(src: &str, dialect: Dialect) -> Vec<u32> {
    semantic::encode(semantic_tokens_with(src, dialect), src)
}

/// An embedded block: its directives as `macro`, its contents by its own
/// language's classifier.
fn embedded_tokens(block: &parser_rs::Embedded, out: &mut Vec<SemanticToken>) {
    for d in std::iter::once(block.open).chain(block.close) {
        out.push(SemanticToken::new(d.start_line, d.start_col, d.end_col, T::Macro, Modifiers::NONE));
    }
    if block.lang == "zinc" {
        out.extend(zinc_rs::semantic_tokens_at(&block.source, block.first_line));
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

#[derive(Default)]
struct FunctionScope {
    params: HashSet<String>,
    locals: HashSet<String>,
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
        let mut in_globals = false;
        for (i, t) in toks.iter().enumerate() {
            let next = |n: usize| toks.get(i + n);
            match &t.kind {
                TokenKind::Keyword(Kw::Globals) => in_globals = true,
                TokenKind::Keyword(Kw::EndGlobals) => in_globals = false,
                TokenKind::Keyword(Kw::Type) => {
                    symbols.types.extend(ident(next(1)).map(String::from));
                }
                TokenKind::Keyword(Kw::Native) => {
                    symbols.natives.extend(ident(next(1)).map(String::from));
                }
                // `constant <type> <name>` inside globals
                TokenKind::Keyword(Kw::Constant) if in_globals => {
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

    fn run(&self, out: &mut Vec<SemanticToken>) {
        let toks = self.toks;
        let mut scope: Option<FunctionScope> = None;
        let mut in_globals = false;
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
                        TokenKind::Keyword(Kw::Globals) => in_globals = true,
                        TokenKind::Keyword(Kw::EndGlobals) => in_globals = false,
                        TokenKind::Keyword(Kw::Function | Kw::Native) if starts_line => {
                            sig_line = Some(t.line);
                            decl_kw = Some(i);
                        }
                        TokenKind::Keyword(Kw::EndFunction) => scope = None,
                        TokenKind::Embedded(block) => embedded_tokens(block, out),
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
                    if *kw == Kw::Function {
                        scope = Some(FunctionScope::default());
                    }
                    let mods = decl.with_if(*kw == Kw::Native, M::DefaultLibrary);
                    SemanticToken::at(t, T::Function, mods)
                }
                // `function Name` as a code value
                Some(TokenKind::Keyword(Kw::Function)) => {
                    SemanticToken::at(t, T::Function, Modifiers::NONE)
                }
                Some(TokenKind::Keyword(Kw::Type)) => self.type_token(t, name, true),
                Some(TokenKind::Keyword(Kw::Extends | Kw::Returns)) => {
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the type part
                _ if matches!(
                    next.map(|n| &n.kind),
                    Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array))
                ) =>
                {
                    self.type_token(t, name, false)
                }
                // `<type> name` / `<type> array name`: the declared name
                Some(TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array)) => {
                    if sig_line == Some(t.line) {
                        if let Some(s) = scope.as_mut() {
                            s.params.insert(name.to_string());
                        }
                        SemanticToken::at(t, T::Parameter, decl)
                    } else if let Some(s) = scope.as_mut() {
                        s.locals.insert(name.to_string());
                        SemanticToken::at(t, T::Variable, decl)
                    } else {
                        let readonly = in_globals && self.symbols.constants.contains(name);
                        SemanticToken::at(t, T::Variable, decl.with_if(readonly, M::Readonly))
                    }
                }
                // calls
                _ if matches!(next.map(|n| &n.kind), Some(TokenKind::Punct(Punct::LParen))) => {
                    let native = self.symbols.natives.contains(name);
                    SemanticToken::at(t, T::Function, Modifiers::NONE.with_if(native, M::DefaultLibrary))
                }
                // references
                _ => {
                    let modified = is_kw(prev, Kw::Set);
                    let local = scope.as_ref();
                    let (ty, mods) = if local.is_some_and(|s| s.params.contains(name)) {
                        (T::Parameter, Modifiers::NONE)
                    } else if local.is_some_and(|s| s.locals.contains(name)) {
                        (T::Variable, Modifiers::NONE)
                    } else if self.symbols.types.contains(name) || PRIMITIVES.contains(&name) {
                        out.push(self.type_token(t, name, false));
                        continue;
                    } else {
                        let readonly = self.symbols.constants.contains(name);
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
