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
        // A textmacro's or novjass's block isn't code of its place.
        let tokens = Lexer::tokenize_with(src, dialect)?.into_iter().filter(|t| !matches!(&t.kind, TokenKind::Embedded(b) if b.lang != "zinc")).collect();
        Parser::new(tokens).program()
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
        // vJASS: `thistype`, the struct a member is in.
        if self.ts.eat(&TokenKind::Keyword(Kw::ThisType)) {
            return Ok("thistype".into());
        }
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
        // vJASS: `private`, `public` ... before a declaration.
        if self.starts_modifiers() {
            let line = self.peek().line;
            let modifiers = self.modifiers();
            return self.declaration(modifiers, line);
        }
        match self.peek_kind().clone() {
            TokenKind::Keyword(Kw::Globals) => self.globals_block(),
            TokenKind::Keyword(Kw::Native) => {
                let line = self.peek().line;
                self.advance();
                let sig = self.function_sig(false, line)?;
                Ok(TopLevel::Native(sig))
            }
            TokenKind::Keyword(Kw::Constant) if matches!(self.ts.nth_kind(1), TokenKind::Keyword(Kw::Native)) => {
                let line = self.peek().line;
                self.advance();
                self.advance();
                let sig = self.function_sig(true, line)?;
                Ok(TopLevel::Native(sig))
            }
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
            _ => {
                let line = self.peek().line;
                self.declaration(Modifiers::default(), line)
            }
        }
    }

    /// Whether a vJASS modifier comes next.
    fn starts_modifiers(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Keyword(Kw::Private | Kw::Public | Kw::Static | Kw::Readonly | Kw::Stub | Kw::Delegate)
        )
    }

    /// vJASS's modifiers, in any order (none in plain JASS: they aren't
    /// its keywords).
    fn modifiers(&mut self) -> Modifiers {
        let mut m = Modifiers::default();
        loop {
            match self.peek_kind() {
                TokenKind::Keyword(Kw::Private) => m.access = Access::Private,
                TokenKind::Keyword(Kw::Public) => m.access = Access::Public,
                TokenKind::Keyword(Kw::Static) => m.is_static = true,
                TokenKind::Keyword(Kw::Readonly) => m.is_readonly = true,
                TokenKind::Keyword(Kw::Stub) => m.is_stub = true,
                TokenKind::Keyword(Kw::Delegate) => m.is_delegate = true,
                _ => return m,
            }
            self.advance();
        }
    }

    /// A declaration at the top level or in a library, after its
    /// modifiers.
    fn declaration(&mut self, modifiers: Modifiers, line: usize) -> Result<TopLevel, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Keyword(Kw::Type) => self.type_def(modifiers),
            TokenKind::Keyword(Kw::Function) if matches!(self.ts.nth_kind(1), TokenKind::Keyword(Kw::Interface)) => {
                self.advance();
                self.advance();
                let mut sig = self.function_sig(false, line)?;
                sig.modifiers = modifiers;
                Ok(TopLevel::FunctionInterface(sig))
            }
            TokenKind::Keyword(Kw::Function) => self.function_decl(modifiers, line),
            TokenKind::Keyword(Kw::Constant) if matches!(self.ts.nth_kind(1), TokenKind::Keyword(Kw::Function)) => {
                self.function_decl(modifiers, line)
            }
            TokenKind::Keyword(Kw::Library | Kw::LibraryOnce | Kw::Scope) => self.library(),
            TokenKind::Keyword(Kw::Struct) => self.struct_decl(modifiers, line),
            TokenKind::Keyword(Kw::Interface) => {
                let (name, members, end_line) = self.type_body(Kw::Interface, Kw::EndInterface)?;
                Ok(TopLevel::Interface(TypeDecl { modifiers, name, members, line, end_line }))
            }
            TokenKind::Keyword(Kw::Module) => {
                let (name, members, end_line) = self.type_body(Kw::Module, Kw::EndModule)?;
                Ok(TopLevel::Module(TypeDecl { modifiers, name, members, line, end_line }))
            }
            TokenKind::Keyword(Kw::Keyword) => {
                self.advance();
                let name = self.expect_identifier()?;
                Ok(TopLevel::Keyword { modifiers, name, line })
            }
            TokenKind::Keyword(Kw::Hook) => {
                self.advance();
                let name = self.expect_identifier()?;
                let mut target = self.expect_identifier()?;
                if self.ts.eat(&TokenKind::Punct(Punct::Dot)) {
                    target = format!("{target}.{}", self.expect_identifier()?);
                }
                Ok(TopLevel::Hook { name, target, line })
            }
            // `integer x = 1`, `constant integer X = 1` at the top level: a
            // global, moved into the `globals` block when the map is built.
            TokenKind::Ident(_) | TokenKind::Keyword(Kw::Constant) => {
                let mut g = self.global_decl()?;
                g.modifiers = modifiers;
                Ok(TopLevel::Global(g))
            }
            other => Err(self.error(format!("unexpected top-level token {other}"))),
        }
    }

    /// `library`, `library_once` or `scope`: its name, `initializer` and
    /// `requires` (`needs`, `uses`) in any order, its items, its end.
    fn library(&mut self) -> Result<TopLevel, ParseError> {
        let open = self.advance();
        let line = open.line;
        let (scope, once) = match open.kind {
            TokenKind::Keyword(Kw::Scope) => (true, false),
            TokenKind::Keyword(Kw::LibraryOnce) => (false, true),
            _ => (false, false),
        };
        let name = self.expect_identifier()?;
        let mut initializer = None;
        let mut requires = Vec::new();
        loop {
            match self.peek_kind() {
                TokenKind::Keyword(Kw::Initializer) => {
                    self.advance();
                    initializer = Some(self.expect_identifier()?);
                }
                TokenKind::Keyword(Kw::Requires | Kw::Needs | Kw::Uses) => {
                    self.advance();
                    loop {
                        let optional = self.ts.eat(&TokenKind::Keyword(Kw::Optional));
                        requires.push((self.expect_identifier()?, optional));
                        if !self.ts.eat(&TokenKind::Punct(Punct::Comma)) {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        let end = if scope { Kw::EndScope } else { Kw::EndLibrary };
        let mut items = Vec::new();
        while !self.check(&TokenKind::Keyword(end)) {
            if self.check(&TokenKind::Eof) {
                return Err(self.error(format!("unexpected end of file inside {} {name}", if scope { "scope" } else { "library" })));
            }
            items.push(self.top_level()?);
        }
        let end_line = self.advance().line;
        Ok(TopLevel::Library(LibraryDecl { name, scope, once, initializer, requires, items, line, end_line }))
    }

    /// `struct Name[size] extends Parent` ... `endstruct`.
    fn struct_decl(&mut self, modifiers: Modifiers, line: usize) -> Result<TopLevel, ParseError> {
        self.expect(TokenKind::Keyword(Kw::Struct))?;
        let name = self.expect_identifier()?;
        let size = if self.ts.eat(&TokenKind::Punct(Punct::LBracket)) {
            let size = self.expr()?;
            self.expect(TokenKind::Punct(Punct::RBracket))?;
            Some(size)
        } else {
            None
        };
        let extends = if self.ts.eat(&TokenKind::Keyword(Kw::Extends)) {
            if self.ts.eat(&TokenKind::Keyword(Kw::Array)) { Some("array".to_string()) } else { Some(self.expect_identifier()?) }
        } else {
            None
        };
        let members = self.members(Kw::EndStruct)?;
        let end_line = self.advance().line;
        Ok(TopLevel::Struct(StructDecl { modifiers, name, size, extends, members, line, end_line }))
    }

    /// An interface's or a module's name, members and end line.
    fn type_body(&mut self, open: Kw, end: Kw) -> Result<(String, Vec<StructMember>, usize), ParseError> {
        self.expect(TokenKind::Keyword(open))?;
        let name = self.expect_identifier()?;
        let members = self.members(end)?;
        let end_line = self.advance().line;
        Ok((name, members, end_line))
    }

    /// A struct's, interface's or module's members, up to `end`.
    fn members(&mut self, end: Kw) -> Result<Vec<StructMember>, ParseError> {
        let mut members = Vec::new();
        while !self.check(&TokenKind::Keyword(end)) {
            if self.check(&TokenKind::Eof) {
                return Err(self.error(format!("unexpected end of file: expected {}", TokenKind::Keyword(end))));
            }
            let line = self.peek().line;
            if self.ts.eat(&TokenKind::Keyword(Kw::Implement)) {
                let optional = self.ts.eat(&TokenKind::Keyword(Kw::Optional));
                let name = self.expect_identifier()?;
                members.push(StructMember::Implement { name, optional, line });
                continue;
            }
            let modifiers = self.modifiers();
            if self.check(&TokenKind::Keyword(Kw::Method)) {
                members.push(StructMember::Method(self.method(modifiers, line)?));
            } else {
                let mut g = self.global_decl()?;
                g.modifiers = modifiers;
                members.push(StructMember::Field(g));
            }
        }
        Ok(members)
    }

    /// `method [operator] name takes ... returns ...`, then its body to
    /// `endmethod`, or (an interface's) none, maybe `defaults` a value.
    fn method(&mut self, modifiers: Modifiers, line: usize) -> Result<MethodDecl, ParseError> {
        self.expect(TokenKind::Keyword(Kw::Method))?;
        let name = if self.ts.eat(&TokenKind::Keyword(Kw::Operator)) { format!("operator {}", self.operator_name()?) } else { self.expect_identifier()? };
        let mut sig = self.sig_after_name(name, false, line)?;
        sig.modifiers = modifiers;
        let defaults = if self.ts.eat(&TokenKind::Keyword(Kw::Defaults)) { Some(self.expr()?) } else { None };
        // A body unless the next line is no statement (an interface's).
        let body = if defaults.is_none() && self.method_has_body() {
            let body = self.stmt_block(&[TokenKind::Keyword(Kw::EndMethod)])?;
            self.expect(TokenKind::Keyword(Kw::EndMethod))?;
            Some(body)
        } else {
            None
        };
        Ok(MethodDecl { sig, body, defaults })
    }

    /// Whether a method has a body: an `endmethod` comes before the next
    /// member (an interface's methods have none).
    fn method_has_body(&self) -> bool {
        let mut i = 0;
        loop {
            match self.ts.nth_kind(i) {
                TokenKind::Keyword(Kw::EndMethod) => return true,
                TokenKind::Keyword(Kw::Method | Kw::EndInterface | Kw::EndStruct | Kw::EndModule | Kw::Implement) | TokenKind::Eof => {
                    return false;
                }
                _ => i += 1,
            }
        }
    }

    /// An operator's name: `[]`, `[]=`, `<`, `x`, `x=`.
    fn operator_name(&mut self) -> Result<String, ParseError> {
        let mut name = match self.peek_kind().clone() {
            TokenKind::Punct(Punct::LBracket) => {
                self.advance();
                self.expect(TokenKind::Punct(Punct::RBracket))?;
                "[]".to_string()
            }
            TokenKind::Punct(Punct::Lt) => {
                self.advance();
                return Ok("<".into());
            }
            TokenKind::Punct(Punct::Gt) => {
                self.advance();
                return Ok(">".into());
            }
            TokenKind::Ident(n) => {
                self.advance();
                n
            }
            other => return Err(self.error(format!("expected an operator, found {other}"))),
        };
        if self.ts.eat(&TokenKind::Punct(Punct::Assign)) {
            name.push('=');
        }
        Ok(name)
    }

    fn type_def(&mut self, modifiers: Modifiers) -> Result<TopLevel, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Type))?;
        let name = self.expect_identifier()?;
        self.expect(TokenKind::Keyword(Kw::Extends))?;
        let extends = self.expect_type_name()?;
        // vJASS: `extends integer array [16]`.
        let size = if self.ts.eat(&TokenKind::Keyword(Kw::Array)) {
            self.expect(TokenKind::Punct(Punct::LBracket))?;
            let size = self.expr()?;
            self.expect(TokenKind::Punct(Punct::RBracket))?;
            Some(size)
        } else {
            None
        };
        Ok(TopLevel::TypeDef {
            name,
            extends,
            line,
            size,
            modifiers,
        })
    }

    fn globals_block(&mut self) -> Result<TopLevel, ParseError> {
        self.expect(TokenKind::Keyword(Kw::Globals))?;
        let mut decls = Vec::new();
        while !self.check(&TokenKind::Keyword(Kw::EndGlobals)) {
            if self.check(&TokenKind::Eof) {
                return Err(self.error("unexpected end of file inside globals block"));
            }
            let modifiers = self.modifiers();
            let mut g = self.global_decl()?;
            g.modifiers = modifiers;
            decls.push(g);
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
        // vJASS: a sized array, `integer array a[100]`.
        let size = if is_array && self.ts.eat(&TokenKind::Punct(Punct::LBracket)) {
            let size = self.expr()?;
            self.expect(TokenKind::Punct(Punct::RBracket))?;
            Some(size)
        } else {
            None
        };
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
            modifiers: Modifiers::default(),
            size,
        })
    }

    fn function_sig(&mut self, is_constant: bool, line: usize) -> Result<FunctionSig, ParseError> {
        let name = self.expect_identifier()?;
        self.sig_after_name(name, is_constant, line)
    }

    /// `takes ... returns ...` after a function's or method's name.
    fn sig_after_name(&mut self, name: String, is_constant: bool, line: usize) -> Result<FunctionSig, ParseError> {
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
            modifiers: Modifiers::default(),
        })
    }

    fn param(&mut self) -> Result<Param, ParseError> {
        let type_name = self.expect_type_name()?;
        let name = self.expect_identifier()?;
        Ok(Param { type_name, name })
    }

    /// `[constant] function ... endfunction`.
    fn function_decl(&mut self, modifiers: Modifiers, line: usize) -> Result<TopLevel, ParseError> {
        let is_constant = self.ts.eat(&TokenKind::Keyword(Kw::Constant));
        self.expect(TokenKind::Keyword(Kw::Function))?;
        let mut sig = self.function_sig(is_constant, line)?;
        sig.modifiers = modifiers;
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
            TokenKind::Keyword(Kw::Static) if matches!(self.ts.nth_kind(1), TokenKind::Keyword(Kw::If)) => self.static_if(),
            // vJASS: `this.x = 1`, `.x = 1`, `a.b()`: `set` or `call` left out.
            TokenKind::Keyword(Kw::This | Kw::ThisType | Kw::Super) | TokenKind::Punct(Punct::Dot) => {
                let line = self.peek().line;
                self.member_stmt(line, false)
            }
            TokenKind::Ident(_) if self.starts_member() => {
                let line = self.peek().line;
                self.member_stmt(line, false)
            }
            // `x = 1`, `x[i] = 1`, `Foo()`: `set` or `call` left out.
            TokenKind::Ident(_) => self.bare_stmt(),
            other => Err(self.error(format!("unexpected token in statement position: {other}"))),
        }
    }

    /// An assignment, a call or a local without its `set`, `call` or
    /// `local`, which this toolchain reads and the game doesn't
    /// (`implicit-keyword`): a name, then `(` for a call, `=` or `[` for an
    /// assignment, a name or `array` for a local (its type, then it; a
    /// function can't declare a global).
    fn bare_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        match self.ts.nth_kind(1) {
            TokenKind::Punct(Punct::LParen) => {
                let name = self.expect_identifier()?;
                self.expect(TokenKind::Punct(Punct::LParen))?;
                let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
                self.expect(TokenKind::Punct(Punct::RParen))?;
                Ok(Stmt::Call { name, args, line, keyword: false })
            }
            TokenKind::Punct(Punct::Assign | Punct::LBracket) => self.assignment(line, false),
            // `integer i = 0`, `unit array u`: in a function, a local.
            TokenKind::Ident(_) | TokenKind::Keyword(Kw::Array) => self.local_decl(line, false),
            _ => {
                let name = self.expect_identifier()?;
                Err(self.error(format!(
                    "'{name}' starts no statement: an assignment ('{name} = ...'), a call ('{name}(...)') or a local ('{name} x')"
                )))
            }
        }
    }

    /// `debug` before a `set`, `call`, `if` or `loop`: nothing else can be
    /// skipped outside debug mode.
    fn debug_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Debug))?;
        match self.peek_kind() {
            // A name: an assignment or a call, `set` or `call` left out.
            TokenKind::Ident(_)
                if !matches!(self.ts.nth_kind(1), TokenKind::Punct(Punct::Assign | Punct::LBracket | Punct::LParen)) =>
            {
                Err(self.error("'debug' can only precede 'set', 'call', 'if' or 'loop'"))
            }
            TokenKind::Keyword(Kw::Set | Kw::Call | Kw::If | Kw::Loop) | TokenKind::Ident(_) => {
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
        self.local_decl(line, true)
    }

    /// `type name [= value]` or `type array name`, after `local` if `keyword`.
    fn local_decl(&mut self, line: usize, keyword: bool) -> Result<Stmt, ParseError> {
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
            keyword,
        })
    }

    fn set_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Set))?;
        if self.starts_member() {
            return self.member_stmt(line, true);
        }
        self.assignment(line, true)
    }

    /// Whether a vJASS member comes next (`.x`, `this`, `a.b`, `a[i].b`).
    fn starts_member(&self) -> bool {
        match self.peek_kind() {
            TokenKind::Keyword(Kw::This | Kw::ThisType | Kw::Super) | TokenKind::Punct(Punct::Dot) => true,
            TokenKind::Ident(_) => match self.ts.nth_kind(1) {
                TokenKind::Punct(Punct::Dot) => true,
                // `a[i].b`: past the index.
                TokenKind::Punct(Punct::LBracket) => {
                    let mut depth = 0;
                    let mut i = 1;
                    loop {
                        match self.ts.nth_kind(i) {
                            TokenKind::Punct(Punct::LBracket) => depth += 1,
                            TokenKind::Punct(Punct::RBracket) => {
                                depth -= 1;
                                if depth == 0 {
                                    return matches!(self.ts.nth_kind(i + 1), TokenKind::Punct(Punct::Dot));
                                }
                            }
                            TokenKind::Eof => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// vJASS: a member set (`a.b = v`, `a.b[i] = v`) or a method called
    /// (`a.b()`), after `set` or `call` if `keyword`.
    fn member_stmt(&mut self, line: usize, keyword: bool) -> Result<Stmt, ParseError> {
        let target = self.unary_expr()?;
        if self.ts.eat(&TokenKind::Punct(Punct::Assign)) {
            let value = self.expr()?;
            return match target.kind {
                ExprKind::Member(..) => Ok(Stmt::SetMember { target, index: None, value, line, keyword }),
                ExprKind::Index(member, index) if matches!(member.kind, ExprKind::Member(..)) => {
                    Ok(Stmt::SetMember { target: *member, index: Some(*index), value, line, keyword })
                }
                ExprKind::Var(name) => Ok(Stmt::Set { name, index: None, value, line, keyword }),
                ExprKind::ArrayAccess(name, index) => Ok(Stmt::Set { name, index: Some(*index), value, line, keyword }),
                _ => Err(self.error("only a name, a member or an element of one can be set")),
            };
        }
        match target.kind {
            ExprKind::MethodCall(..) => Ok(Stmt::CallMethod { call: target, line, keyword }),
            ExprKind::Call(name, args) => Ok(Stmt::Call { name, args, line, keyword }),
            _ => Err(self.error("expected '=' or a call")),
        }
    }

    /// vJASS: `static if cond then ... [elseif ...] [else ...] endif`.
    fn static_if(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Static))?;
        match self.if_stmt()? {
            Stmt::If { branches, else_branch, end_line, .. } => Ok(Stmt::StaticIf { branches, else_branch, line, end_line }),
            _ => unreachable!("if_stmt reads an if"),
        }
    }

    /// `name = value` or `name[index] = value`, after `set` if `keyword`.
    fn assignment(&mut self, line: usize, keyword: bool) -> Result<Stmt, ParseError> {
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
            keyword,
        })
    }

    fn call_stmt(&mut self) -> Result<Stmt, ParseError> {
        let line = self.peek().line;
        self.expect(TokenKind::Keyword(Kw::Call))?;
        if self.starts_member() {
            return self.member_stmt(line, true);
        }
        let name = self.expect_identifier()?;
        self.expect(TokenKind::Punct(Punct::LParen))?;
        let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
        self.expect(TokenKind::Punct(Punct::RParen))?;
        Ok(Stmt::Call { name, args, line, keyword: true })
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
        let mut else_span = None;
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
                    else_span = Some(self.advance().span());
                    let body = self.stmt_block(&[TokenKind::Keyword(Kw::EndIf)])?;
                    else_branch = Some(body);
                    break;
                }
                TokenKind::Keyword(Kw::EndIf) => break,
                _ => return Err(self.error("expected 'elseif', 'else', or 'endif'")),
            }
        }
        let end_line = self.expect(TokenKind::Keyword(Kw::EndIf))?.line;
        Ok(Stmt::If {
            branches,
            else_branch,
            line,
            else_span,
            end_line,
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
        // On its line: a name on the next is a statement of its own
        // (`set` and `call` may be left out).
        let value = if self.starts_expr() && self.peek().line == line {
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
                | TokenKind::Keyword(Kw::This)
                | TokenKind::Keyword(Kw::ThisType)
                | TokenKind::Keyword(Kw::Super)
                | TokenKind::Punct(Punct::Dot)
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
            _ => {
                let e = self.primary_expr()?;
                self.postfix(e)
            }
        }
    }

    /// vJASS: what follows an operand: `.name`, `.name(args)`, `[index]`.
    fn postfix(&mut self, mut e: Expr) -> Result<Expr, ParseError> {
        loop {
            if self.ts.eat(&TokenKind::Punct(Punct::Dot)) {
                let tok = self.peek().clone();
                let name = self.expect_identifier()?;
                if self.ts.eat(&TokenKind::Punct(Punct::LParen)) {
                    let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
                    let close = self.expect(TokenKind::Punct(Punct::RParen))?;
                    let span = Span::new(e.span.start_line, e.span.start_col, close.line, close.end_col);
                    e = Expr::new(ExprKind::MethodCall(Box::new(e), name, args), span);
                } else {
                    let span = Span::new(e.span.start_line, e.span.start_col, tok.line, tok.end_col);
                    e = Expr::new(ExprKind::Member(Box::new(e), name), span);
                }
            } else if matches!(e.kind, ExprKind::Member(..) | ExprKind::MethodCall(..) | ExprKind::Index(..)) && self.check(&TokenKind::Punct(Punct::LBracket)) {
                self.advance();
                let index = self.expr()?;
                let close = self.expect(TokenKind::Punct(Punct::RBracket))?;
                let span = Span::new(e.span.start_line, e.span.start_col, close.line, close.end_col);
                e = Expr::new(ExprKind::Index(Box::new(e), Box::new(index)), span);
            } else {
                return Ok(e);
            }
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
                let mut end = self.peek().clone();
                // vJASS: `function thistype.onTimer`, `function Data.onTimer`.
                let mut name = if self.ts.eat(&TokenKind::Keyword(Kw::ThisType)) { "thistype".to_string() } else { self.expect_identifier()? };
                while self.ts.eat(&TokenKind::Punct(Punct::Dot)) {
                    end = self.peek().clone();
                    name = format!("{name}.{}", self.expect_identifier()?);
                }
                let span = Span::new(start.line, start.col, end.line, end.end_col);
                Ok(Expr::new(ExprKind::FuncRef(name), span))
            }
            TokenKind::Keyword(Kw::This) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::This, tok_span(&tok)))
            }
            TokenKind::Keyword(Kw::Super) => {
                let tok = self.advance();
                Ok(Expr::new(ExprKind::Super, tok_span(&tok)))
            }
            // `thistype(x)`: a cast, as a struct's name.
            TokenKind::Keyword(Kw::ThisType) => {
                let tok = self.advance();
                if self.ts.eat(&TokenKind::Punct(Punct::LParen)) {
                    let args = self.expr_list(&TokenKind::Punct(Punct::RParen))?;
                    let close = self.expect(TokenKind::Punct(Punct::RParen))?;
                    let span = Span::new(tok.line, tok.col, close.line, close.end_col);
                    return Ok(Expr::new(ExprKind::Call("thistype".into(), args), span));
                }
                Ok(Expr::new(ExprKind::ThisType, tok_span(&tok)))
            }
            // `.x`: `this.x`, the `this` written nowhere.
            TokenKind::Punct(Punct::Dot) => {
                let tok = self.peek().clone();
                Ok(Expr::new(ExprKind::This, Span::new(tok.line, tok.col, tok.line, tok.col)))
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
