use crate::ast::*;
pub use parser_rs::Node;

/// Builds a visualizable tree out of a parsed program.
pub fn build_tree(program: &Program) -> Node {
    Node::with_children(
        "Program",
        program.items.iter().map(top_level_node).collect(),
    )
    .tag("root", None)
}

/// Renders the program's AST as an indented tree, e.g.:
///
/// ```text
/// Program
/// └─ Function Add(integer a, integer b) -> integer (line 2)
///    ├─ Local integer sum (line 3)
///    │  └─ Binary Add
///    │     ├─ Var a
///    │     └─ Var b
///    └─ Return (line 4)
///       └─ Var sum
/// ```
pub fn render_tree(program: &Program) -> String {
    parser_rs::render_tree(&build_tree(program))
}

/// Renders the program's AST as a Graphviz DOT graph.
pub fn render_dot(program: &Program) -> String {
    parser_rs::render_dot(&build_tree(program))
}

fn top_level_node(item: &TopLevel) -> Node {
    match item {
        TopLevel::Zinc(block) => {
            let span = block.span();
            Node::with_children(
                format!("Zinc (lines {}-{})", span.start_line, span.end_line),
                zinc_rs::printer::build_tree(&block.program).children,
            )
            .tag("container", Some(span))
        }
        TopLevel::Globals(decls) => {
            Node::with_children("Globals", decls.iter().map(global_decl_node).collect())
                .tag("container", None)
        }
        TopLevel::Global(d) => global_decl_node(d),
        TopLevel::Stmt(s) => stmt_node(s),
        TopLevel::TypeDef {
            name,
            extends,
            line,
            size,
            modifiers,
        } => {
            let label = format!("{}Type {name} extends {extends}{} (line {line})", access(modifiers), if size.is_some() { " array" } else { "" });
            Node::with_children(label, size.iter().map(expr_node).collect()).tag("decl", Some(Span::whole_line(*line)))
        }
        TopLevel::Library(l) => {
            let kind = if l.scope { "Scope" } else if l.once { "LibraryOnce" } else { "Library" };
            let mut label = format!("{kind} {}", l.name);
            if let Some(i) = &l.initializer {
                label += &format!(" initializer {i}");
            }
            if !l.requires.is_empty() {
                let r: Vec<String> = l.requires.iter().map(|(n, o)| if *o { format!("optional {n}") } else { n.clone() }).collect();
                label += &format!(" requires {}", r.join(", "));
            }
            label += &format!(" (lines {}-{})", l.line, l.end_line);
            Node::with_children(label, l.items.iter().map(top_level_node).collect()).tag("container", Some(Span::new(l.line, 1, l.end_line, 1)))
        }
        TopLevel::Struct(st) => {
            let mut label = format!("{}Struct {}", access(&st.modifiers), st.name);
            if let Some(e) = &st.extends {
                label += &format!(" extends {e}");
            }
            label += &format!(" (lines {}-{})", st.line, st.end_line);
            let mut children: Vec<Node> = st.size.iter().map(|e| Node::with_children("Size", vec![expr_node(e)]).tag("container", Some(e.span))).collect();
            children.extend(st.members.iter().map(member_node));
            Node::with_children(label, children).tag("container", Some(Span::new(st.line, 1, st.end_line, 1)))
        }
        TopLevel::Interface(t) | TopLevel::Module(t) => {
            let kind = if matches!(item, TopLevel::Interface(_)) { "Interface" } else { "Module" };
            let label = format!("{}{kind} {} (lines {}-{})", access(&t.modifiers), t.name, t.line, t.end_line);
            Node::with_children(label, t.members.iter().map(member_node).collect()).tag("container", Some(Span::new(t.line, 1, t.end_line, 1)))
        }
        TopLevel::FunctionInterface(sig) => {
            Node::leaf(format!("{}FunctionInterface {}", access(&sig.modifiers), sig_label(sig))).tag("sig", Some(Span::whole_line(sig.line)))
        }
        TopLevel::Keyword { modifiers, name, line } => {
            Node::leaf(format!("{}Keyword {name} (line {line})", access(modifiers))).tag("decl", Some(Span::whole_line(*line)))
        }
        TopLevel::Native(sig) => Node::leaf(format!("Native {}", sig_label(sig)))
            .tag("sig", Some(Span::whole_line(sig.line))),
        TopLevel::Function(f) => Node::with_children(
            format!("{}Function {}", access(&f.sig.modifiers), sig_label(&f.sig)),
            f.body.iter().map(stmt_node).collect(),
        )
        .tag("sig", Some(Span::whole_line(f.sig.line))),
    }
}

