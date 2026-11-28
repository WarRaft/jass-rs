//! Formatting JASS and vJASS: each line moved left or right to its level,
//! never a line added, removed or joined (see [`parser_rs::format`]).
//!
//! A JASS block is its keywords, one statement a line: a line starting
//! with `function`, `globals`, `if`, `loop` (and vJASS's `library`,
//! `scope`, `struct`, `interface`, `module`, `method`, `static if`) opens
//! one, its `end...` closes it, `else` and `elseif` stand at their `if`.
//! Modifiers before the keyword (`private`, `public`, `static`,
//! `constant`, `stub`, `debug`) don't matter. What has no body opens
//! nothing: `function interface`, and a method of an interface.
//!
//! A `//! zinc` block is formatted as Zinc at its directives' level. A
//! textmacro's, novjass's or inject's lines, text and not code of their
//! place, move with their opening directive and keep their own layout.
//! The lines a string goes on into stay as they are.

use parser_rs::format::{edits, line_count, literal_lines, Indent};
use parser_rs::{tokenize_lossy, Edit, Kw, TokenKind};

use crate::lexer::Dialect;

/// The blocks a JASS line can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Function,
    Globals,
    If,
    Loop,
    Library,
    Scope,
    Struct,
    Interface,
    Module,
    Method,
}

/// The block `kw` closes.
fn closes(kw: Kw) -> Option<Block> {
    Some(match kw {
        Kw::EndFunction => Block::Function,
        Kw::EndGlobals => Block::Globals,
        Kw::EndIf => Block::If,
        Kw::EndLoop => Block::Loop,
        Kw::EndLibrary => Block::Library,
        Kw::EndScope => Block::Scope,
        Kw::EndStruct => Block::Struct,
        Kw::EndInterface => Block::Interface,
        Kw::EndModule => Block::Module,
        Kw::EndMethod => Block::Method,
        _ => return None,
    })
}

/// A word before a declaration's or statement's own keyword.
fn is_modifier(kw: Kw) -> bool {
    matches!(kw, Kw::Private | Kw::Public | Kw::Static | Kw::Constant | Kw::Stub | Kw::Debug)
}

/// The level of each line of `src`, read as `dialect`.
pub fn indents(src: &str, dialect: Dialect) -> Vec<Indent> {
    let lexed = tokenize_lossy(src, dialect.spec());
    let mut indents = vec![Indent::Level(0); line_count(src)];
    literal_lines(&lexed, 1, &mut indents);
    let mut stack: Vec<Block> = Vec::new();
    let tokens: Vec<_> = lexed.tokens.iter().filter(|t| t.kind != TokenKind::Eof).collect();
    let mut next = 0;
    let mut line = 1;
    while line <= indents.len() {
        let start = next;
        while next < tokens.len() && tokens[next].line == line {
            next += 1;
        }
        let here = &tokens[start..next];
        let slot = line - 1;
        if let Some(TokenKind::Embedded(block)) = here.first().map(|t| &t.kind) {
            // Its directives at the level here, its lines Zinc's or its own.
            let depth = stack.len();
            let open = block.open.start_line;
            // The closing directive's line, or past the end.
            let end = block.close.map_or(indents.len() + 1, |c| c.start_line);
            indents[open - 1] = Indent::Level(depth);
            if let Some(c) = block.close {
                indents[c.start_line - 1] = Indent::Level(depth);
            }
            let inner: Vec<Indent> = if block.lang == "zinc" {
                zinc_rs::format::indents_at(&block.source, block.first_line, depth)
            } else {
                vec![Indent::Follow(open); line_count(&block.source)]
            };
            for (i, indent) in inner.into_iter().enumerate() {
                let l = block.first_line + i;
                if l < end && l <= indents.len() {
                    indents[l - 1] = indent;
                }
            }
            line = end.max(line + 1);
            if block.close.is_some() {
                line += 1;
            }
            while next < tokens.len() && tokens[next].line < line {
                next += 1;
            }
            continue;
        }
        if !matches!(indents[slot], Indent::Level(_)) {
            line += 1;
            continue;
        }
        let words: Vec<Kw> = here
            .iter()
            .map_while(|t| match t.kind {
                TokenKind::Keyword(k) => Some(k),
                _ => None,
            })
            .collect();
        let mut own = words.iter().copied().skip_while(|k| is_modifier(*k));
        let kw = own.next();
        let after = own.next();
        let level = match kw {
            Some(k) if closes(k).is_some() => {
                let block = closes(k).expect("just checked");
                if let Some(i) = stack.iter().rposition(|b| *b == block) {
                    stack.truncate(i);
                }
                stack.len()
            }
            Some(Kw::Else | Kw::ElseIf) if stack.last() == Some(&Block::If) => stack.len() - 1,
            _ => stack.len(),
        };
        indents[slot] = Indent::Level(level);
        let opens = match kw {
            Some(Kw::Function) if after != Some(Kw::Interface) => Some(Block::Function),
            Some(Kw::Globals) => Some(Block::Globals),
            Some(Kw::If) => Some(Block::If),
            Some(Kw::Loop) => Some(Block::Loop),
            Some(Kw::Library | Kw::LibraryOnce) => Some(Block::Library),
            Some(Kw::Scope) => Some(Block::Scope),
            Some(Kw::Struct) => Some(Block::Struct),
            Some(Kw::Interface) => Some(Block::Interface),
            Some(Kw::Module) => Some(Block::Module),
            Some(Kw::Method) if stack.last() != Some(&Block::Interface) => Some(Block::Method),
            _ => None,
        };
        stack.extend(opens);
        line += 1;
    }
    indents
}

/// The edits that indent `src`'s lines `lines` (1-based, inclusive), read
/// as `dialect`, each level `unit`.
pub fn format(src: &str, dialect: Dialect, unit: &str, lines: std::ops::RangeInclusive<usize>) -> Vec<Edit> {
    edits(src, &indents(src, dialect), unit, lines)
}

#[cfg(test)]
#[path = "format.test.rs"]
mod tests;
