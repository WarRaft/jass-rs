use super::*;

#[test]
fn parses_empty_globals_block() {
    let prog = Parser::parse_str("globals\nendglobals").unwrap();
    assert_eq!(prog.items, vec![TopLevel::Globals(vec![])]);
}

#[test]
fn parses_global_declarations() {
    let prog = Parser::parse_str(
        "globals\n    integer x = 1\n    constant real PI = 3.14\n    unit array units\nendglobals",
    )
    .unwrap();
    match &prog.items[0] {
        TopLevel::Globals(decls) => {
            assert_eq!(decls.len(), 3);
            assert_eq!(decls[0].name, "x");
            assert_eq!(decls[0].type_name, "integer");
            assert!(!decls[0].is_constant);
            assert_eq!(decls[1].name, "PI");
            assert!(decls[1].is_constant);
            assert_eq!(decls[2].name, "units");
            assert!(decls[2].is_array);
        }
        other => panic!("expected globals, got {other:?}"),
    }
}

#[test]
fn parses_native_declaration() {
    let prog = Parser::parse_str("native DoNothing takes nothing returns nothing").unwrap();
    match &prog.items[0] {
        TopLevel::Native(sig) => {
            assert_eq!(sig.name, "DoNothing");
            assert!(sig.params.is_empty());
            assert_eq!(sig.returns, None);
        }
        other => panic!("expected native, got {other:?}"),
    }
}

