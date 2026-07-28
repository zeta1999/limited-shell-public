//! Pretty printer for Limited Shell AST — round-trips parse → pretty_print → parse.

use crate::ast;
use crate::ast::expr::{BinOp, Expr, Literal, UnOp};

/// Format an entire program back to source code.
pub fn pretty_print(program: &ast::Program) -> String {
    let mut buf = String::new();
    for item in &program.items {
        buf.push_str(&item_pretty(item));
        buf.push('\n');
    }
    for stmt in &program.statements {
        buf.push_str(&stmt_pretty(stmt));
        buf.push('\n');
    }
    buf
}

fn item_pretty(item: &ast::Item) -> String {
    match item {
        ast::Item::Alias(a) => alias_pretty(a),
        ast::Item::Role(r) => role_pretty(r),
        ast::Item::Resource(r) => resource_pretty(r),
        ast::Item::Device(d) => device_pretty(d),
        ast::Item::Operation(o) => operation_pretty(o),
        ast::Item::Service(s) => service_pretty(s),
        ast::Item::Function(f) => function_pretty(f),
        _ => String::new(),
    }
}

fn stmt_pretty(stmt: &ast::Statement) -> String {
    match stmt {
        ast::Statement::Grant(g) => grant_pretty(g),
        ast::Statement::Alias(a) => alias_pretty(a),
        ast::Statement::OnMachine(o) => on_machine_pretty(o),
        ast::Statement::TaskBlock(t) => task_block_pretty(t),
        ast::Statement::ControlFlow(c) => control_flow_pretty(c),
        ast::Statement::MachineDecl(m) => machine_pretty(m),
        ast::Statement::RoleDecl(r) => role_decl_stmt_pretty(r),
    }
}

// ─── Aliases ───────────────────────────────────────────────

fn alias_pretty(a: &ast::AliasDecl) -> String {
    let mut s = String::from("alias ");
    match a.kind {
        ast::AliasKind::Machine => s.push_str("m as machine = "),
        ast::AliasKind::Path => s.push_str("path = "),
        ast::AliasKind::Role => s.push_str("r as role = "),
        ast::AliasKind::Generic => s.push_str(&format!("{} = ", a.name)),
    }
    s.push_str(&type_pretty(&a.target));
    s
}

// ─── Roles ─────────────────────────────────────────────────

fn role_pretty(r: &ast::RoleDecl) -> String {
    let mut s = format!("role {} {{\n", r.name);
    if let Some(up) = &r.up {
        s.push_str(&format!("  up: {},\n", up));
    }
    for perm in &r.permissions {
        s.push_str(&format!("  {},\n", perm_pretty(perm)));
    }
    for target in &r.define_ops_for {
        s.push_str(&format!(
            "  can define operation for {},\n",
            target_pretty(target)
        ));
    }
    s.push_str("}");
    s
}

fn role_decl_stmt_pretty(r: &ast::RoleDecl) -> String {
    let mut s = role_pretty(r);
    s.push(';');
    s
}

fn perm_pretty(p: &ast::Permission) -> String {
    let mut s: String = if p.deny {
        "cannot ".into()
    } else {
        "can ".into()
    };
    s.push_str(&p.op.to_string());
    if let Some(rp) = &p.resource {
        s.push_str(&format!(
            " {{ {}: {} }}",
            rp.variable,
            type_pretty(&rp.resource_type)
        ));
    }
    if let Some(cond) = &p.condition {
        s.push_str(&format!(" if {}", cond_pretty(cond)));
    }
    s
}

fn target_pretty(t: &ast::DefineOpsTarget) -> String {
    match t {
        ast::DefineOpsTarget::Role(r) => r.to_string(),
        ast::DefineOpsTarget::Down(r) => format!("{} down", r),
        ast::DefineOpsTarget::RoleDown(r) => format!("{}.down", r),
    }
}

// ─── Resources ─────────────────────────────────────────────

fn resource_pretty(r: &ast::ResourceDecl) -> String {
    let mut s = format!("resource {} {{\n", r.name);
    for cap in &r.capacities {
        if cap.name.name == "_" {
            s.push_str(&format!("  capacity: {},\n", type_pretty(&cap.ty)));
        } else {
            s.push_str(&format!(
                "  capacity {} = {},\n",
                cap.name,
                type_pretty(&cap.ty)
            ));
        }
    }
    for field in &r.fields {
        s.push_str(&format!("  field {}:", field.name));
        if let Some(default) = &field.default {
            s.push_str(&format!(
                " {} = {}",
                type_pretty(&field.ty),
                expr_pretty(default)
            ));
        } else {
            s.push_str(&format!(" {}", type_pretty(&field.ty)));
        }
        s.push('\n');
    }
    s.push_str("}");
    s
}

// ─── Devices ───────────────────────────────────────────────