/// vJASS's modifiers as a label's start: `private static `.
fn access(m: &Modifiers) -> String {
    let mut out = String::new();
    match m.access {
        Access::Private => out += "private ",
        Access::Public => out += "public ",
        Access::Default => {}
    }
    for (on, word) in [(m.is_static, "static "), (m.is_readonly, "readonly "), (m.is_stub, "stub "), (m.is_delegate, "delegate ")] {
        if on {
            out += word;
        }
    }
    out
}

fn member_node(m: &StructMember) -> Node {
    match m {
        StructMember::Field(d) => global_decl_node(d),
        StructMember::Method(md) => {
            let label = format!("{}Method {}", access(&md.sig.modifiers), sig_label(&md.sig));
            let mut children: Vec<Node> = md.defaults.iter().map(|e| Node::with_children("Defaults", vec![expr_node(e)]).tag("container", Some(e.span))).collect();
            children.extend(md.body.iter().flatten().map(stmt_node));
            Node::with_children(label, children).tag("sig", Some(Span::whole_line(md.sig.line)))
        }
        StructMember::Implement { name, optional, line } => {
            Node::leaf(format!("Implement {}{name} (line {line})", if *optional { "optional " } else { "" })).tag("decl", Some(Span::whole_line(*line)))
        }
    }
}

fn sig_label(sig: &FunctionSig) -> String {
    let params = sig
        .params
        .iter()
        .map(|p| format!("{} {}", p.type_name, p.name))
        .collect::<Vec<_>>()
        .join(", ");
    let params = if params.is_empty() {
        "nothing".to_string()
    } else {
        params
    };
    let returns = sig.returns.clone().unwrap_or_else(|| "nothing".to_string());
    let constant = if sig.is_constant { "constant " } else { "" };
    format!(
        "{constant}{}({params}) -> {returns} (line {})",
        sig.name, sig.line
    )
}

fn global_decl_node(d: &GlobalDecl) -> Node {
    let label = format!(
        "{}{}{}{} {} (line {})",
        access(&d.modifiers),
        if d.is_constant { "constant " } else { "" },
        d.type_name,
        if d.is_array { " array" } else { "" },
        d.name,
        d.line
    );
    let mut children: Vec<Node> = d.size.iter().map(|e| Node::with_children("Size", vec![expr_node(e)]).tag("container", Some(e.span))).collect();
    children.extend(d.initializer.iter().map(expr_node));
    Node::with_children(label, children).tag("decl", Some(Span::whole_line(d.line)))
}