#[test]
fn parses_simple_function() {
    let src = r#"
function Add takes integer a, integer b returns integer
local integer sum = a + b
return sum
endfunction
"#;
    let prog = Parser::parse_str(src).unwrap();
    match &prog.items[0] {
        TopLevel::Function(f) => {
            assert_eq!(f.sig.name, "Add");
            assert_eq!(f.sig.params.len(), 2);
            assert_eq!(f.sig.returns, Some("integer".to_string()));
            assert_eq!(f.body.len(), 2);
        }
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_if_elseif_else() {
    let src = r#"
function Test takes nothing returns nothing
if 1 > 0 then
    call A()
elseif 1 == 0 then
    call B()
else
    call C()
endif
endfunction
"#;
    let prog = Parser::parse_str(src).unwrap();
    match &prog.items[0] {
        TopLevel::Function(f) => match &f.body[0] {
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                assert_eq!(branches.len(), 2);
                assert!(else_branch.is_some());
            }
            other => panic!("expected if, got {other:?}"),
        },
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_loop_and_exitwhen() {
    let src = r#"
function Test takes nothing returns nothing
local integer i = 0
loop
    exitwhen i > 10
    set i = i + 1
endloop
endfunction
"#;
    let prog = Parser::parse_str(src).unwrap();
    match &prog.items[0] {
        TopLevel::Function(f) => match &f.body[1] {
            Stmt::Loop { body, .. } => {
                assert_eq!(body.len(), 2);
                assert!(matches!(body[0], Stmt::ExitWhen { .. }));
            }
            other => panic!("expected loop, got {other:?}"),
        },
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn respects_operator_precedence() {
    let mut parser = Parser::new(Lexer::tokenize("1 + 2 * 3").unwrap());
    let e = parser.expr().unwrap();
    assert_eq!(
        e,
        Expr::dummy(ExprKind::Binary(
            Box::new(Expr::dummy(ExprKind::IntLiteral(1))),
            BinOp::Add,
            Box::new(Expr::dummy(ExprKind::Binary(
                Box::new(Expr::dummy(ExprKind::IntLiteral(2))),
                BinOp::Mul,
                Box::new(Expr::dummy(ExprKind::IntLiteral(3))),
            ))),
        ))
    );
    assert_eq!(e.span, Span::new(1, 1, 1, 10));
}

#[test]
fn and_binds_looser_than_or() {
    // `false and true or true` == `false and (true or true)` == false.
    let mut parser = Parser::new(Lexer::tokenize("false and true or true").unwrap());
    let e = parser.expr().unwrap();
    let b = |v| Box::new(Expr::dummy(ExprKind::BoolLiteral(v)));
    assert_eq!(
        e,
        Expr::dummy(ExprKind::Binary(
            b(false),
            BinOp::And,
            Box::new(Expr::dummy(ExprKind::Binary(b(true), BinOp::Or, b(true)))),
        ))
    );

    // Same grouping with the operators the other way round.
    let mut parser = Parser::new(Lexer::tokenize("true or false and false").unwrap());
    let e = parser.expr().unwrap();
    assert_eq!(
        e,
        Expr::dummy(ExprKind::Binary(
            Box::new(Expr::dummy(ExprKind::Binary(b(true), BinOp::Or, b(false)))),
            BinOp::And,
            b(false),
        ))
    );
}

#[test]
fn parses_array_access_and_call() {
    let mut parser = Parser::new(Lexer::tokenize("arr[GetInt(1)]").unwrap());
    let e = parser.expr().unwrap();
    assert_eq!(
        e,
        Expr::dummy(ExprKind::ArrayAccess(
            "arr".to_string(),
            Box::new(Expr::dummy(ExprKind::Call(
                "GetInt".to_string(),
                vec![Expr::dummy(ExprKind::IntLiteral(1))]
            ))),
        ))
    );
    assert_eq!(e.span, Span::new(1, 1, 1, 15));
}

#[test]
fn reports_error_on_missing_endif() {
    let src = "function T takes nothing returns nothing\n if true then\n call A()\nendfunction";
    let err = Parser::parse_str(src).unwrap_err();
    assert!(err.message.contains("'endfunction'"));
}

#[test]
fn reports_error_on_unknown_top_level_token() {
    let err = Parser::parse_str("42").unwrap_err();
    assert!(err.message.contains("unexpected top-level token"));
}

#[test]
fn parses_globals_declared_at_top_level() {
    let src = "integer x = 1\nfunction F takes nothing returns nothing\nendfunction\nconstant real PI = 3.14\nunit array units";
    let prog = Parser::parse_str(src).unwrap();
    let globals: Vec<(&str, bool, bool, usize)> = prog
        .items
        .iter()
        .filter_map(|item| match item {
            TopLevel::Global(d) => Some((d.name.as_str(), d.is_constant, d.is_array, d.line)),
            _ => None,
        })
        .collect();
    assert_eq!(globals, [("x", false, false, 1), ("PI", true, false, 4), ("units", false, true, 5)]);
}

#[test]
fn reads_statements_at_top_level_in_full() {
    let src = "if x then\n    call F()\nendif\nset y = 1\nfunction G takes nothing returns nothing\nendfunction";
    let prog = Parser::parse_str(src).unwrap();
    assert!(matches!(&prog.items[0], TopLevel::Stmt(Stmt::If { branches, .. }) if branches[0].1.len() == 1));
    assert!(matches!(&prog.items[1], TopLevel::Stmt(Stmt::Set { .. })));
    assert!(matches!(&prog.items[2], TopLevel::Function(_)));
}

#[test]
fn parses_constant_function() {
    let prog = Parser::parse_str("constant function F takes nothing returns integer\n    return 1\nendfunction").unwrap();
    match &prog.items[0] {
        TopLevel::Function(f) => assert!(f.sig.is_constant),
        other => panic!("expected a function, got {other:?}"),
    }
}

/// The body of the only function in `src`.
fn body(src: &str) -> Vec<Stmt> {
    match Parser::parse_str(src).unwrap().items.into_iter().next() {
        Some(TopLevel::Function(f)) => f.body,
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn parses_debug_statements() {
    // The forms the game's own AI scripts use.
    let src = "\
function F takes nothing returns nothing
    debug call Trace(\"x\")
    debug set x = 1
    debug if a then
        set x = 2
    endif
    debug loop
        exitwhen true
    endloop
endfunction";
    let stmts = body(src);
    let got: Vec<(usize, &str)> = stmts
        .iter()
        .map(|s| match s {
            Stmt::Debug { stmt, line } => (
                *line,
                match stmt.as_ref() {
                    Stmt::Call { .. } => "call",
                    Stmt::Set { .. } => "set",
                    Stmt::If { .. } => "if",
                    Stmt::Loop { .. } => "loop",
                    other => panic!("unexpected {other:?}"),
                },
            ),
            other => panic!("expected debug, got {other:?}"),
        })
        .collect();
    assert_eq!(got, vec![(2, "call"), (3, "set"), (4, "if"), (7, "loop")]);
}

/// One statement, parsed alone in a function.
fn stmts_of(stmt: &str) -> Stmt {
    body(&format!("function F takes nothing returns nothing\n    {stmt}\nendfunction"))
        .into_iter()
        .next()
        .unwrap()
}

#[test]
fn debug_only_precedes_set_call_if_or_loop() {
    for bad in ["debug local integer x", "debug return", "debug exitwhen true", "debug debug call F()"] {
        let src = format!("function F takes nothing returns nothing\n    {bad}\nendfunction");
        let err = Parser::parse_str(&src).unwrap_err();
        assert!(err.message.contains("'debug' can only precede"), "{bad}: {}", err.message);
        assert_eq!((err.line, err.col), (2, 11), "{bad}");
    }
}

#[test]
fn parses_unary_plus() {
    // As in the game's u08x02.ai: `call attack_move(+OFFSET_X,+OFFSET_Y)`.
    match stmts_of("call attack_move(+OFFSET_X, -+1)") {
        Stmt::Call { args, .. } => {
            assert!(matches!(&args[0].kind, ExprKind::Unary(UnaryOp::Plus, inner) if matches!(inner.kind, ExprKind::Var(_))));
            assert_eq!((args[0].span.start_col, args[0].span.end_col), (22, 31));
            assert!(matches!(&args[1].kind, ExprKind::Unary(UnaryOp::Neg, inner) if matches!(inner.kind, ExprKind::Unary(UnaryOp::Plus, _))));
        }
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn takes_and_returns_may_be_left_out() {
    let sig = |src: &str| match Parser::parse_str(src).unwrap().items.remove(0) {
        TopLevel::Function(f) => f.sig,
        TopLevel::Native(sig) => sig,
        other => panic!("expected a function, got {other:?}"),
    };
    let s = sig("function main\n    call Init()\nendfunction");
    assert_eq!((s.params.len(), s.returns.clone(), s.has_takes, s.has_returns), (0, None, false, false));

    let s = sig("function F takes integer a, real b\nendfunction");
    assert_eq!((s.params.len(), s.returns.clone(), s.has_takes, s.has_returns), (2, None, true, false));

    let s = sig("constant function G returns integer\n    return 1\nendfunction");
    assert_eq!((s.params.len(), s.returns.clone(), s.has_takes, s.has_returns), (0, Some("integer".into()), false, true));
    assert!(s.is_constant);

    let s = sig("native N\nnative M takes nothing returns nothing");
    assert_eq!((s.name.as_str(), s.has_takes, s.has_returns), ("N", false, false));

    let s = sig("function H takes nothing returns nothing\nendfunction");
    assert_eq!((s.has_takes, s.has_returns), (true, true));

    // A one-line function with nothing after the name.
    let prog = Parser::parse_str("function A\nendfunction\nfunction B\nendfunction").unwrap();
    assert_eq!(prog.items.len(), 2);
}

#[test]
fn set_and_call_may_be_left_out() {
    let body = |src: &str| match Parser::parse_str(src).unwrap().items.remove(0) {
        TopLevel::Function(f) => f.body,
        other => panic!("expected a function, got {other:?}"),
    };
    let b = body("function F\n    x = 1\n    a[i + 1] = x\n    Foo(x, 2)\n    set y = 2\n    call Bar()\nendfunction");
    let kinds: Vec<(&str, &str, bool)> = b
        .iter()
        .map(|s| match s {
            Stmt::Set { name, keyword, .. } => ("set", name.as_str(), *keyword),
            Stmt::Call { name, keyword, .. } => ("call", name.as_str(), *keyword),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, [("set", "x", false), ("set", "a", false), ("call", "Foo", false), ("set", "y", true), ("call", "Bar", true)]);
    assert!(matches!(&b[1], Stmt::Set { index: Some(_), .. }));

    // `return` takes a value on its line only: a name on the next is a statement.
    let b = body("function F\n    if a then\n        return\n    endif\n    Foo()\nendfunction");
    assert!(matches!(&b[1], Stmt::Call { keyword: false, .. }), "{b:?}");
}

#[test]
fn a_name_alone_starts_no_statement() {
    let err = Parser::parse_str("function F\n    x\nendfunction").unwrap_err();
    assert!(err.message.contains("'x' starts no statement"), "{}", err.message);
    // `debug` before one without its keyword.
    let prog = Parser::parse_str("function F\n    debug Foo()\nendfunction").unwrap();
    match &prog.items[0] {
        TopLevel::Function(f) => assert!(matches!(&f.body[0], Stmt::Debug { stmt, .. } if matches!(**stmt, Stmt::Call { keyword: false, .. }))),
        other => panic!("{other:?}"),
    }
}

#[test]
fn local_may_be_left_out() {
    let prog = Parser::parse_str("integer g = 1\nfunction F\n    integer i = 0\n    unit array us\n    local real r\n    i = g\nendfunction").unwrap();
    // At the top level a global, in a function a local.
    assert!(matches!(&prog.items[0], TopLevel::Global(d) if d.name == "g"));
    let TopLevel::Function(f) = &prog.items[1] else { panic!("{:?}", prog.items[1]) };
    let locals: Vec<(&str, &str, bool, bool)> = f
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Local { type_name, name, is_array, keyword, .. } => Some((type_name.as_str(), name.as_str(), *is_array, *keyword)),
            _ => None,
        })
        .collect();
    assert_eq!(locals, [("integer", "i", false, false), ("unit", "us", true, false), ("real", "r", false, true)]);
    assert!(matches!(&f.body[3], Stmt::Set { keyword: false, .. }));
    // Not after `debug`: it skips set, call, if and loop only.
    let err = Parser::parse_str("function F\n    debug integer i\nendfunction").unwrap_err();
    assert!(err.message.contains("'debug' can only precede"), "{}", err.message);
}

fn vjass(src: &str) -> Program {
    Parser::parse_str_with(src, Dialect::VJass).unwrap_or_else(|e| panic!("line {}: {}", e.line, e.message))
}

#[test]
fn vjass_libraries_structs_and_members() {
    let prog = vjass(include_str!("../tests/fixtures/vjass.j"));
    let TopLevel::Library(lib) = &prog.items[0] else { panic!("{:?}", prog.items[0]) };
    assert_eq!((lib.name.as_str(), lib.scope, lib.initializer.as_deref()), ("Counters", false, Some("Init")));
    assert_eq!(lib.requires, vec![("Utils".to_string(), false), ("TimerUtils".to_string(), true)]);
    assert_eq!((lib.line, lib.end_line), (2, 75));

    let TopLevel::Globals(g) = &lib.items[0] else { panic!() };
    let globals: Vec<(&str, Access, bool, bool)> = g.iter().map(|d| (d.name.as_str(), d.modifiers.access, d.is_constant, d.size.is_some())).collect();
    assert_eq!(globals, [("count", Access::Private, false, false), ("MAX", Access::Public, true, false), ("sized", Access::Default, false, true)]);
    assert!(matches!(&lib.items[1], TopLevel::Keyword { name, modifiers, .. } if name == "Ahead" && modifiers.access == Access::Private));
    assert!(matches!(&lib.items[2], TopLevel::FunctionInterface(sig) if sig.name == "Callback" && sig.params.len() == 1));
    assert!(matches!(&lib.items[3], TopLevel::TypeDef { name, size: Some(_), .. } if name == "IntList"));

    let TopLevel::Interface(shape) = &lib.items[4] else { panic!() };
    let methods: Vec<(&str, bool, bool)> = shape
        .members
        .iter()
        .map(|m| match m {
            StructMember::Method(md) => (md.sig.name.as_str(), md.body.is_some(), md.defaults.is_some()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(methods, [("area", false, false), ("name", false, true)]);
    let TopLevel::Module(pooled) = &lib.items[5] else { panic!() };
    assert!(matches!(&pooled.members[0], StructMember::Method(md) if md.sig.modifiers.is_static && md.body.is_some()));

    let TopLevel::Struct(st) = &lib.items[6] else { panic!() };
    assert_eq!((st.name.as_str(), st.extends.as_deref(), st.size.is_some()), ("Counter", Some("Shape"), true));
    let fields: Vec<(&str, bool, Access, bool, bool)> = st
        .members
        .iter()
        .filter_map(|m| match m {
            StructMember::Field(d) => Some((d.name.as_str(), d.modifiers.is_static, d.modifiers.access, d.modifiers.is_readonly, d.is_array)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fields,
        [("value", false, Access::Default, false, false), ("made", true, Access::Default, false, false), ("ratio", false, Access::Private, false, false), ("id", false, Access::Default, true, false), ("list", false, Access::Default, false, true)]
    );
    let implements: Vec<(&str, bool)> = st
        .members
        .iter()
        .filter_map(|m| match m {
            StructMember::Implement { name, optional, .. } => Some((name.as_str(), *optional)),
            _ => None,
        })
        .collect();
    assert_eq!(implements, [("Pooled", false), ("Extra", true)]);
    let methods: Vec<&MethodDecl> = st
        .members
        .iter()
        .filter_map(|m| match m {
            StructMember::Method(md) => Some(md),
            _ => None,
        })
        .collect();
    let names: Vec<&str> = methods.iter().map(|m| m.sig.name.as_str()).collect();
    assert_eq!(names, ["create", "area", "operator []", "operator x=", "onDestroy"]);
    assert!(methods[4].sig.modifiers.is_stub);

    let v = |n: &str| Expr::dummy(ExprKind::Var(n.into()));
    let member = |o: Expr, n: &str| Expr::dummy(ExprKind::Member(Box::new(o), n.into()));
    let int = |i: i64| Expr::dummy(ExprKind::IntLiteral(i));
    let this_type = || Expr::dummy(ExprKind::ThisType);
    let create = methods[0].body.as_ref().unwrap();
    assert_eq!(
        create[0],
        Stmt::Local {
            type_name: "thistype".into(),
            is_array: false,
            name: "c".into(),
            initializer: Some(Expr::dummy(ExprKind::MethodCall(Box::new(this_type()), "allocate".into(), vec![]))),
            line: 34,
            keyword: true
        }
    );
    assert_eq!(create[1], Stmt::SetMember { target: member(v("c"), "value"), index: None, value: v("v"), line: 35, keyword: true });
    let list_0 = Expr::dummy(ExprKind::Index(Box::new(member(v("c"), "list")), Box::new(int(0))));
    assert_eq!(
        create[2],
        Stmt::SetMember {
            target: member(v("c"), "list"),
            index: Some(int(1)),
            value: Expr::dummy(ExprKind::Binary(Box::new(list_0), BinOp::Add, Box::new(int(2)))),
            line: 36,
            keyword: true
        }
    );
    // `.id`: `this` written nowhere.
    let this = || Expr::dummy(ExprKind::This);
    assert_eq!(create[4], Stmt::SetMember { target: member(this(), "id"), index: None, value: member(this_type(), "made"), line: 38, keyword: true });
    let destroy = methods[4].body.as_ref().unwrap();
    assert!(matches!(&destroy[0], Stmt::CallMethod { call, .. } if matches!(&call.kind, ExprKind::MethodCall(o, n, _) if o.kind == ExprKind::Super && n == "onDestroy")));

    let TopLevel::Library(inner) = &lib.items[7] else { panic!() };
    assert!(inner.scope && inner.initializer.as_deref() == Some("Start"));
    let TopLevel::Function(start) = &inner.items[0] else { panic!() };
    assert_eq!(start.sig.modifiers.access, Access::Private);
    assert!(matches!(&start.body[1], Stmt::CallMethod { call, .. } if matches!(&call.kind, ExprKind::MethodCall(o, n, a) if **o == v("c") && n == "destroy" && a.is_empty())));
    assert!(matches!(&start.body[2], Stmt::Call { args, .. } if args[3].kind == ExprKind::FuncRef("Counter.tick".into())));
    assert!(matches!(&start.body[3], Stmt::StaticIf { branches, else_branch: Some(_), .. } if branches.len() == 1));
    let TopLevel::Function(init) = &lib.items[8] else { panic!() };
    // `Counter(5).value`: a cast's member.
    assert!(matches!(&init.body[0], Stmt::Set { value, .. } if matches!(&value.kind, ExprKind::Member(o, n) if n == "value" && matches!(&o.kind, ExprKind::Call(c, _) if c == "Counter"))));
}

#[test]
fn vjass_words_are_names_in_jass() {
    // Plain JASS: no members, and vJASS's words are names.
    let prog = Parser::parse_str("function F takes nothing returns nothing\n    local integer library = 1\n    local integer struct = library\nendfunction").unwrap();
    assert!(matches!(&prog.items[0], TopLevel::Function(f) if f.body.len() == 2));
    assert!(Parser::parse_str("function F takes nothing returns nothing\n    call a.b()\nendfunction").is_err());
}
