//! Strict JASS checked as the game reads a map's script: one file. Some
//! makers build their maps with tools of their own, from several files
//! put together in an order; the game has no imports, so those files are
//! read as the one file they make, glued in the order given (each ending
//! its line), and every problem is said at its own file and line.
//!
//! What's found: a parse error (the game would read no further), every
//! lint rule (the group `strict`'s always errors: what the game rejects),
//! and the names over `env`, the game's declarations (`common.j`,
//! `Blizzard.j`), which come before the script and aren't part of it.

use crate::names::Declarations;
use crate::{Dialect, Diagnostic, Level, Span};

/// A problem, at its file (an index into the files given) and its line
/// there (and span, when the rule knows one).
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub file: usize,
    pub diagnostic: Diagnostic,
}

/// Is `rule` one of the group `strict`'s?
pub fn in_strict_group(rule: &str) -> bool {
    parser_rs::find_rule(rule).is_some_and(|r| r.groups.contains(&parser_rs::group::STRICT))
}

/// `files` (name, text) glued in their order and checked as strict JASS
/// over `env`. A rule off by default that isn't the group `strict`'s is
/// left out; the caller applies a project's choices to the rest.
pub fn check(files: &[(&str, &str)], env: &[&Declarations]) -> Vec<Found> {
    check_with(files, env, &crate::lint::Options::default())
}

/// [`check`], with the lint's `options` (its limits; the source is the
/// files glued).
pub fn check_with(files: &[(&str, &str)], env: &[&Declarations], options: &crate::lint::Options) -> Vec<Found> {
    let mut text = String::new();
    // The first line of each file in the whole.
    let mut starts = Vec::with_capacity(files.len());
    let mut lines = 0;
    for (_, t) in files {
        starts.push(lines + 1);
        text.push_str(t);
        let ends_broken = t.ends_with(parser_rs::lines::is_break);
        if !t.is_empty() && !ends_broken {
            text.push('\n');
        }
        // Its lines, every break counted (`parser_rs::lines`).
        lines += parser_rs::lines::line_count(t) - 1 + usize::from(!t.is_empty() && !ends_broken);
    }
    let place = |line: usize| -> (usize, usize) {
        let file = starts.iter().rposition(|&s| s <= line).unwrap_or(0);
        (file, line + 1 - starts.get(file).copied().unwrap_or(1))
    };
    // A message's "line N" (of the whole) said as the file's: "b.j:3".
    let relabel = |message: &str| -> String {
        if files.len() < 2 {
            return message.to_string();
        }
        let mut out = String::new();
        let mut rest = message;
        while let Some(at) = rest.find("line ") {
            let digits: String = rest[at + 5..].chars().take_while(char::is_ascii_digit).collect();
            out.push_str(&rest[..at]);
            match digits.parse::<usize>() {
                Ok(n) => {
                    let (file, line) = place(n);
                    out.push_str(&format!("{}:{line}", files[file].0));
                }
                Err(_) => out.push_str("line "),
            }
            rest = &rest[at + 5 + digits.len()..];
        }
        out.push_str(rest);
        out
    };
    let found = |mut d: Diagnostic| -> Found {
        let (file, line) = place(d.line);
        d.line = line;
        d.message = relabel(&d.message);
        if let Some(s) = d.span {
            let (_, end) = place(s.end_line);
            d.span = Some(Span { start_line: line, end_line: end, ..s });
        }
        Found { file, diagnostic: d }
    };
    let program = match crate::parse_with(&text, Dialect::Strict) {
        Ok(p) => p,
        Err(e) => {
            let d = Diagnostic::error("", e.message.clone(), e.line).at(Span::new(e.line, e.col, e.line, e.end_col.max(e.col + 1)));
            return vec![found(d)];
        }
    };
    let options = crate::lint::Options { source: Some(&text), ..*options };
    let mut out: Vec<Found> = crate::lint_with(&program, &options)
        .into_iter()
        .filter_map(|mut d| {
            if in_strict_group(d.rule) {
                d.level = Level::Error;
                return Some(d);
            }
            parser_rs::find_rule(d.rule).is_none_or(|r| r.enabled).then_some(d)
        })
        .chain(crate::names::check(&program, env))
        .map(found)
        .collect();
    out.sort_by_key(|f| (f.file, f.diagnostic.line));
    out
}

#[cfg(test)]
#[path = "strict.test.rs"]
mod tests;