fn stmt_node(s: &Stmt) -> Node {
    match s {
        Stmt::Local {
            type_name,
            is_array,
            name,
            initializer,
            line,
            ..
        } => {
            let label = format!(
                "Local {type_name}{} {name} (line {line})",
                if *is_array { " array" } else { "" }
            );
            Node::with_children(label, initializer.iter().map(expr_node).collect())
                .tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::Set {
            name,
            index,
            value,
            line,
            ..
        } => {
            let mut children = Vec::new();
            if let Some(idx) = index {
                children.push(
                    Node::with_children("Index", vec![expr_node(idx)])
                        .tag("container", Some(Span::whole_line(*line))),
                );
            }
            children.push(
                Node::with_children("Value", vec![expr_node(value)])
                    .tag("container", Some(Span::whole_line(*line))),
            );
            Node::with_children(format!("Set {name} (line {line})"), children)
                .tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::Call { name, args, line, .. } => Node::with_children(
            format!("Call {name} (line {line})"),
            args.iter().map(expr_node).collect(),
        )
        .tag("stmt", Some(Span::whole_line(*line))),
        Stmt::If {
            branches,
            else_branch,
            line,
            ..
        } => {
            let mut children = Vec::new();
            for (i, (cond, body)) in branches.iter().enumerate() {
                let keyword = if i == 0 { "If" } else { "ElseIf" };
                children.push(
                    Node::with_children(format!("{keyword} condition"), vec![expr_node(cond)])
                        .tag("container", Some(Span::whole_line(*line))),
                );
                children.push(
                    Node::with_children("Then", body.iter().map(stmt_node).collect())
                        .tag("container", Some(Span::whole_line(*line))),
                );
            }
            if let Some(body) = else_branch {
                children.push(
                    Node::with_children("Else", body.iter().map(stmt_node).collect())
                        .tag("container", Some(Span::whole_line(*line))),
                );
            }
            Node::with_children(format!("If (line {line})"), children)
                .tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::Loop { body, line } => Node::with_children(
            format!("Loop (line {line})"),
            body.iter().map(stmt_node).collect(),
        )
        .tag("stmt", Some(Span::whole_line(*line))),
        Stmt::ExitWhen { cond, line } => {
            Node::with_children(format!("ExitWhen (line {line})"), vec![expr_node(cond)])
                .tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::Return { value, line } => Node::with_children(
            format!("Return (line {line})"),
            value.iter().map(expr_node).collect(),
        )
        .tag("stmt", Some(Span::whole_line(*line))),
        Stmt::Debug { stmt, line } => {
            Node::with_children(format!("Debug (line {line})"), vec![stmt_node(stmt)])
                .tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::SetMember { target, index, value, line, .. } => {
            let mut children = vec![Node::with_children("Target", vec![expr_node(target)]).tag("container", Some(target.span))];
            if let Some(idx) = index {
                children.push(Node::with_children("Index", vec![expr_node(idx)]).tag("container", Some(idx.span)));
            }
            children.push(Node::with_children("Value", vec![expr_node(value)]).tag("container", Some(value.span)));
            Node::with_children(format!("SetMember (line {line})"), children).tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::CallMethod { call, line, .. } => {
            Node::with_children(format!("CallMethod (line {line})"), vec![expr_node(call)]).tag("stmt", Some(Span::whole_line(*line)))
        }
        Stmt::StaticIf { branches, else_branch, line, .. } => {
            let mut children = Vec::new();
            for (cond, body) in branches {
                children.push(Node::with_children("Condition", vec![expr_node(cond)]).tag("container", Some(cond.span)));
                children.push(Node::with_children("Then", body.iter().map(stmt_node).collect()).tag("container", Some(Span::whole_line(*line))));
            }
            if let Some(body) = else_branch {
                children.push(Node::with_children("Else", body.iter().map(stmt_node).collect()).tag("container", Some(Span::whole_line(*line))));
            }
            Node::with_children(format!("StaticIf (line {line})"), children).tag("stmt", Some(Span::whole_line(*line)))
        }
    }
}

fn expr_node(e: &Expr) -> Node {
    let span = Some(e.span);
    match &e.kind {
        ExprKind::IntLiteral(v) => Node::leaf(format!("Int {v}")).tag("lit", span),
        ExprKind::RealLiteral(v) => Node::leaf(format!("Real {v}")).tag("lit", span),
        ExprKind::StringLiteral(v) => Node::leaf(format!("String {v:?}")).tag("lit", span),
        ExprKind::BoolLiteral(v) => Node::leaf(format!("Bool {v}")).tag("lit", span),
        ExprKind::Null => Node::leaf("Null").tag("lit", span),
        ExprKind::Var(name) => Node::leaf(format!("Var {name}")).tag("expr", span),
        ExprKind::ArrayAccess(name, idx) => {
            Node::with_children(format!("ArrayAccess {name}"), vec![expr_node(idx)])
                .tag("expr", span)
        }
        ExprKind::Call(name, args) => {
            Node::with_children(format!("Call {name}"), args.iter().map(expr_node).collect())
                .tag("expr", span)
        }
        ExprKind::FuncRef(name) => Node::leaf(format!("FuncRef {name}")).tag("expr", span),
        ExprKind::Unary(op, inner) => {
            Node::with_children(format!("Unary {op:?}"), vec![expr_node(inner)]).tag("expr", span)
        }
        ExprKind::Binary(l, op, r) => {
            Node::with_children(format!("Binary {op:?}"), vec![expr_node(l), expr_node(r)])
                .tag("expr", span)
        }
        ExprKind::This => Node::leaf("This").tag("expr", span),
        ExprKind::ThisType => Node::leaf("ThisType").tag("expr", span),
        ExprKind::Super => Node::leaf("Super").tag("expr", span),
        ExprKind::Member(object, name) => Node::with_children(format!("Member {name}"), vec![expr_node(object)]).tag("expr", span),
        ExprKind::MethodCall(object, name, args) => {
            Node::with_children(format!("MethodCall {name}"), std::iter::once(object.as_ref()).chain(args).map(expr_node).collect()).tag("expr", span)
        }
        ExprKind::Index(object, index) => Node::with_children("Index", vec![expr_node(object), expr_node(index)]).tag("expr", span),
    }
}

#[cfg(test)]
#[path = "printer.test.rs"]
mod tests;
