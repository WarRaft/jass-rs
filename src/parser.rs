use crate::ast::*;
use crate::lexer::{Dialect, Lexer};
use crate::token::{Kw, Punct, Token, TokenKind};
use parser_rs::{TokenParser, TokenStream};

pub use parser_rs::ParseError;

pub struct Parser {
    ts: TokenStream<TokenKind>,
}

impl TokenParser for Parser {
    type Kind = TokenKind;

    fn tokens(&mut self) -> &mut TokenStream<TokenKind> {
        &mut self.ts
    }
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            ts: TokenStream::new(tokens),
        }
    }

    pub fn parse_str(src: &str) -> Result<Program, ParseError> {
        Parser::parse_str_with(src, Dialect::Jass)
    }

    pub fn parse_str_with(src: &str, dialect: Dialect) -> Result<Program, ParseError> {
        Parser::new(Lexer::tokenize_with(src, dialect)?).program()
    }

    fn peek(&self) -> &Token {
        self.ts.peek()
    }

    fn peek_kind(&self) -> &TokenKind {
        self.ts.peek_kind()
    }

    fn advance(&mut self) -> Token {
        self.ts.advance()
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.ts.check(kind)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        self.ts.error(message)
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        self.ts.expect(kind)
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(self.error(format!("expected identifier, found {other}"))),
        }
    }

    fn expect_type_name(&mut self) -> Result<String, ParseError> {
        self.expect_identifier()
    }

    pub fn program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            items.push(self.top_level()?);
        }
        Ok(Program { items })
    }

    fn top_level(&mut self) -> Result<TopLevel, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Keyword(Kw::Globals) => self.globals_block(),
            TokenKind::Keyword(Kw::Type) => self.type_def(),
            TokenKind::Keyword(Kw::Native) => {
                let line = self.peek().line;
                self.advance();
                let sig = self.function_sig(false, line)?;
                Ok(TopLevel::Native(sig))
            }
            TokenKind::Keyword(Kw::Constant) => match self.ts.nth_kind(1) {
                TokenKind::Keyword(Kw::Native) => {
                    let line = self.peek().line;
                    self.advance();
                    self.advance();
                    let sig = self.function_sig(true, line)?;
                    Ok(TopLevel::Native(sig))
                }
                TokenKind::Keyword(Kw::Function) => self.function_decl(),
                // `constant integer X = 1`: a top-level global
                _ => Ok(TopLevel::Global(self.global_decl()?)),
            },
            TokenKind::Keyword(Kw::Function) => self.function_decl(),
            // `integer x = 1` at the top level: a global, moved into the
            // `globals` block when the map is built.
            TokenKind::Ident(_) => Ok(TopLevel::Global(self.global_decl()?)),
            // Statements don't belong here, but they're read in full, so
            // lint can ask for them to be moved into a function and the
            // rest of the file still parses.
            TokenKind::Keyword(
                Kw::Local | Kw::Set | Kw::Call | Kw::If | Kw::Loop | Kw::ExitWhen | Kw::Return | Kw::Debug,
            ) => Ok(TopLevel::Stmt(self.stmt()?)),
            // Control passes to the Zinc parser for the whole block, with
            // file-absolute lines, and comes back here for the next token.
            TokenKind::Embedded(block) if block.lang == "zinc" => {
                self.advance();
                let program = zinc_rs::parse_at(&block.source, block.first_line)?;
                Ok(TopLevel::Zinc(ZincBlock {
                    program,
                    open: block.open,
                    close: block.close,
                }))
            }
            other => Err(self.error(format!("unexpected top-level token {other}"))),
        }
    }

    fn type_def(&mut self) -> Result<TopLevel, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Type))?;
        let name = self.expect_identifier()?;
        self.expect(TokenKind::Keyword(Kw::Extends))?;
        let extends = self.expect_type_name()?;
        Ok(TopLevel::TypeDef {
            name,
            extends,
            line,
        })
    }

    fn globals_block(&mut self) -> Result<TopLevel, ParseError> {
        self.expect(TokenKind::Keyword(Kw::Globals))?;
        let mut decls = Vec::new();
        while !self.check(&TokenKind::Keyword(Kw::EndGlobals)) {
            if self.check(&TokenKind::Eof) {
                return Err(self.error("unexpected end of file inside globals block"));
            }
            decls.push(self.global_decl()?);
        }
        self.expect(TokenKind::Keyword(Kw::EndGlobals))?;
        Ok(TopLevel::Globals(decls))
    }

    fn global_decl(&mut self) -> Result<GlobalDecl, ParseError> {
        let line = self.peek().line;
        let is_constant = if self.check(&TokenKind::Keyword(Kw::Constant)) {
            self.advance();
            true
        } else {
            false
        };
        let type_name = self.expect_type_name()?;
        let is_array = if self.check(&TokenKind::Keyword(Kw::Array)) {
            self.advance();
            true
        } else {
            false
        };
        let name = self.expect_identifier()?;
        let initializer = if self.check(&TokenKind::Punct(Punct::Assign)) {
            self.advance();
            Some(self.expr()?)
        } else {
            None
        };
        Ok(GlobalDecl {
            is_constant,
            type_name,
            is_array,
            name,
            initializer,
            line,
        })
    }

    fn function_sig(&mut self, is_constant: bool, line: usize) -> Result<FunctionSig, ParseError> {
        let name = self.expect_identifier()?;
        // `takes` and `returns` may be left out: `function F` is
        // `function F takes nothing returns nothing`.
        let has_takes = self.ts.eat(&TokenKind::Keyword(Kw::Takes));
        let params = if !has_takes {
            Vec::new()
        } else if self.check(&TokenKind::Keyword(Kw::Nothing)) {
            self.advance();
            Vec::new()
        } else {
            let mut params = vec![self.param()?];
            while self.check(&TokenKind::Punct(Punct::Comma)) {
                self.advance();
                params.push(self.param()?);
            }
            params
        };
        let has_returns = self.ts.eat(&TokenKind::Keyword(Kw::Returns));
        let returns = if !has_returns {
            None
        } else if self.check(&TokenKind::Keyword(Kw::Nothing)) {
            self.advance();
            None
        } else {
            Some(self.expect_type_name()?)
        };
        Ok(FunctionSig {
            name,
            params,
            returns,
            is_constant,
            line,
            has_takes,
            has_returns,
        })
    }

    fn param(&mut self) -> Result<Param, ParseError> {
        let type_name = self.expect_type_name()?;
        let name = self.expect_identifier()?;
        Ok(Param { type_name, name })
    }

    /// `[constant] function ... endfunction`.
    fn function_decl(&mut self) -> Result<TopLevel, ParseError> {
        let line = self.peek().line;
        let is_constant = self.ts.eat(&TokenKind::Keyword(Kw::Constant));
        self.expect(TokenKind::Keyword(Kw::Function))?;
        let sig = self.function_sig(is_constant, line)?;
        let body = self.stmt_block(&[TokenKind::Keyword(Kw::EndFunction)])?;
        self.expect(TokenKind::Keyword(Kw::EndFunction))?;
        Ok(TopLevel::Function(FunctionDecl { sig, body }))
    }

    fn stmt_block(&mut self, stop: &[TokenKind]) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !stop.contains(self.peek_kind()) {
            if self.check(&TokenKind::Eof) {
                return Err(self.error("unexpected end of file"));
            }
            stmts.push(self.stmt()?);
        }
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Keyword(Kw::Local) => self.local_stmt(),
            TokenKind::Keyword(Kw::Set) => self.set_stmt(),
            TokenKind::Keyword(Kw::Call) => self.call_stmt(),
            TokenKind::Keyword(Kw::If) => self.if_stmt(),
            TokenKind::Keyword(Kw::Loop) => self.loop_stmt(),
            TokenKind::Keyword(Kw::ExitWhen) => self.exitwhen_stmt(),
            TokenKind::Keyword(Kw::Return) => self.return_stmt(),
            TokenKind::Keyword(Kw::Debug) => self.debug_stmt(),
            other => Err(self.error(format!("unexpected token in statement position: {other}"))),
        }
    }

    /// `debug` before a `set`, `call`, `if` or `loop`: nothing else can be
    /// skipped outside debug mode.
    fn debug_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Debug))?;
        match self.peek_kind() {
            TokenKind::Keyword(Kw::Set | Kw::Call | Kw::If | Kw::Loop) => {
                let stmt = Box::new(self.stmt()?);
                Ok(Stmt::Debug { stmt, line })
            }
            other => {
                let other = other.clone();
                Err(self.error(format!(
                    "'debug' can only precede 'set', 'call', 'if' or 'loop', found {other}"
                )))
            }
        }
    }

    fn local_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Local))?;
        let type_name = self.expect_type_name()?;
        let is_array = if self.check(&TokenKind::Keyword(Kw::Array)) {
            self.advance();
            true
        } else {
            false
        };
        let name = self.expect_identifier()?;
        let initializer = if self.check(&TokenKind::Punct(Punct::Assign)) {
            self.advance();
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Stmt::Local {
            type_name,
            is_array,
            name,
            initializer,
            line,
        })
    }

    fn set_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Set))?;
        let name = self.expect_identifier()?;
        let index = if self.check(&TokenKind::Punct(Punct::LBracket)) {
            self.advance();
            let idx = self.expr()?;
            self.expect(TokenKind::Punct(Punct::RBracket))?;
            Some(idx)
        } else {
            None
        };
        self.expect(TokenKind::Punct(Punct::Assign))?;
        let value = self.expr()?;
        Ok(Stmt::Set {
            name,
            index,
            value,
            line,
        })
    }

    fn call_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Call))?;
        let name = self.expect_identifier()?;
        self.expect(TokenKind::Punct(Punct::LParen))?;
        let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
        self.expect(TokenKind::Punct(Punct::RParen))?;
        Ok(Stmt::Call { name, args, line })
    }

    fn if_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::If))?;
        let mut branches = Vec::new();

        let cond = self.expr()?;
        self.expect(TokenKind::Keyword(Kw::Then))?;
        let body = self.stmt_block(&[TokenKind::Keyword(Kw::ElseIf), TokenKind::Keyword(Kw::Else), TokenKind::Keyword(Kw::EndIf)])?;
        branches.push((cond, body));

        let mut else_branch = None;
        loop {
            match self.peek_kind() {
                TokenKind::Keyword(Kw::ElseIf) => {
                    self.advance();
                    let cond = self.expr()?;
                    self.expect(TokenKind::Keyword(Kw::Then))?;
                    let body =
                        self.stmt_block(&[TokenKind::Keyword(Kw::ElseIf), TokenKind::Keyword(Kw::Else), TokenKind::Keyword(Kw::EndIf)])?;
                    branches.push((cond, body));
                }
                TokenKind::Keyword(Kw::Else) => {
                    self.advance();
                    let body = self.stmt_block(&[TokenKind::Keyword(Kw::EndIf)])?;
                    else_branch = Some(body);
                    break;
                }
                TokenKind::Keyword(Kw::EndIf) => break,
                _ => return Err(self.error("expected 'elseif', 'else', or 'endif'")),
            }
        }
        self.expect(TokenKind::Keyword(Kw::EndIf))?;
        Ok(Stmt::If {
            branches,
            else_branch,
            line,
        })
    }

    fn loop_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Loop))?;
        let body = self.stmt_block(&[TokenKind::Keyword(Kw::EndLoop)])?;
        self.expect(TokenKind::Keyword(Kw::EndLoop))?;
        Ok(Stmt::Loop { body, line })
    }

    fn exitwhen_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::ExitWhen))?;
        let cond = self.expr()?;
        Ok(Stmt::ExitWhen { cond, line })
    }

    fn return_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Return))?;
        let value = if self.starts_expr() {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Stmt::Return { value, line })
    }

    fn starts_expr(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Ident(_)
                | TokenKind::Int(_)
                | TokenKind::Real(_)
                | TokenKind::Str(_)
                | TokenKind::Keyword(Kw::True)
                | TokenKind::Keyword(Kw::False)
                | TokenKind::Keyword(Kw::Null)
                | TokenKind::Punct(Punct::LParen)
                | TokenKind::Punct(Punct::Minus)
                | TokenKind::Punct(Punct::Plus)
                | TokenKind::Keyword(Kw::Not)
                | TokenKind::Keyword(Kw::Function)
        )
    }

    fn expr_list(&mut self, end: &TokenKind) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek_kind() == end {
            return Ok(args);
        }
        args.push(self.expr()?);
        while self.check(&TokenKind::Punct(Punct::Comma)) {
            self.advance();
            args.push(self.expr()?);
        }
        Ok(args)
    }

    pub fn expr(&mut self) -> Result<Expr, ParseError> {
        self.and_expr()
    }

    /// Unlike C-family languages, JASS gives `and` *lower* precedence than
    /// `or`: `false and true or true` is `false and (true or true)`, i.e.
    /// `false`.
    fn and_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::or_expr,
            |k| matches!(k, TokenKind::Keyword(Kw::And)).then_some(BinOp::And),
            binary,
        )
    }

    fn or_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::equality_expr,
            |k| matches!(k, TokenKind::Keyword(Kw::Or)).then_some(BinOp::Or),
            binary,
        )
    }

    fn equality_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::relational_expr,
            |k| match k {
                TokenKind::Punct(Punct::EqEq) => Some(BinOp::Eq),
                TokenKind::Punct(Punct::NotEq) => Some(BinOp::NotEq),
                _ => None,
            },
            binary,
        )
    }

    fn relational_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::additive_expr,
            |k| match k {
                TokenKind::Punct(Punct::Gt) => Some(BinOp::Gt),
                TokenKind::Punct(Punct::Lt) => Some(BinOp::Lt),
                TokenKind::Punct(Punct::GtEq) => Some(BinOp::GtEq),
                TokenKind::Punct(Punct::LtEq) => Some(BinOp::LtEq),
                _ => None,
            },
            binary,
        )
    }

    fn additive_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::multiplicative_expr,
            |k| match k {
                TokenKind::Punct(Punct::Plus) => Some(BinOp::Add),
                TokenKind::Punct(Punct::Minus) => Some(BinOp::Sub),
                _ => None,
            },
            binary,
        )
    }

    fn multiplicative_expr(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(
            Self::unary_expr,
            |k| match k {
                TokenKind::Punct(Punct::Star) => Some(BinOp::Mul),
                TokenKind::Punct(Punct::Slash) => Some(BinOp::Div),
                _ => None,
            },
            binary,
        )
    }

    fn unary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind() {
            TokenKind::Punct(Punct::Minus) => {
                let op_tok = self.advance();
                let e = self.unary_expr()?;
                let span = Span::new(op_tok.line, op_tok.col, e.span.end_line, e.span.end_col);
                Ok(Expr::new(ExprKind::Unary(UnaryOp::Neg, Box::new(e)), span))
            }
            TokenKind::Punct(Punct::Plus) => {
                let op_tok = self.advance();
                let e = self.unary_expr()?;
                let span = Span::new(op_tok.line, op_tok.col, e.span.end_line, e.span.end_col);
                Ok(Expr::new(ExprKind::Unary(UnaryOp::Plus, Box::new(e)), span))
            }
            TokenKind::Keyword(Kw::Not) => {
                let op_tok = self.advance();
                let e = self.unary_expr()?;
                let span = Span::new(op_tok.line, op_tok.col, e.span.end_line, e.span.end_col);
                Ok(Expr::new(ExprKind::Unary(UnaryOp::Not, Box::new(e)), span))
            }
            _ => self.primary_expr(),
        }
    }

    fn primary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Int(v) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::IntLiteral(v), tok_span(&tok)))
            }
            TokenKind::Real(v) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::RealLiteral(v), tok_span(&tok)))
            }
            TokenKind::Str(v) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::StringLiteral(v), tok_span(&tok)))
            }
            TokenKind::Keyword(Kw::True) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::BoolLiteral(true), tok_span(&tok)))
            }
            TokenKind::Keyword(Kw::False) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::BoolLiteral(false), tok_span(&tok)))
            }
            TokenKind::Keyword(Kw::Null) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::Null, tok_span(&tok)))
            }
            TokenKind::Punct(Punct::LParen) => {
                let open = self.advance();
                let e = self.expr()?;
                let close = self.expect(TokenKind::Punct(Punct::RParen))?;
                let span = Span::new(open.line, open.col, close.line, close.end_col);
                Ok(Expr::new(e.kind, span))
            }
            TokenKind::Keyword(Kw::Function) => {
                let start = self.advance();
                let ident_tok = self.peek().clone();
                let name = self.expect_identifier()?;
                let span = Span::new(start.line, start.col, ident_tok.line, ident_tok.end_col);
                Ok(Expr::new(ExprKind::FuncRef(name), span))
            }
            TokenKind::Ident(name) => {
                let tok = self.advance();
                if self.check(&TokenKind::Punct(Punct::LParen)) {
                    self.advance();
                    let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
                    let close = self.expect(TokenKind::Punct(Punct::RParen))?;
                    let span = Span::new(tok.line, tok.col, close.line, close.end_col);
                    Ok(Expr::new(ExprKind::Call(name, args), span))
                } else if self.check(&TokenKind::Punct(Punct::LBracket)) {
                    self.advance();
                    let idx = self.expr()?;
                    let close = self.expect(TokenKind::Punct(Punct::RBracket))?;
                    let span = Span::new(tok.line, tok.col, close.line, close.end_col);
                    Ok(Expr::new(ExprKind::ArrayAccess(name, Box::new(idx)), span))
                } else {
                    Ok(Expr::new(ExprKind::Var(name), tok_span(&tok)))
                }
            }
            other => Err(self.error(format!("unexpected token in expression: {other}"))),
        }
    }
}

fn tok_span(tok: &Token) -> Span {
    tok.span()
}

fn binary(left: Expr, op: BinOp, right: Expr) -> Expr {
    let span = left.span.join(right.span);
    Expr::new(ExprKind::Binary(Box::new(left), op, Box::new(right)), span)
}

#[cfg(test)]
#[path = "parser.test.rs"]
mod tests;