fn device_pretty(d: &ast::DeviceDecl) -> String {
    let mut s = format!("device {} {{\n", d.name);
    for extent in &d.extents {
        s.push_str(&extent_pretty(extent));
    }
    for rate in &d.rates {
        s.push_str(&format!("  rate {}:\n", rate.name));
        s.push_str(&format!("    {}\n", type_pretty(&rate.rate)));
    }
    for rule in &d.cost_rules {
        s.push_str(&cost_rule_pretty(rule));
    }
    s.push_str("}");
    s
}

fn extent_pretty(e: &ast::ExtentDecl) -> String {
    match e {
        ast::ExtentDecl::Simple { name, ty, default } => {
            let mut s = format!("  extent {}", name);
            match ty {
                ast::ExtentType::Bytes => s.push_str(" bytes"),
                ast::ExtentType::Count(n) => s.push_str(&format!(" count {}", n)),
            }
            if let Some(d) = default {
                s.push_str(&format!(" default = {}", expr_pretty(d)));
            }
            s.push('\n');
            s
        }
        ast::ExtentDecl::Disk {
            name,
            size,
            mountpoint,
        } => {
            format!(
                "  extent {} size {} mountpoint {}\n",
                name,
                bytes_lit_pretty(size),
                expr_pretty(mountpoint)
            )
        }
    }
}

fn cost_rule_pretty(rule: &ast::CostRule) -> String {
    let mut s = "  cost rule {\n".to_string();
    for constraint in &rule.constraints {
        match constraint {
            ast::CostConstraint::SumLt { extent, pool } => {
                s.push_str(&format!("    extent {} <= {}\n", extent, expr_pretty(pool)));
            }
            ast::CostConstraint::SumOpLt {
                op,
                left_extent,
                right_extent,
                pool,
            } => {
                let op_str = match op {
                    ast::SumOp::Plus => "+",
                    ast::SumOp::Minus => "-",
                };
                s.push_str(&format!(
                    "    sum(cost {}) {} sum(cost {}) <= {}\n",
                    left_extent,
                    op_str,
                    right_extent,
                    expr_pretty(pool)
                ));
            }
        }
    }
    s.push('}');
    s
}

// ─── Machines ──────────────────────────────────────────────

fn machine_pretty(m: &ast::MachineDecl) -> String {
    let mut s = format!("machine {} {{\n", m.name);
    for ext in &m.extents {
        match ext {
            ast::MachineExtent::RAM(bytes) => {
                s.push_str(&format!("  RAM {}\n", bytes_lit_pretty(bytes)));
            }
            ast::MachineExtent::Disk(d) => {
                s.push_str(&format!(
                    "  disk {} {} mountpoint {}\n",
                    d.name,
                    bytes_lit_pretty(&d.size),
                    expr_pretty(&d.mountpoint)
                ));
            }
        }
    }
    for key in &m.keys {
        s.push_str(&format!("  key {}\n", bytes_lit_pretty(key)));
    }
    for dev in &m.devices {
        s.push_str(&format!("  device {} type {}\n", dev.name, dev.device_type));
    }
    s.push_str("}");
    s
}

// ─── Operations ────────────────────────────────────────────

fn operation_pretty(o: &ast::OperationDecl) -> String {
    let mut s = format!("operation {}(", o.name);
    for (i, param) in o.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("{} {}", param.ty, param.name));
    }
    s.push_str(") {\n");
    for req in &o.requires {
        s.push_str(&format!("  requires: {},\n", cond_pretty(req)));
    }
    if let Some(allow) = &o.allow {
        s.push_str(&format!("  allow if role is {},\n", allow));
    }
    for opt in &o.options {
        s.push_str(&operation_option_pretty(opt));
    }
    if let Some(cost) = &o.cost {
        s.push_str("  costs {\n");
        s.push_str(&cost_entries_pretty(&cost.costs, 2));
        s.push_str("  },\n");
    }
    s.push_str("}");
    s
}

fn operation_option_pretty(opt: &ast::OperationOption) -> String {
    let mut s = format!("  \"{}\" {{\n", opt.name);
    for stmt in &opt.body {
        s.push_str(&operation_stmt_pretty(stmt));
    }
    s.push_str("  },\n");
    s
}

fn operation_stmt_pretty(stmt: &ast::OperationStatement) -> String {
    match stmt {
        ast::OperationStatement::Require(cond) => format!("    requires: {}\n", cond_pretty(cond)),
        ast::OperationStatement::LetDecl(decl) => {
            format!("    let {}{}\n", decl.name, decl_ty_pretty(decl))
        }
        ast::OperationStatement::OnMachine(m) => format!("    on {}\n", m),
        ast::OperationStatement::ExecCommand { mode, cmd, args } => {
            let mode_s = match mode {
                ast::ExecMode::Batch => "",
                ast::ExecMode::Interactive => "interactive ",
            };
            let args_s = args.iter().map(expr_pretty).collect::<Vec<_>>().join(" ");
            if args_s.is_empty() {
                format!("    exec {mode_s}{cmd}\n")
            } else {
                format!("    exec {mode_s}{cmd} {args_s}\n")
            }
        }
        ast::OperationStatement::Transfer {
            from,
            machine,
            location,
        } => {
            format!(
                "    transfer {} to {} location {}\n",
                expr_pretty(from),
                expr_pretty(machine),
                expr_pretty(location)
            )
        }
        ast::OperationStatement::ShellCmd { cmd, args } => {
            format!("    shell {} {}\n", cmd, args.join(" "))
        }
        ast::OperationStatement::Choose(e) => {
            format!("    choose {{ {}: {} }}\n", e.variable, type_pretty(&e.ty))
        }
    }
}

// ─── Services ──────────────────────────────────────────────

fn service_pretty(s: &ast::ServiceDecl) -> String {
    let mut inner = String::new();
    if !s.costs.is_empty() {
        inner.push_str(&cost_entries_pretty(&s.costs, 2));
    }
    format!("service {}(", s.name)
        + &s.params
            .iter()
            .map(|p| format!("{} {}", p.ty, p.name))
            .collect::<Vec<_>>()
            .join(", ")
        + &format!(") on {} {{\n{}}}", s.on, inner)
}

fn cost_entries_pretty(costs: &[ast::CostEntry], indent_level: usize) -> String {
    let indent_str: String = (0..indent_level).map(|_| ' ').collect();
    let mut s = String::new();
    for cost in costs {
        s.push_str(&format!(
            "{}  {}: {}\n",
            indent_str,
            cost.kind,
            expr_pretty(&cost.value)
        ));
    }
    s
}

// ─── Functions ─────────────────────────────────────────────

fn function_pretty(f: &ast::FunctionDecl) -> String {
    let mut s = format!("function {}(", f.name);
    for (i, param) in f.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("{} {}", param.ty, param.name));
    }
    s.push_str(") {\n");
    for req in &f.requires {
        s.push_str(&format!("  requires: {},\n", cond_pretty(req)));
    }
    if let Some(allow) = &f.allow {
        s.push_str(&format!("  allow if role is {},\n", allow));
    }
    for stmt in &f.body {
        s.push_str(&function_stmt_pretty(stmt));
    }
    if let Some(success) = &f.success_if {
        s.push_str(&format!("  success if {}\n", cond_pretty(success)));
    }
    if let Some(fail) = &f.failure {
        match fail {
            ast::FailAction::Otherwise => s.push_str("  failure otherwise\n"),
        }
    }
    s.push_str("}");
    s
}

fn function_stmt_pretty(stmt: &ast::FunctionStatement) -> String {
    match stmt {
        ast::FunctionStatement::Require(cond) => format!("  requires: {}\n", cond_pretty(cond)),
        ast::FunctionStatement::LetDecl(decl) => {
            format!("  let {}{}\n", decl.name, decl_ty_pretty(decl))
        }
        ast::FunctionStatement::OnMachine(m) => format!("  on {}\n", m),
        ast::FunctionStatement::ExecCommand { mode, cmd, args } => {
            let mode_s = match mode {
                ast::ExecMode::Batch => "",
                ast::ExecMode::Interactive => "interactive ",
            };
            let args_s = args.iter().map(expr_pretty).collect::<Vec<_>>().join(" ");
            if args_s.is_empty() {
                format!("  exec {mode_s}{cmd}\n")
            } else {
                format!("  exec {mode_s}{cmd} {args_s}\n")
            }
        }
        ast::FunctionStatement::ReadJson { var } => format!("  read: {}\n", var),
        ast::FunctionStatement::WriteJson { value } => format!("  write {}\n", expr_pretty(value)),
        ast::FunctionStatement::Transfer {
            from,
            machine,
            location,
        } => {
            format!(
                "  transfer {} to {} location {}\n",
                expr_pretty(from),
                expr_pretty(machine),
                expr_pretty(location)
            )
        }
        ast::FunctionStatement::Dependency {
            service_name,
            service_param,
            on_machine,
            as_name,
        } => {
            format!(
                "  dependency {}{} on {} as {}\n",
                service_name,
                service_param
                    .as_ref()
                    .map_or(String::new(), |p| format!(" {}", expr_pretty(p))),
                on_machine,
                as_name
            )
        }
        ast::FunctionStatement::Return(e) => format!("  return {}\n", expr_pretty(e)),
        ast::FunctionStatement::SetEnv { name, secret } => {
            format!("  set {} = {}\n", name, secret_source_pretty(secret))
        }
    }
}

fn secret_source_pretty(src: &ast::SecretSource) -> String {
    match src {
        ast::SecretSource::CmdSecret { machine } => format!("cmd {} ", machine),
        ast::SecretSource::Env(name) => format!("env {}", name),
    }
}

// ─── Grants ────────────────────────────────────────────────

fn grant_pretty(g: &ast::GrantDecl) -> String {
    let mut s = format!("grant {} can {}", g.target_role, g.op);
    if g.resource.resource_type != ast::Type::Primitive(ast::PrimitiveType::JSON)
        || g.resource.variable.name != "_"
    {
        s.push_str(&format!(
            " {{ {}: {} }}",
            g.resource.variable,
            type_pretty(&g.resource.resource_type)
        ));
    }
    if let Some(cond) = &g.condition {
        s.push_str(&format!(" if {}", cond_pretty(cond)));
    }
    s
}

// ─── On-Machine ────────────────────────────────────────────

fn on_machine_pretty(o: &ast::OnMachineStmt) -> String {
    let mut s = format!("on {}", machines_pretty(&o.machines));
    if let Some(body) = &o.body {
        s.push_str(&format!(" {{{}}}", task_block_pretty(body)));
    } else {
        s.push(';');
    }
    s
}

fn machines_pretty(m: &ast::Machines) -> String {
    match m {
        ast::Machines::Single(n) => n.to_string(),
        ast::Machines::Set(n) => format!("machine set {}", n),
        ast::Machines::Inline(list) => list
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

// ─── Task Blocks ───────────────────────────────────────────

fn task_block_pretty(t: &ast::TaskBlock) -> String {
    let mut s = String::from("tasks { ");
    let items: Vec<String> = t.body.iter().map(task_item_pretty).collect();
    s.push_str(&items.join(", "));
    s.push_str(" }");
    s
}

fn task_item_pretty(item: &ast::TaskItem) -> String {
    match item {
        ast::TaskItem::Bind {
            variable,
            assignment,
        } => {
            format!("{} <- {}", variable, expr_pretty(assignment))
        }
        ast::TaskItem::OpCall { op, args } => {
            format!(
                "{} {}",
                op,
                args.iter().map(expr_pretty).collect::<Vec<_>>().join(" ")
            )
        }
        ast::TaskItem::OpCallArgs { op, args } => {
            format!(
                "{} {}",
                op,
                args.iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        ast::TaskItem::ExprTask(e) => expr_pretty(e),
        ast::TaskItem::RemoteWrite {
            variable,
            machine,
            path,
        } => {
            format!("{} @ {} = {}", variable, machine, expr_pretty(path))
        }
        ast::TaskItem::Optimize { metric } => format!("optimize for {}", metric),
        ast::TaskItem::Dependency {
            service_name,
            service_param,
            on_machine,
            as_name,
        } => {
            format!(
                "dependency {}{} on {} as {}",
                service_name,
                service_param
                    .as_ref()
                    .map_or(String::new(), |p| format!(" {}", expr_pretty(p))),
                on_machine,
                as_name
            )
        }
    }
}

// ─── Control Flow ──────────────────────────────────────────

fn control_flow_pretty(c: &ast::ControlFlow) -> String {
    match c {
        ast::ControlFlow::For(f) => for_loop_pretty(f),
        ast::ControlFlow::While(w) => while_loop_pretty(w),
        ast::ControlFlow::If(i) => if_pretty(i),
        ast::ControlFlow::TryCatch(t) => try_catch_pretty(t),
    }
}

fn for_loop_pretty(f: &ast::ForLoop) -> String {
    match f {
        ast::ForLoop::List {
            var,
            iterable,
            body,
        } => {
            format!(
                "for {} in {} {{ {} }}",
                var,
                expr_pretty(iterable),
                stmts_pretty(body)
            )
        }
        ast::ForLoop::Dict {
            key_var,
            value_var,
            iterable,
            body,
        } => {
            format!(
                "for {} and {} in {} {{ {} }}",
                key_var,
                value_var,
                expr_pretty(iterable),
                stmts_pretty(body)
            )
        }
    }
}

fn while_loop_pretty(w: &ast::WhileLoop) -> String {
    format!(
        "while {} is true {{ {} }}",
        w.can_tell,
        stmts_pretty(&w.body)
    )
}

fn if_pretty(i: &ast::IfStmt) -> String {
    let mut s = format!(
        "if {} {{ {} }}",
        expr_pretty(&i.condition),
        stmts_pretty(&i.then_body)
    );
    for (cond, body) in &i.else_if {
        s.push_str(&format!(
            " else if {} {{ {} }}",
            expr_pretty(cond),
            stmts_pretty(body)
        ));
    }
    if !i.else_body.is_empty() {
        s.push_str(&format!(" else {{ {} }}", stmts_pretty(&i.else_body)));
    }
    s
}

fn try_catch_pretty(t: &ast::TryCatch) -> String {
    let mut s = format!("try {{ {} }}", stmts_pretty(&t.body));
    if let Some(err_var) = &t.catch_err_var {
        s.push_str(&format!(" catch error {} {{ {} }}", err_var, stmts_pretty(&t.catch_body)));
    } else if !t.catch_body.is_empty() {
        s.push_str(&format!(" catch {{ {} }}", stmts_pretty(&t.catch_body)));
    }
    if !t.finally_body.is_empty() {
        s.push_str(&format!(" finally {{ {} }}", stmts_pretty(&t.finally_body)));
    }
    s
}

fn stmts_pretty(stmts: &[ast::Statement]) -> String {
    stmts.iter().map(stmt_pretty).collect::<Vec<_>>().join(" ")
}

// ─── Conditions ────────────────────────────────────────────

fn cond_pretty(c: &ast::Condition) -> String {
    c.predicates
        .iter()
        .map(|p| cond_pred_pretty(p))
        .collect::<Vec<_>>()
        .join(" and ")
}

fn cond_pred_pretty(p: &ast::ConditionPred) -> String {
    match p {
        ast::ConditionPred::Is { left, roles } => {
            format!(
                "{} is {}",
                expr_pretty(left),
                roles
                    .iter()
                    .map(|r| role_ref_pretty(r))
                    .collect::<Vec<_>>()
                    .join(" or ")
            )
        }
        ast::ConditionPred::Can { op, resource } => {
            let mut s = format!("can {}", op);
            if let Some(rp) = resource {
                s.push_str(&format!(
                    " {{ {}: {} }}",
                    rp.variable,
                    type_pretty(&rp.resource_type)
                ));
            }
            s
        }
        ast::ConditionPred::StartsWith { expr, prefix } => {
            format!("{} starts with \"{}\"", expr_pretty(expr), prefix)
        }
        ast::ConditionPred::EndsWith { expr, suffix } => {
            format!("{} ends with \"{}\"", expr_pretty(expr), suffix)
        }
        ast::ConditionPred::InSet { expr, set } => {
            format!("{} in {}", expr_pretty(expr), set)
        }
        ast::ConditionPred::Exists(e) => format!("{} exists", expr_pretty(e)),
        ast::ConditionPred::Matches { expr, pattern } => {
            format!("{} matches \"{}\"", expr_pretty(expr), pattern)
        }
        ast::ConditionPred::Not(inner) => format!("not {}", cond_pred_pretty(inner)),
        ast::ConditionPred::And(l, r) => {
            format!("{} and {}", cond_pred_pretty(l), cond_pred_pretty(r))
        }
        ast::ConditionPred::Or(l, r) => {
            format!("{} or {}", cond_pred_pretty(l), cond_pred_pretty(r))
        }
        ast::ConditionPred::DropPrefixEq {
            prefix,
            left,
            right,
        } => {
            format!(
                "drop \"{}\" {} is drop \"{}\" {}",
                prefix,
                expr_pretty(left),
                prefix,
                expr_pretty(right)
            )
        }
    }
}

fn role_ref_pretty(r: &ast::RoleRef) -> String {
    match r {
        ast::RoleRef::Exact(n) => n.to_string(),
        ast::RoleRef::Down(n) => format!("{} down", n),
        ast::RoleRef::RoleDown(n) => format!("{}.down", n),
    }
}

// ─── Types ─────────────────────────────────────────────────

fn type_pretty(t: &ast::Type) -> String {
    match t {
        ast::Type::Primitive(p) => p.to_string(),
        ast::Type::List(inner) => format!("[{}]", type_pretty(inner)),
        ast::Type::MutList(inner) => format!("[mut {}]", type_pretty(inner)),
        ast::Type::Map(key, val) => format!("map of {} to {}", type_pretty(key), type_pretty(val)),
        ast::Type::OrderedMap(key, val) => format!(
            "ordered map of {} to {}",
            type_pretty(key),
            type_pretty(val)
        ),
        ast::Type::Set(inner) => format!("set of {}", type_pretty(inner)),
        ast::Type::OrderedSet(inner) => format!("ordered set of {}", type_pretty(inner)),
        ast::Type::SizedList(inner, size) => {
            format!("new list of {}({})", type_pretty(inner), size)
        }
        ast::Type::Resource(name) => name.to_string(),
    }
}

fn decl_ty_pretty(d: &ast::LetDecl) -> String {
    let mut s = String::new();
    if let Some(ty) = &d.ty {
        s.push_str(&format!(": {}", type_pretty(ty)));
    }
    if let Some(init) = &d.init {
        s.push_str(&format!(" = {}", expr_pretty(init)));
    }
    s
}

// ─── Literals & Expressions ────────────────────────────────

fn bytes_lit_pretty(b: &ast::BytesLit) -> String {
    format!("{}{}", b.value, bytes_suffix_pretty(&b.suffix))
}

fn bytes_suffix_pretty(s: &ast::BytesSuffix) -> &'static str {
    match s {
        ast::BytesSuffix::None => "",
        ast::BytesSuffix::KB => "KB",
        ast::BytesSuffix::KiB => "KiB",
        ast::BytesSuffix::MB => "MB",
        ast::BytesSuffix::MiB => "MiB",
        ast::BytesSuffix::GB => "GB",
        ast::BytesSuffix::GiB => "GiB",
        ast::BytesSuffix::TB => "TB",
        ast::BytesSuffix::TiB => "TiB",
    }
}

fn expr_pretty(e: &Expr) -> String {
    match e {
        Expr::Lit(l) => match l {
            Literal::Bool(b) => b.to_string(),
            Literal::Int(i) => i.to_string(),
            Literal::Bytes(b) => bytes_lit_pretty(b),
            Literal::StringVal(s) => {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
        },
        Expr::Var(id) => id.to_string(),
        Expr::Struct { fields } => {
            format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, expr_pretty(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Expr::FieldAccess { target, field } => format!("{}.{}", expr_pretty(target), field),
        Expr::IndexAccess { target, index } => {
            format!("{}[{}]", expr_pretty(target), expr_pretty(index))
        }
        Expr::Call { func, args } => {
            format!(
                "{}({})",
                func,
                args.iter().map(expr_pretty).collect::<Vec<_>>().join(", ")
            )
        }
        Expr::BinOp { op, left, right } => format!(
            "{} {} {}",
            expr_pretty(left),
            bin_op_str(op),
            expr_pretty(right)
        ),
        Expr::UnOp { op, operand } => {
            if *op == UnOp::Neg {
                format!("-{}", expr_pretty(operand))
            } else {
                format!("not {}", expr_pretty(operand))
            }
        }
        Expr::Template(s) => format!("\"{}\"", s),
        Expr::Choose {
            variable,
            ty,
            from_set,
        } => {
            let mut s = format!("choose {{ {}: {} }}", variable, type_pretty(ty));
            if let Some(set) = from_set {
                s.push_str(&format!(" from {}", set));
            }
            s
        }
    }
}

fn bin_op_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Plus => "+",
        BinOp::Minus => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
    }
}

// ─── Tests ────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use crate::ast::expr::{Expr, Literal};
    use crate::ast::{BytesLit, BytesSuffix, Ident, ResourcePattern};

    fn ident(name: &str) -> ast::Ident {
        ast::Ident { name: name.into() }
    }

    fn var(name: &str) -> Expr {
        Expr::Var(ident(name))
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Lit(Literal::Int(n))
    }

    fn bool_lit(b: bool) -> Expr {
        Expr::Lit(Literal::Bool(b))
    }

    fn string_lit(s: &str) -> Expr {
        Expr::Lit(Literal::StringVal(s.into()))
    }

    #[test]
    fn test_pretty_print_empty_program() {
        let program = ast::Program {
            items: vec![],
            statements: vec![],
        };
        let result = pretty_print(&program);
        // No items or statements → empty string
        assert_eq!(result, "");
    }

    #[test]
    fn test_pretty_print_let_statement() {
        let stmt = ast::Statement::OnMachine(ast::OnMachineStmt {
            machines: ast::Machines::Single(ident("x")),
            body: Some(Box::new(ast::TaskBlock {
                machines: ast::Machines::Single(ident("x")),
                body: vec![ast::TaskItem::ExprTask(Box::new(Expr::BinOp {
                    op: BinOp::Plus,
                    left: Box::new(int_lit(1)),
                    right: Box::new(int_lit(2)),
                }))],
            })),
        });
        let program = ast::Program {
            items: vec![],
            statements: vec![stmt],
        };
        let result = pretty_print(&program);
        assert!(result.contains("on"));
    }

    #[test]
    fn test_pretty_print_if_statement() {
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::If(ast::IfStmt {
            condition: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(var("x")),
                right: Box::new(int_lit(1)),
            }),
            then_body: vec![ast::Statement::OnMachine(ast::OnMachineStmt {
                machines: ast::Machines::Single(ident("x")),
                body: None,
            })],
            else_if: vec![],
            else_body: vec![],
        }));
        let program = ast::Program {
            items: vec![],
            statements: vec![stmt],
        };
        let result = pretty_print(&program);
        assert!(result.contains("if"));
        assert!(result.contains("=="));
    }

    #[test]
    fn test_pretty_print_binops() {
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(Expr::BinOp {
                op: BinOp::Mul,
                left: Box::new(int_lit(2)),
                right: Box::new(int_lit(3)),
            }),
            right: Box::new(int_lit(4)),
        };
        let result = expr_pretty(&expr);
        // Pretty printer does not add parentheses
        assert_eq!(result, "2 * 3 + 4");
    }

    #[test]
    fn test_pretty_print_unops() {
        let neg = expr_pretty(&Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(int_lit(5)),
        });
        assert_eq!(neg, "-5");

        let not = expr_pretty(&Expr::UnOp {
            op: UnOp::Not,
            operand: Box::new(bool_lit(true)),
        });
        assert_eq!(not, "not true");
    }

    #[test]
    fn test_pretty_print_call_expr() {
        let call = expr_pretty(&Expr::Call {
            func: ident("len"),
            args: vec![var("items")],
        });
        assert!(call.contains("len"));
        assert!(call.contains("items"));
    }

    #[test]
    fn test_pretty_print_field_access() {
        let expr = Expr::FieldAccess {
            target: Box::new(var("machine")),
            field: ident("name"),
        };
        assert_eq!(expr_pretty(&expr), "machine.name");
    }

    #[test]
    fn test_pretty_print_index_access() {
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(int_lit(0)),
        };
        assert_eq!(expr_pretty(&expr), "items[0]");
    }

    #[test]
    fn test_pretty_print_struct_literal() {
        let expr = Expr::Struct {
            fields: vec![
                (ident("name"), Box::new(string_lit("test"))),
                (ident("count"), Box::new(int_lit(1))),
            ],
        };
        let result = expr_pretty(&expr);
        assert!(result.contains("name"));
        assert!(result.contains("count"));
    }

    #[test]
    fn test_pretty_print_choose_expr() {
        let expr = Expr::Choose {
            variable: ident("m"),
            ty: ast::Type::Resource(ident("Machine")),
            from_set: None,
        };
        let result = expr_pretty(&expr);
        assert!(result.contains("choose"));
        assert!(result.contains("m"));
        assert!(result.contains("Machine"));
    }

    #[test]
    fn test_pretty_print_choose_with_from() {
        let expr = Expr::Choose {
            variable: ident("m"),
            ty: ast::Type::Resource(ident("Machine")),
            from_set: Some(ident("all_machines")),
        };
        let result = expr_pretty(&expr);
        assert!(result.contains("from"));
    }

    #[test]
    fn test_pretty_print_template_literal() {
        let expr = expr_pretty(&Expr::Template("hello world".into()));
        assert!(expr.contains("hello"));
    }

    #[test]
    fn test_pretty_print_string_literal_escaping() {
        let expr = Expr::Lit(Literal::StringVal(r#"he said "hi""#.into()));
        let result = expr_pretty(&expr);
        assert!(result.contains(r#"he said \"hi\""#));
    }

    #[test]
    fn test_pretty_print_bytes_literal() {
        let bytes = Expr::Lit(Literal::Bytes(BytesLit {
            value: 1024,
            suffix: BytesSuffix::KiB,
        }));
        let result = expr_pretty(&bytes);
        assert_eq!(result, "1024KiB");
    }

    #[test]
    fn test_pretty_print_list_type() {
        let t = type_pretty(&ast::Type::List(Box::new(ast::Type::Primitive(ast::PrimitiveType::JSON))));
        assert_eq!(t, "[JSON]");
    }

    #[test]
    fn test_pretty_print_map_type() {
        let t = type_pretty(&ast::Type::Map(
            Box::new(ast::Type::Primitive(ast::PrimitiveType::JSON)),
            Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)),
        ));
        assert_eq!(t, "map of JSON to Int");
    }

    #[test]
    fn test_pretty_print_bytes_suffix_none() {
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::None), "");
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::MB), "MB");
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::GiB), "GiB");
    }

    #[test]
    fn test_pretty_print_machines_inline() {
        let machines = ast::Machines::Inline(vec![ident("s1"), ident("s2")]);
        let result = machines_pretty(&machines);
        assert!(result.contains("s1"));
        assert!(result.contains("s2"));
    }

    #[test]
    fn test_pretty_print_machines_set() {
        let result = machines_pretty(&ast::Machines::Set(Ident { name: "my_set".into() }));
        assert_eq!(result, "machine set my_set");
    }

    #[test]
    fn test_pretty_print_condition_exists() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Exists(Box::new(var("x")))],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, "x exists");
    }

    #[test]
    fn test_pretty_print_condition_starts_with() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::StartsWith {
                expr: Box::new(var("name")),
                prefix: "test".into(),
            }],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, r#"name starts with "test""#);
    }

    #[test]
    fn test_pretty_print_condition_matches() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Matches {
                expr: Box::new(var("version")),
                pattern: r#"1\..*"#.into(),
            }],
        };
        let result = cond_pretty(&cond);
        assert!(result.contains(r#"matches "1\..*""#), "got: {}", result);
    }

    #[test]
    fn test_pretty_print_condition_in_set() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::InSet {
                expr: Box::new(var("role")),
                set: ident("web_roles"),
            }],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, "role in web_roles");
    }

    #[test]
    fn test_pretty_print_condition_not() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Not(Box::new(
                ast::ConditionPred::Exists(Box::new(var("x")))
            ))],
        };
        let result = cond_pretty(&cond);
        assert!(result.starts_with("not "));
    }

    #[test]
    fn test_pretty_print_condition_and_or() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::And(
                Box::new(ast::ConditionPred::Exists(Box::new(var("a")))),
                Box::new(ast::ConditionPred::Exists(Box::new(var("b")))),
            )],
        };
        let result = cond_pretty(&cond);
        assert!(result.contains(" and "));

        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Or(
                Box::new(ast::ConditionPred::Exists(Box::new(var("a")))),
                Box::new(ast::ConditionPred::Exists(Box::new(var("b")))),
            )],
        };
        let result = cond_pretty(&cond);
        assert!(result.contains(" or "));
    }

    #[test]
    fn test_pretty_print_condition_drop_prefix() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::DropPrefixEq {
                prefix: "test".into(),
                left: Box::new(var("a")),
                right: Box::new(var("b")),
            }],
        };
        let result = cond_pretty(&cond);
        assert!(result.contains("drop"));
    }

    #[test]
    fn test_pretty_print_condition_can() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Can {
                op: ident("create"),
                resource: None,
            }],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, "can create");
    }

    #[test]
    fn test_pretty_print_condition_is_role() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Is {
                left: var("role"),
                roles: vec![ast::RoleRef::Exact(Ident { name: "web_admin".into() })],
            }],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, "role is web_admin");
    }

    #[test]
    fn test_pretty_print_condition_is_role_down() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Is {
                left: var("role"),
                roles: vec![ast::RoleRef::Down(Ident { name: "web".into() })],
            }],
        };
        let result = cond_pretty(&cond);
        assert_eq!(result, "role is web down");
    }

    #[test]
    fn test_pretty_print_condition_can_with_resource() {
        let cond = ast::Condition {
            predicates: vec![ast::ConditionPred::Can {
                op: ident("read"),
                resource: Some(ResourcePattern {
                    variable: ident("server"),
                    resource_type: ast::Type::Resource(ident("Server")),
                }),
            }],
        };
        let result = cond_pretty(&cond);
        assert!(result.contains("read"));
        assert!(result.contains("Server"));
    }

    #[test]
    fn test_pretty_print_binops_all() {
        assert_eq!(bin_op_str(&BinOp::Eq), "==");
        assert_eq!(bin_op_str(&BinOp::Neq), "!=");
        assert_eq!(bin_op_str(&BinOp::Lt), "<");
        assert_eq!(bin_op_str(&BinOp::Le), "<=");
        assert_eq!(bin_op_str(&BinOp::Gt), ">");
        assert_eq!(bin_op_str(&BinOp::Ge), ">=");
        assert_eq!(bin_op_str(&BinOp::And), "and");
        assert_eq!(bin_op_str(&BinOp::Or), "or");
        assert_eq!(bin_op_str(&BinOp::Plus), "+");
        assert_eq!(bin_op_str(&BinOp::Minus), "-");
        assert_eq!(bin_op_str(&BinOp::Mul), "*");
        assert_eq!(bin_op_str(&BinOp::Div), "/");
    }

    #[test]
    fn test_pretty_print_role_ref() {
        assert_eq!(role_ref_pretty(&ast::RoleRef::Exact(Ident { name: "web".into() })), "web");
        assert_eq!(role_ref_pretty(&ast::RoleRef::Down(Ident { name: "web".into() })), "web down");
        assert_eq!(role_ref_pretty(&ast::RoleRef::RoleDown(Ident { name: "web".into() })), "web.down");
    }

    #[test]
    fn test_pretty_print_secret_source_env() {
        let src = ast::SecretSource::Env(Ident { name: "DB_URL".into() });
        let result = secret_source_pretty(&src);
        assert!(result.contains("env"));
        assert!(result.contains("DB_URL"));
    }

    #[test]
    fn test_pretty_print_set_type() {
        let ty = ast::Type::Set(Box::new(ast::Type::Primitive(ast::PrimitiveType::String)));
        let result = type_pretty(&ty);
        assert_eq!(result, "set of String");
    }

    #[test]
    fn test_pretty_print_ordered_map_type() {
        let ty = ast::Type::OrderedMap(
            Box::new(ast::Type::Primitive(ast::PrimitiveType::String)),
            Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)),
        );
        let result = type_pretty(&ty);
        assert_eq!(result, "ordered map of String to Int");
    }

    #[test]
    fn test_pretty_print_mut_list_type() {
        let ty = ast::Type::MutList(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)));
        let result = type_pretty(&ty);
        assert_eq!(result, "[mut Int]");
    }

    #[test]
    fn test_pretty_print_bytes_suffix_all() {
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::KB), "KB");
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::MB), "MB");
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::GB), "GB");
        assert_eq!(bytes_suffix_pretty(&ast::BytesSuffix::TB), "TB");
    }
}
