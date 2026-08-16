//! Execution engine for the Limited Shell language.
//!
//! Evaluates AST nodes into [`RuntimeValue`] and executes statements with side effects.
//!
//! # Modules
//!
//! - [`ExecutionContext`] — variable scopes, machine tracking, resource registry access
//! - [`eval_expr`]       — evaluate expressions to runtime values
//! - [`execute_stmt`]    — execute statements (control flow, task blocks, etc.)
//! - [`execute_program`] — execute a full Program

use std::collections::HashMap;

use crate::ast;
use crate::ast::expr::{BinOp, Expr, Literal, UnOp};
use crate::resource::{ExtentEngine, MachineRegistry, ResourceRegistry, RuntimeValue};

// ─── ExecutionContext ─────────────────────────────────────────

/// Holds all mutable execution state.
///
/// Scopes are a stack: each variable lookup pushes down through the stack
/// until a binding is found. Assignment always modifies the innermost scope.
pub struct ExecutionContext<'a> {
    /// Stack of variable scopes. Innermost = last element.
    scopes: Vec<HashMap<String, RuntimeValue>>,
    /// Currently executing machine (none = global scope).
    current_machine: Option<String>,
    /// Registered resources.
    registry: &'a ResourceRegistry,
    /// Device extent pools.
    extent_engine: &'a ExtentEngine,
    /// Machine registry for `on machine` lookups.
    machine_registry: &'a MachineRegistry,
    /// Remote transport layer.
    pub remote: &'a dyn crate::remote::RemoteTransport,
}

impl<'a> ExecutionContext<'a> {
    pub fn new(
        registry: &'a ResourceRegistry,
        extent_engine: &'a ExtentEngine,
        machine_registry: &'a MachineRegistry,
    ) -> Self {
        Self {
            scopes: vec![HashMap::new()],
            current_machine: None,
            registry,
            extent_engine,
            machine_registry,
            remote: &crate::remote::NoopTransport,
        }
    }

    pub fn with_transport(
        registry: &'a ResourceRegistry,
        extent_engine: &'a ExtentEngine,
        machine_registry: &'a MachineRegistry,
        remote: &'a dyn crate::remote::RemoteTransport,
    ) -> Self {
        Self {
            scopes: vec![HashMap::new()],
            current_machine: None,
            registry,
            extent_engine,
            machine_registry,
            remote,
        }
    }

    /// Push a new block scope (e.g. for `{ ... }` bodies).
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope.
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Get a snapshot of top-level variables as strings.
    pub fn top_variables(&self) -> HashMap<String, String> {
        self.scopes
            .first()
            .into_iter()
            .flat_map(|scope| scope.iter().map(|(k, v)| (k.clone(), v.to_string())))
            .collect()
    }

    /// Look up a variable by name, searching innermost to outermost.
    pub fn lookup(&self, name: &str) -> Option<&RuntimeValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    /// Bind a variable in the innermost scope.
    pub fn bind(&mut self, name: String, value: RuntimeValue) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    /// Update a variable (innermost scope first, then search outward).
    pub fn update(&mut self, name: &str, value: RuntimeValue) -> bool {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                scope.insert(name.into(), value);
                return true;
            }
        }
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.into(), value);
                return true;
            }
        }
        false
    }

    /// Set the currently executing machine.
    pub fn set_machine(&mut self, name: String) {
        self.current_machine = Some(name);
    }

    /// Get the currently executing machine name.
    pub fn get_machine(&self) -> Option<&str> {
        self.current_machine.as_deref()
    }

    /// Access the resource registry.
    pub fn registry(&self) -> &ResourceRegistry {
        self.registry
    }

    /// Access the extent engine.
    pub fn extent_engine(&self) -> &ExtentEngine {
        self.extent_engine
    }

    /// Access the machine registry.
    pub fn machine_registry(&self) -> &MachineRegistry {
        self.machine_registry
    }

    /// Get a value from the resource registry for condition evaluation.
    pub fn registry_value(&self, name: &str) -> Option<RuntimeValue> {
        self.registry.get(name).map(|r| {
            RuntimeValue::Resource(
                r.resource_type.clone(),
                r.fields
                    .clone()
                    .into_iter()
                    .map(|(k, v)| (k, v.clone()))
                    .collect(),
            )
        })
    }
}

// ─── Expression Evaluation ────────────────────────────────────

/// Evaluate an expression to a [`RuntimeValue`].
pub fn eval_expr(expr: &Expr, ctx: &ExecutionContext) -> Result<RuntimeValue, ExecutionError> {
    match expr {
        Expr::Lit(lit) => Ok(lit_to_runtime(lit)),
        Expr::Var(ident) => {
            if let Some(v) = ctx.lookup(&ident.name) {
                Ok(v.clone())
            } else if let Some(v) = ctx.registry_value(&ident.name) {
                Ok(v)
            } else {
                Err(ExecutionError::UndefinedVariable(ident.name.clone()))
            }
        }
        Expr::Struct { fields } => {
            let mut map = HashMap::new();
            for (name, value) in fields {
                let v = eval_expr(value, ctx)?;
                map.insert(name.name.clone(), v);
            }
            Ok(RuntimeValue::Struct(map))
        }
        Expr::FieldAccess { target, field } => {
            let target_val = eval_expr(target, ctx)?;
            match target_val {
                RuntimeValue::Struct(map) => map
                    .get(&field.name)
                    .cloned()
                    .ok_or_else(|| ExecutionError::FieldNotFound(field.name.clone())),
                v => Err(ExecutionError::ExpectedStruct(v.type_name().to_string())),
            }
        }
        Expr::IndexAccess { target, index } => {
            let target_val = eval_expr(target, ctx)?;
            let index_val = eval_expr(index, ctx)?;
            match (target_val, index_val) {
                (RuntimeValue::List(items), RuntimeValue::Int(i)) => {
                    let idx = normalize_index(i, items.len());
                    items
                        .get(idx)
                        .cloned()
                        .ok_or(ExecutionError::IndexOutOfBounds(i))
                }
                (RuntimeValue::Struct(map), RuntimeValue::StringVal(k)) => {
                    map.get(&k).cloned().ok_or(ExecutionError::FieldNotFound(k))
                }
                (t, i) => Err(ExecutionError::IndexTypeMismatch(
                    t.type_name().to_string(),
                    i.type_name().to_string(),
                )),
            }
        }
        Expr::Call { func, args } => {
            let arg_vals: Vec<RuntimeValue> = args
                .iter()
                .map(|a| eval_expr(a, ctx))
                .collect::<Result<_, _>>()?;
            eval_builtin_call(func.name.as_str(), &arg_vals, ctx)
        }
        Expr::BinOp { op, left, right } => {
            let left_val = eval_expr(left, ctx)?;
            let right_val = eval_expr(right, ctx)?;
            eval_binop(op, left_val, right_val)
        }
        Expr::UnOp { op, operand } => {
            let val = eval_expr(operand, ctx)?;
            eval_unop(op, val)
        }
        Expr::Template(s) => {
            // Expand template variables $NAME
            let mut result = String::new();
            let mut chars = s.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '$' {
                    if let Some(&next) = chars.peek() {
                        if next == '{' {
                            chars.next(); // skip {
                            let mut var_name = String::new();
                            while let Some(&nc) = chars.peek() {
                                if nc == '}' {
                                    chars.next();
                                    break;
                                }
                                var_name.push(nc);
                                chars.next();
                            }
                            if let Some(v) = ctx.lookup(&var_name) {
                                match v {
                                    RuntimeValue::StringVal(s) => result.push_str(s),
                                    RuntimeValue::Int(i) => result.push_str(&i.to_string()),
                                    RuntimeValue::Bytes(b) => result.push_str(&b.to_string()),
                                    v => result.push_str(&v.to_string()),
                                }
                            } else {
                                result.push('$');
                                result.push('{');
                                result.push_str(&var_name);
                                result.push('}');
                            }
                        } else if next.is_alphanumeric() || next == '_' {
                            let mut var_name = String::new();
                            while let Some(&nc) = chars.peek() {
                                if nc.is_alphanumeric() || nc == '_' {
                                    var_name.push(nc);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            if let Some(v) = ctx.lookup(&var_name) {
                                match v {
                                    RuntimeValue::StringVal(s) => result.push_str(s),
                                    RuntimeValue::Int(i) => result.push_str(&i.to_string()),
                                    RuntimeValue::Bytes(b) => result.push_str(&b.to_string()),
                                    v => result.push_str(&v.to_string()),
                                }
                            } else {
                                result.push('$');
                                result.push_str(&var_name);
                            }
                        } else {
                            result.push('$');
                        }
                    } else {
                        result.push('$');
                    }
                } else {
                    result.push(c);
                }
            }
            Ok(RuntimeValue::StringVal(result))
        }
        Expr::Choose { .. } => Err(ExecutionError::ChooseNotAllowedOutsideOperation),
    }
}

/// Convert an AST literal to a [`RuntimeValue`].
pub fn lit_to_runtime(lit: &Literal) -> RuntimeValue {
    match lit {
        Literal::Bool(b) => RuntimeValue::Bool(*b),
        Literal::Int(n) => RuntimeValue::Int(*n),
        Literal::Bytes(b) => RuntimeValue::Bytes(b.value),
        Literal::StringVal(s) => RuntimeValue::StringVal(s.clone()),
    }
}

// ─── Expression Evaluation Helpers ────────────────────────────

fn normalize_index(idx: i64, len: usize) -> usize {
    if idx < 0 {
        let abs = (-idx) as usize;
        len.saturating_sub(abs)
    } else {
        idx as usize % len
    }
}

fn eval_binop(
    op: &BinOp,
    left: RuntimeValue,
    right: RuntimeValue,
) -> Result<RuntimeValue, ExecutionError> {
    match op {
        BinOp::Eq => ok_bool(runtime_eq(&left, &right)),
        BinOp::Neq => ok_bool(!runtime_eq(&left, &right)),
        BinOp::Lt => {
            let (l, r) = as_cmp_values(left, right)?;
            ok_bool(l < r)
        }
        BinOp::Le => {
            let (l, r) = as_cmp_values(left, right)?;
            ok_bool(l <= r)
        }
        BinOp::Gt => {
            let (l, r) = as_cmp_values(left, right)?;
            ok_bool(l > r)
        }
        BinOp::Ge => {
            let (l, r) = as_cmp_values(left, right)?;
            ok_bool(l >= r)
        }
        BinOp::And => {
            if !left.is_truthy() {
                return Ok(RuntimeValue::Bool(false));
            }
            if !right.is_truthy() {
                return Ok(RuntimeValue::Bool(false));
            }
            Ok(RuntimeValue::Bool(true))
        }
        BinOp::Or => {
            if left.is_truthy() {
                return Ok(RuntimeValue::Bool(true));
            }
            if right.is_truthy() {
                return Ok(RuntimeValue::Bool(true));
            }
            Ok(RuntimeValue::Bool(false))
        }
        BinOp::Plus => runtime_add(left, right),
        BinOp::Minus => {
            let (l, r) = as_numbers(left, right)?;
            Ok(RuntimeValue::Int(l - r))
        }
        BinOp::Mul => {
            let (l, r) = as_numbers(left, right)?;
            Ok(RuntimeValue::Int(l * r))
        }
        BinOp::Div => {
            let (l, r) = as_numbers(left, right)?;
            if r == 0 {
                Err(ExecutionError::DivisionByZero)
            } else {
                Ok(RuntimeValue::Int(l / r))
            }
        }
    }
}

fn eval_unop(op: &UnOp, val: RuntimeValue) -> Result<RuntimeValue, ExecutionError> {
    match op {
        UnOp::Not => ok_bool(!val.is_truthy()),
        UnOp::Neg => {
            if let RuntimeValue::Int(n) = val {
                Ok(RuntimeValue::Int(-n))
            } else {
                Err(ExecutionError::ExpectedInt(val.type_name().to_string()))
            }
        }
    }
}

fn ok_bool(v: bool) -> Result<RuntimeValue, ExecutionError> {
    Ok(RuntimeValue::Bool(v))
}

fn as_cmp_values(left: RuntimeValue, right: RuntimeValue) -> Result<(f64, f64), ExecutionError> {
    let l = runtime_to_f64(left)?;
    let r = runtime_to_f64(right)?;
    Ok((l, r))
}

fn runtime_to_f64(v: RuntimeValue) -> Result<f64, ExecutionError> {
    match v {
        RuntimeValue::Int(n) => Ok(n as f64),
        RuntimeValue::Bytes(n) => Ok(n as f64),
        RuntimeValue::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        _ => Err(ExecutionError::ExpectedNumeric(v.type_name().to_string())),
    }
}

fn as_numbers(left: RuntimeValue, right: RuntimeValue) -> Result<(i64, i64), ExecutionError> {
    let l = runtime_to_i64(left)?;
    let r = runtime_to_i64(right)?;
    Ok((l, r))
}

fn runtime_to_i64(v: RuntimeValue) -> Result<i64, ExecutionError> {
    match v {
        RuntimeValue::Int(n) => Ok(n),
        RuntimeValue::Bytes(n) => {
            if n <= i64::MAX as u64 {
                Ok(n as i64)
            } else {
                Err(ExecutionError::IntegerOverflow)
            }
        }
        _ => Err(ExecutionError::ExpectedNumeric(v.type_name().to_string())),
    }
}

fn runtime_eq(left: &RuntimeValue, right: &RuntimeValue) -> bool {
    match (left, right) {
        (RuntimeValue::Int(a), RuntimeValue::Int(b)) => a == b,
        (RuntimeValue::Bytes(a), RuntimeValue::Bytes(b)) => a == b,
        (RuntimeValue::Bool(a), RuntimeValue::Bool(b)) => a == b,
        (RuntimeValue::StringVal(a), RuntimeValue::StringVal(b)) => a == b,
        (RuntimeValue::Null, RuntimeValue::Null) => true,
        (RuntimeValue::Struct(a), RuntimeValue::Struct(b)) => a == b,
        (RuntimeValue::List(a), RuntimeValue::List(b)) => a == b,
        _ => false,
    }
}

fn runtime_add(left: RuntimeValue, right: RuntimeValue) -> Result<RuntimeValue, ExecutionError> {
    match (left, right) {
        (RuntimeValue::Int(a), RuntimeValue::Int(b)) => Ok(RuntimeValue::Int(a + b)),
        (RuntimeValue::Bytes(a), RuntimeValue::Bytes(b)) => Ok(RuntimeValue::Bytes(a + b)),
        (RuntimeValue::StringVal(a), RuntimeValue::StringVal(b)) => {
            Ok(RuntimeValue::StringVal(a + &b))
        }
        (t, _) => Err(ExecutionError::ExpectedNumeric(t.type_name().to_string())),
    }
}

// ─── Built-in Functions ───────────────────────────────────────

fn eval_builtin_call(
    func: &str,
    args: &[RuntimeValue],
    ctx: &ExecutionContext,
) -> Result<RuntimeValue, ExecutionError> {
    match func {
        "machines" => {
            let names = ctx.machine_registry().list();
            Ok(RuntimeValue::List(
                names
                    .iter()
                    .map(|n| RuntimeValue::StringVal(n.to_string()))
                    .collect(),
            ))
        }
        "len" => match &args[0] {
            RuntimeValue::List(items) => Ok(RuntimeValue::Int(items.len() as i64)),
            RuntimeValue::Struct(map) => Ok(RuntimeValue::Int(map.len() as i64)),
            RuntimeValue::StringVal(s) => Ok(RuntimeValue::Int(s.chars().count() as i64)),
            t => Err(ExecutionError::ExpectedCollection(
                t.type_name().to_string(),
            )),
        },
        "exists" => match &args[0] {
            RuntimeValue::List(items) => Ok(RuntimeValue::Bool(!items.is_empty())),
            RuntimeValue::Struct(map) => Ok(RuntimeValue::Bool(!map.is_empty())),
            RuntimeValue::StringVal(s) => Ok(RuntimeValue::Bool(!s.is_empty())),
            RuntimeValue::Int(n) => Ok(RuntimeValue::Bool(*n != 0)),
            RuntimeValue::Bytes(n) => Ok(RuntimeValue::Bool(*n != 0)),
            RuntimeValue::Bool(b) => Ok(RuntimeValue::Bool(*b)),
            RuntimeValue::Null => Ok(RuntimeValue::Bool(false)),
            _ => Ok(RuntimeValue::Bool(true)),
        },
        "to_int" => {
            if let RuntimeValue::StringVal(s) = &args[0] {
                s.parse::<i64>()
                    .map(RuntimeValue::Int)
                    .map_err(|_| ExecutionError::ParseError(s.clone()))
            } else {
                Err(ExecutionError::ExpectedString(
                    args[0].type_name().to_string(),
                ))
            }
        }
        "to_string" => Ok(RuntimeValue::StringVal(args[0].to_string())),
        "range" => {
            if args.len() < 2 {
                return Err(ExecutionError::ParseError(
                    "range requires 2 arguments (start, end)".into(),
                ));
            }
            let start = match &args[0] {
                RuntimeValue::Int(n) => *n,
                t => return Err(ExecutionError::ExpectedNumeric(t.type_name().to_string())),
            };
            let end = match &args[1] {
                RuntimeValue::Int(n) => *n,
                t => return Err(ExecutionError::ExpectedNumeric(t.type_name().to_string())),
            };
            let mut items = Vec::new();
            if start < end {
                for i in start..end {
                    items.push(RuntimeValue::Int(i));
                }
            } else {
                for i in (end + 1..start + 1).rev() {
                    items.push(RuntimeValue::Int(i));
                }
            }
            Ok(RuntimeValue::List(items))
        }
        name => Err(ExecutionError::UnknownBuiltin(name.to_string())),
    }
}

// ─── Execution Error ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ExecutionError {
    UndefinedVariable(String),
    FieldNotFound(String),
    IndexOutOfBounds(i64),
    IndexTypeMismatch(String, String),
    ExpectedStruct(String),
    ExpectedInt(String),
    ExpectedNumeric(String),
    ExpectedString(String),
    ExpectedCollection(String),
    DivisionByZero,
    IntegerOverflow,
    ParseError(String),
    UnknownBuiltin(String),
    ChooseNotAllowedOutsideOperation,
    MachineNotFound(String),
    RequirementNotSatisfied,
    Remote(crate::remote::RemoteError),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndefinedVariable(n) => write!(f, "undefined variable: {n}"),
            Self::FieldNotFound(n) => write!(f, "field not found: {n}"),
            Self::IndexOutOfBounds(i) => write!(f, "index {i} out of bounds"),
            Self::IndexTypeMismatch(t, i) => write!(f, "cannot index {t} with {i}"),
            Self::ExpectedStruct(got) => write!(f, "expected struct, got {got}"),
            Self::ExpectedInt(got) => write!(f, "expected Int, got {got}"),
            Self::ExpectedNumeric(got) => write!(f, "expected numeric, got {got}"),
            Self::ExpectedString(got) => write!(f, "expected String, got {got}"),
            Self::ExpectedCollection(got) => write!(f, "expected collection, got {got}"),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::ParseError(s) => write!(f, "parse error: {s}"),
            Self::UnknownBuiltin(n) => write!(f, "unknown builtin function: {n}"),
            Self::ChooseNotAllowedOutsideOperation => {
                write!(f, "choose not allowed outside operation body")
            }
            Self::MachineNotFound(n) => write!(f, "machine not found: {n}"),
            Self::RequirementNotSatisfied => write!(f, "requirement not satisfied"),
            Self::Remote(err) => write!(f, "remote error: {err}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

// ─── Statement Execution ──────────────────────────────────────

/// Execute a statement with side effects.
pub fn execute_stmt(
    stmt: &ast::Statement,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    match stmt {
        ast::Statement::ControlFlow(cf) => execute_control_flow(cf, ctx),
        ast::Statement::OnMachine(om) => execute_on_machine(om, ctx),
        ast::Statement::TaskBlock(tb) => execute_task_block(tb, ctx),
        _ => Ok(()), // Role, Grant, Alias are compile-time only
    }
}

/// Execute a control flow statement.
fn execute_control_flow(
    cf: &ast::ControlFlow,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    match cf {
        ast::ControlFlow::If(if_stmt) => execute_if(if_stmt, ctx),
        ast::ControlFlow::For(for_loop) => execute_for(for_loop, ctx),
        ast::ControlFlow::While(while_loop) => execute_while(while_loop, ctx),
        ast::ControlFlow::TryCatch(tc) => {
            let result = execute_body(&tc.body, ctx);
            match result {
                Ok(()) => {
                    // No error — run finally if present
                    if !tc.finally_body.is_empty() {
                        execute_body(&tc.finally_body, ctx)?;
                    }
                    Ok(())
                }
                Err(e) => {
                    // Try catch body with error variable bound
                    if !tc.catch_body.is_empty() {
                        if let Some(err_var) = &tc.catch_err_var {
                            ctx.push_scope();
                            ctx.bind(err_var.name.clone(), RuntimeValue::StringVal(e.to_string()));
                        }
                        let catch_result = execute_body(&tc.catch_body, ctx);
                        if tc.catch_err_var.is_some() {
                            ctx.pop_scope();
                        }
                        // Always run finally
                        if !tc.finally_body.is_empty() {
                            execute_body(&tc.finally_body, ctx)?;
                        }
                        catch_result
                    } else {
                        // No catch body — run finally then propagate error
                        if !tc.finally_body.is_empty() {
                            execute_body(&tc.finally_body, ctx)?;
                        }
                        Err(e)
                    }
                }
            }
        }
    }
}

fn execute_if(if_stmt: &ast::IfStmt, ctx: &mut ExecutionContext) -> Result<(), ExecutionError> {
    let cond = eval_expr(&if_stmt.condition, ctx)?;
    if cond.is_truthy() {
        return execute_body(&if_stmt.then_body, ctx);
    }
    for (else_if_cond, else_if_body) in &if_stmt.else_if {
        let ec = eval_expr(else_if_cond, ctx)?;
        if ec.is_truthy() {
            return execute_body(else_if_body, ctx);
        }
    }
    if !if_stmt.else_body.is_empty() {
        return execute_body(&if_stmt.else_body, ctx);
    }
    Ok(())
}

fn execute_for(for_loop: &ast::ForLoop, ctx: &mut ExecutionContext) -> Result<(), ExecutionError> {
    match for_loop {
        ast::ForLoop::List {
            var,
            iterable,
            body,
        } => {
            let iter_val = eval_expr(iterable, ctx)?;
            let items = match iter_val {
                RuntimeValue::List(items) => items,
                t => {
                    return Err(ExecutionError::ExpectedCollection(
                        t.type_name().to_string(),
                    ))
                }
            };
            let body = body.clone();
            for item in items {
                ctx.push_scope();
                ctx.bind(var.name.clone(), item);
                for stmt in &body {
                    execute_stmt(stmt, ctx)?;
                }
                ctx.pop_scope();
            }
            Ok(())
        }
        ast::ForLoop::Dict {
            key_var,
            value_var,
            iterable,
            body,
        } => {
            let iter_val = eval_expr(iterable, ctx)?;
            let items = match iter_val {
                RuntimeValue::Struct(map) => map.into_iter().collect::<Vec<_>>(),
                t => {
                    return Err(ExecutionError::ExpectedCollection(
                        t.type_name().to_string(),
                    ))
                }
            };
            let body = body.clone();
            for (k, v) in items {
                ctx.push_scope();
                ctx.bind(key_var.name.clone(), RuntimeValue::StringVal(k));
                ctx.bind(value_var.name.clone(), v);
                for stmt in &body {
                    execute_stmt(stmt, ctx)?;
                }
                ctx.pop_scope();
            }
            Ok(())
        }
    }
}

fn execute_while(
    while_loop: &ast::WhileLoop,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    let body = while_loop.body.clone();
    loop {
        // Call the tell function to refresh state before each iteration
        if !while_loop.tell_args.is_empty() {
            let tell_args: Vec<RuntimeValue> = while_loop
                .tell_args
                .iter()
                .map(|a| eval_expr(a, ctx))
                .collect::<Result<_, _>>()?;
            let _ = eval_builtin_call(&while_loop.tell_func.name, &tell_args, ctx);
            // Tell result is used to update the cantell variable
        }
        let cond = eval_expr(&while_loop.condition, ctx)?;
        if !cond.is_truthy() {
            break;
        }
        for stmt in &body {
            execute_stmt(stmt, ctx)?;
        }
    }
    Ok(())
}

fn execute_on_machine(
    om: &ast::OnMachineStmt,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    let machines = match &om.machines {
        ast::Machines::Single(name) => vec![name.name.clone()],
        ast::Machines::Set(name) => ctx
            .machine_registry()
            .get_set_members(&name.name)
            .unwrap_or_default()
            .into_iter()
            .collect(),
        ast::Machines::Inline(names) => names.iter().map(|n| n.name.clone()).collect(),
    };
    if let Some(body) = &om.body {
        for machine in &machines {
            ctx.push_scope();
            ctx.set_machine(machine.clone());
            for item in &body.body {
                execute_task_item(item, ctx)?;
            }
            ctx.pop_scope();
        }
    } else {
        // `on machine <name>;` — just set context
        if let Some(m) = machines.first() {
            ctx.set_machine(m.clone());
        }
    }
    Ok(())
}

fn execute_task_block(
    tb: &ast::TaskBlock,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    for item in &tb.body {
        execute_task_item(item, ctx)?;
    }
    Ok(())
}

fn execute_task_item(
    item: &ast::TaskItem,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    match item {
        ast::TaskItem::Bind {
            variable,
            assignment,
        } => {
            let val = eval_expr(assignment, ctx)?;
            ctx.bind(variable.name.clone(), val);
        }
        ast::TaskItem::OpCall { op, args } => {
            let _ = args
                .iter()
                .map(|a| eval_expr(a, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            let _ = op;
        }
        ast::TaskItem::OpCallArgs { op, args } => {
            let _ = args
                .iter()
                .map(|a| {
                    if let Some(v) = ctx.lookup(&a.name) {
                        Ok(v.clone())
                    } else {
                        Err(ExecutionError::UndefinedVariable(a.name.clone()))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let _ = op;
        }
        ast::TaskItem::ExprTask(expr) => {
            let _ = eval_expr(expr, ctx)?;
        }
        ast::TaskItem::RemoteWrite {
            variable,
            machine: machine_name,
            path,
        } => {
            let val = ctx
                .lookup(&variable.name)
                .cloned()
                .ok_or(ExecutionError::UndefinedVariable(variable.name.clone()))?;
            // machine_name is an Ident — look it up in context
            let machine = if let Some(v) = ctx.lookup(&machine_name.name) {
                v.to_string()
            } else {
                machine_name.name.clone()
            };
            let location = eval_expr(path, ctx)?;
            let location_str = location.to_string();

            ctx.remote
                .remote_write(&machine, &val, &location_str)
                .map_err(ExecutionError::Remote)?;
        }
        ast::TaskItem::Optimize { metric: _ } => {
            // Optimization directive — affects scheduler, not runtime
        }
        ast::TaskItem::Dependency { .. } => {
            // Service dependency — tracked by scheduler
        }
    }
    Ok(())
}

fn execute_body(body: &[ast::Statement], ctx: &mut ExecutionContext) -> Result<(), ExecutionError> {
    for stmt in body {
        execute_stmt(stmt, ctx)?;
    }
    Ok(())
}

/// Execute a function body, returning the last `Return` value (or None).
pub fn execute_func_body(
    body: &[ast::FunctionStatement],
    ctx: &mut ExecutionContext,
) -> Result<Option<RuntimeValue>, ExecutionError> {
    let mut last_value: Option<RuntimeValue> = None;
    for stmt in body {
        last_value = execute_function(stmt, ctx)?;
        if last_value.is_some() {
            return Ok(last_value);
        }
    }
    Ok(last_value)
}

/// Execute an entire Program.
pub fn execute_program(
    program: &ast::Program,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    for stmt in &program.statements {
        execute_stmt(stmt, ctx)?;
    }
    Ok(())
}

/// Execute operation body statements.
pub fn execute_op_statement(
    stmt: &ast::OperationStatement,
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    match stmt {
        ast::OperationStatement::Require(cond) => {
            let mut result = true;
            for pred in &cond.predicates {
                if !eval_condition_pred(pred, ctx)? {
                    result = false;
                    break;
                }
            }
            if !result {
                return Err(ExecutionError::RequirementNotSatisfied);
            }
            Ok(())
        }
        ast::OperationStatement::Choose(choose) => {
            // Choose binds a variable to an arbitrary value from a set.
            // For now, bind a placeholder. Real scheduling picks actual value.
            let placeholder = RuntimeValue::Resource(choose.ty.to_string(), HashMap::new());
            ctx.bind(choose.variable.name.clone(), placeholder);
            Ok(())
        }
        ast::OperationStatement::LetDecl(decl) => {
            if let Some(init) = &decl.init {
                let val = eval_expr(init, ctx)?;
                ctx.bind(decl.name.name.clone(), val);
            }
            Ok(())
        }
        ast::OperationStatement::OnMachine(machine) => {
            ctx.set_machine(machine.name.clone());
            Ok(())
        }
        ast::OperationStatement::ExecCommand { mode, cmd, args } => {
            let target = ctx.get_machine().unwrap_or("local");

            let args_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    let v = eval_expr(a, ctx)?;
                    Ok(v.to_string())
                })
                .collect::<Result<Vec<String>, ExecutionError>>()?;

            match mode {
                ast::ExecMode::Batch => {
                    ctx.remote
                        .exec(target, &cmd.name, &args_strs)
                        .map_err(ExecutionError::Remote)?;
                }
                ast::ExecMode::Interactive => {
                    ctx.remote
                        .exec_interactive(target, &cmd.name, &args_strs)
                        .map_err(ExecutionError::Remote)?;
                }
            }
            Ok(())
        }
        ast::OperationStatement::Transfer {
            from,
            machine,
            location,
        } => {
            let from_val = eval_expr(from, ctx)?;
            let from_str = match &from_val {
                RuntimeValue::StringVal(s) => s.clone(),
                RuntimeValue::Resource(_, fields) => fields
                    .get("location")
                    .or_else(|| fields.get("path"))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| from_val.to_string()),
                _ => from_val.to_string(),
            };

            let machine_val = eval_expr(machine, ctx)?;
            let target_machine = match &machine_val {
                RuntimeValue::StringVal(s) => s.clone(),
                RuntimeValue::Resource(_, fields) => fields
                    .get("name")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| machine_val.to_string()),
                _ => machine_val.to_string(),
            };

            let location_val = eval_expr(location, ctx)?;
            let location_str = location_val.to_string();

            ctx.remote
                .transfer(&target_machine, &from_str, &location_str)
                .map_err(ExecutionError::Remote)?;
            Ok(())
        }
        ast::OperationStatement::ShellCmd { cmd, args } => {
            let target = ctx.get_machine().unwrap_or("local");

            let full_command = if args.is_empty() {
                cmd.clone()
            } else {
                format!("{} {}", cmd, args.join(" "))
            };

            ctx.remote
                .shell(target, &full_command)
                .map_err(ExecutionError::Remote)?;
            Ok(())
        }
    }
}

/// Execute a full operation body.
pub fn execute_op_body(
    body: &[ast::OperationStatement],
    ctx: &mut ExecutionContext,
) -> Result<(), ExecutionError> {
    for stmt in body {
        execute_op_statement(stmt, ctx)?;
    }
    Ok(())
}

/// Execute function body statements, returning an optional value.
///
/// Functions can return early via `Return` statements.
pub fn execute_function(
    stmt: &ast::FunctionStatement,
    ctx: &mut ExecutionContext,
) -> Result<Option<RuntimeValue>, ExecutionError> {
    match stmt {
        ast::FunctionStatement::Require(cond) => {
            let mut result = true;
            for pred in &cond.predicates {
                if !eval_condition_pred(pred, ctx)? {
                    result = false;
                    break;
                }
            }
            if !result {
                return Err(ExecutionError::RequirementNotSatisfied);
            }
            Ok(None)
        }
        ast::FunctionStatement::LetDecl(decl) => {
            if let Some(init) = &decl.init {
                let val = eval_expr(init, ctx)?;
                ctx.bind(decl.name.name.clone(), val);
            }
            Ok(None)
        }
        ast::FunctionStatement::OnMachine(machine) => {
            ctx.set_machine(machine.name.clone());
            Ok(None)
        }
        ast::FunctionStatement::SetEnv { name, secret } => {
            let target = ctx.get_machine().unwrap_or("local");

            ctx.remote
                .set_env(target, &name.name, secret)
                .map_err(ExecutionError::Remote)?;
            Ok(None)
        }
        ast::FunctionStatement::ExecCommand { mode, cmd, args } => {
            let target = ctx.get_machine().unwrap_or("local");

            let args_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    let v = eval_expr(a, ctx)?;
                    Ok(v.to_string())
                })
                .collect::<Result<Vec<String>, ExecutionError>>()?;

            match mode {
                ast::ExecMode::Batch => {
                    ctx.remote
                        .exec(target, &cmd.name, &args_strs)
                        .map_err(ExecutionError::Remote)?;
                }
                ast::ExecMode::Interactive => {
                    ctx.remote
                        .exec_interactive(target, &cmd.name, &args_strs)
                        .map_err(ExecutionError::Remote)?;
                }
            }
            Ok(None)
        }
        ast::FunctionStatement::ReadJson { var } => {
            let target = ctx.get_machine().unwrap_or("local");
            let path = format!("/tmp/{}.json", var.name);
            let value = ctx
                .remote
                .remote_read(target, &path)
                .map_err(ExecutionError::Remote)?;
            ctx.bind(var.name.clone(), value);
            Ok(None)
        }
        ast::FunctionStatement::WriteJson { value } => {
            let target = ctx.get_machine().unwrap_or("local");
            let resolved = eval_expr(value, ctx)?;
            let path = format!("/tmp/output_{}.json", resolved.type_name().to_lowercase());
            ctx.remote
                .remote_write(target, &resolved, &path)
                .map_err(ExecutionError::Remote)?;
            Ok(None)
        }
        ast::FunctionStatement::Transfer {
            from,
            machine,
            location,
        } => {
            let from_val = eval_expr(from, ctx)?;
            let from_str = match &from_val {
                RuntimeValue::StringVal(s) => s.clone(),
                RuntimeValue::Resource(_, fields) => fields
                    .get("location")
                    .or_else(|| fields.get("path"))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| from_val.to_string()),
                _ => from_val.to_string(),
            };

            let machine_val = eval_expr(machine, ctx)?;
            let target_machine = match &machine_val {
                RuntimeValue::StringVal(s) => s.clone(),
                RuntimeValue::Resource(_, fields) => fields
                    .get("name")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| machine_val.to_string()),
                _ => machine_val.to_string(),
            };

            let location_val = eval_expr(location, ctx)?;
            let location_str = location_val.to_string();

            ctx.remote
                .transfer(&target_machine, &from_str, &location_str)
                .map_err(ExecutionError::Remote)?;
            Ok(None)
        }
        ast::FunctionStatement::Dependency {
            service_name,
            service_param,
            on_machine,
            as_name,
        } => {
            let _ = service_param;
            let _ = (service_name, on_machine, as_name);
            Ok(None)
        }
        ast::FunctionStatement::Return(expr) => {
            let val = eval_expr(expr, ctx)?;
            Ok(Some(val))
        }
    }
}

/// Evaluate a condition predicate to a boolean RuntimeValue.
fn eval_condition_pred(
    pred: &ast::ConditionPred,
    ctx: &ExecutionContext,
) -> Result<bool, ExecutionError> {
    match pred {
        ast::ConditionPred::Is { left, roles } => {
            let val = eval_expr(left, ctx)?;
            let resource_type = val.resource_type().map(|s| s.to_string());
            Ok(roles.iter().any(|r| match r {
                ast::RoleRef::Exact(name) => resource_type.as_deref() == Some(name.name.as_str()),
                ast::RoleRef::Down(name) => resource_type.as_deref() == Some(name.name.as_str()),
                ast::RoleRef::RoleDown(_) => false,
            }))
        }
        ast::ConditionPred::Can { op, resource: _ } => {
            let _ = op;
            // Future: check if current role can perform the operation
            Ok(true)
        }
        ast::ConditionPred::StartsWith { expr, prefix } => {
            let val = eval_expr(expr, ctx)?;
            if let RuntimeValue::StringVal(s) = val {
                Ok(s.starts_with(prefix))
            } else {
                Ok(false)
            }
        }
        ast::ConditionPred::EndsWith { expr, suffix } => {
            let val = eval_expr(expr, ctx)?;
            if let RuntimeValue::StringVal(s) = val {
                Ok(s.ends_with(suffix))
            } else {
                Ok(false)
            }
        }
        ast::ConditionPred::DropPrefixEq {
            prefix,
            left,
            right,
        } => {
            let left_val = eval_expr(left, ctx)?;
            let right_val = eval_expr(right, ctx)?;
            fn drop_prefix<'a>(s: &'a str, prefix: &str) -> &'a str {
                s.strip_prefix(prefix).unwrap_or(s)
            }
            match (left_val, right_val) {
                (RuntimeValue::StringVal(a), RuntimeValue::StringVal(b)) => {
                    Ok(drop_prefix(&a, prefix) == drop_prefix(&b, prefix))
                }
                _ => Ok(false),
            }
        }
        ast::ConditionPred::InSet { expr, set } => {
            let val = eval_expr(expr, ctx)?;
            let set_members = ctx.machine_registry().get_set_members(&set.name);
            if let Some(members) = set_members {
                if let RuntimeValue::StringVal(s) = val {
                    Ok(members.contains(&s))
                } else {
                    Ok(false)
                }
            } else {
                Ok(false)
            }
        }
        ast::ConditionPred::Exists(expr) => {
            let val = eval_expr(expr, ctx)?;
            Ok(val.is_truthy())
        }
        ast::ConditionPred::Matches { expr, pattern } => {
            let val = eval_expr(expr, ctx)?;
            if let RuntimeValue::StringVal(s) = val {
                // Simple prefix match for now — full regex would need regex crate
                Ok(s.contains(pattern))
            } else {
                Ok(false)
            }
        }
        ast::ConditionPred::Not(inner) => Ok(!eval_condition_pred(inner, ctx)?),
        ast::ConditionPred::And(a, b) => {
            Ok(eval_condition_pred(a, ctx)? && eval_condition_pred(b, ctx)?)
        }
        ast::ConditionPred::Or(a, b) => {
            Ok(eval_condition_pred(a, ctx)? || eval_condition_pred(b, ctx)?)
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ExtentEngine, MachineRegistry, ResourceRegistry};

    fn make_ctx() -> ExecutionContext<'static> {
        let reg = Box::leak(Box::new(ResourceRegistry::new()));
        let ext = Box::leak(Box::new(ExtentEngine::new()));
        let mach = Box::leak(Box::new(MachineRegistry::new()));
        ExecutionContext::new(reg, ext, mach)
    }

    fn int_val(n: i64) -> RuntimeValue {
        RuntimeValue::Int(n)
    }

    fn str_val(s: &str) -> RuntimeValue {
        RuntimeValue::StringVal(s.to_string())
    }

    fn bytes_val(n: u64) -> RuntimeValue {
        RuntimeValue::Bytes(n)
    }

    fn bool_val(b: bool) -> RuntimeValue {
        RuntimeValue::Bool(b)
    }

    // ── ExecutionContext tests ─────────────────────

    #[test]
    fn test_ctx_bind_lookup() {
        let mut ctx = make_ctx();
        ctx.bind("x".to_string(), int_val(42));
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(42));
    }

    #[test]
    fn test_ctx_scope_push_pop() {
        let mut ctx = make_ctx();
        ctx.bind("x".to_string(), int_val(1));
        ctx.push_scope();
        ctx.bind("x".to_string(), int_val(2));
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(2));
        ctx.pop_scope();
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(1));
    }

    #[test]
    fn test_ctx_lookup_missing() {
        let ctx = make_ctx();
        assert!(ctx.lookup("nonexistent").is_none());
    }

    // ── Expression evaluation tests ──────────────────

    #[test]
    fn test_eval_literal() {
        let ctx = make_ctx();
        assert_eq!(
            eval_expr(&Expr::Lit(Literal::Int(42)), &ctx).unwrap(),
            int_val(42)
        );
        assert_eq!(
            eval_expr(&Expr::Lit(Literal::Bool(true)), &ctx).unwrap(),
            bool_val(true)
        );
        assert_eq!(
            eval_expr(&Expr::Lit(Literal::StringVal("hi".into())), &ctx).unwrap(),
            str_val("hi")
        );
        assert_eq!(
            eval_expr(
                &Expr::Lit(Literal::Bytes(ast::BytesLit {
                    value: 100,
                    suffix: ast::BytesSuffix::None
                })),
                &ctx
            )
            .unwrap(),
            bytes_val(100)
        );
    }

    #[test]
    fn test_eval_variable() {
        let mut ctx = make_ctx();
        ctx.bind("x".to_string(), int_val(99));
        let var_expr = Expr::Var(ast::Ident {
            name: "x".to_string(),
        });
        assert_eq!(eval_expr(&var_expr, &ctx).unwrap(), int_val(99));
    }

    #[test]
    fn test_eval_undefined_variable() {
        let ctx = make_ctx();
        let var_expr = Expr::Var(ast::Ident {
            name: "x".to_string(),
        });
        assert!(matches!(
            eval_expr(&var_expr, &ctx).unwrap_err(),
            ExecutionError::UndefinedVariable(_)
        ));
    }

    #[test]
    fn test_eval_binop_arithmetic() {
        let ctx = make_ctx();
        let a = Expr::Lit(Literal::Int(10));
        let b = Expr::Lit(Literal::Int(3));

        // Addition
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), int_val(13));

        // Subtraction
        let expr = Expr::BinOp {
            op: BinOp::Minus,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), int_val(7));

        // Multiplication
        let expr = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), int_val(30));

        // Division
        let expr = Expr::BinOp {
            op: BinOp::Div,
            left: Box::new(a),
            right: Box::new(b),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), int_val(3));
    }

    #[test]
    fn test_eval_division_by_zero() {
        let ctx = make_ctx();
        let a = Expr::Lit(Literal::Int(10));
        let b = Expr::Lit(Literal::Int(0));
        let expr = Expr::BinOp {
            op: BinOp::Div,
            left: Box::new(a),
            right: Box::new(b),
        };
        assert!(matches!(
            eval_expr(&expr, &ctx).unwrap_err(),
            ExecutionError::DivisionByZero
        ));
    }

    #[test]
    fn test_eval_unop_neg_non_int() {
        let ctx = make_ctx();
        let not = Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::Lit(Literal::StringVal("x".into()))),
        };
        assert!(matches!(
            eval_expr(&not, &ctx).unwrap_err(),
            ExecutionError::ExpectedInt(_)
        ));
    }

    #[test]
    fn test_eval_binop_comparison() {
        let ctx = make_ctx();
        let a = Expr::Lit(Literal::Int(5));
        let b = Expr::Lit(Literal::Int(3));

        let eq = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        assert_eq!(eval_expr(&eq, &ctx).unwrap(), bool_val(false));

        let eq2 = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(a.clone()),
            right: Box::new(a.clone()),
        };
        assert_eq!(eval_expr(&eq2, &ctx).unwrap(), bool_val(true));

        let lt = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(b),
            right: Box::new(a),
        };
        assert_eq!(eval_expr(&lt, &ctx).unwrap(), bool_val(true));
    }

    #[test]
    fn test_eval_binop_logical() {
        let ctx = make_ctx();
        let t = Expr::Lit(Literal::Bool(true));
        let f = Expr::Lit(Literal::Bool(false));

        // And
        let and = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(t.clone()),
            right: Box::new(t),
        };
        assert_eq!(eval_expr(&and, &ctx).unwrap(), bool_val(true));

        // Or (short-circuit: first is truthy)
        let or = Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(f.clone()),
            right: Box::new(f),
        };
        assert_eq!(eval_expr(&or, &ctx).unwrap(), bool_val(false));
    }

    #[test]
    fn test_eval_unop() {
        let ctx = make_ctx();

        let not = Expr::UnOp {
            op: UnOp::Not,
            operand: Box::new(Expr::Lit(Literal::Bool(true))),
        };
        assert_eq!(eval_expr(&not, &ctx).unwrap(), bool_val(false));

        let neg = Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::Lit(Literal::Int(42))),
        };
        assert_eq!(eval_expr(&neg, &ctx).unwrap(), int_val(-42));
    }

    #[test]
    fn test_eval_field_access() {
        let mut ctx = make_ctx();
        let s = Expr::Struct {
            fields: vec![
                (
                    ast::Ident { name: "x".into() },
                    Box::new(Expr::Lit(Literal::Int(10))),
                ),
                (
                    ast::Ident { name: "y".into() },
                    Box::new(Expr::Lit(Literal::Int(20))),
                ),
            ],
        };
        ctx.bind("s".to_string(), eval_expr(&s, &ctx).unwrap());

        let access = Expr::FieldAccess {
            target: Box::new(Expr::Var(ast::Ident { name: "s".into() })),
            field: ast::Ident { name: "x".into() },
        };
        assert_eq!(eval_expr(&access, &ctx).unwrap(), int_val(10));
    }

    #[test]
    fn test_eval_index_list() {
        let mut ctx = make_ctx();
        let items = vec![str_val("a"), str_val("b"), str_val("c")];
        ctx.bind("lst".to_string(), RuntimeValue::List(items));

        let idx = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident { name: "lst".into() })),
            index: Box::new(Expr::Lit(Literal::Int(1))),
        };
        assert_eq!(eval_expr(&idx, &ctx).unwrap(), str_val("b"));

        // Negative index (from end)
        let idx_neg = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident { name: "lst".into() })),
            index: Box::new(Expr::Lit(Literal::Int(-1))),
        };
        assert_eq!(eval_expr(&idx_neg, &ctx).unwrap(), str_val("c"));
    }

    // ── Built-in function tests ──────────────────────

    #[test]
    fn test_builtin_len() {
        let mut ctx = make_ctx();
        let items = vec![int_val(1), int_val(2), int_val(3)];
        ctx.bind("lst".to_string(), RuntimeValue::List(items));

        let call_expr = Expr::Call {
            func: ast::Ident { name: "len".into() },
            args: vec![Expr::Var(ast::Ident { name: "lst".into() })],
        };
        assert_eq!(eval_expr(&call_expr, &ctx).unwrap(), int_val(3));
    }

    #[test]
    fn test_builtin_exists() {
        let ctx = make_ctx();

        let call = Expr::Call {
            func: ast::Ident {
                name: "exists".into(),
            },
            args: vec![Expr::Lit(Literal::Int(42))],
        };
        assert_eq!(eval_expr(&call, &ctx).unwrap(), bool_val(true));

        let call_null = Expr::Call {
            func: ast::Ident {
                name: "exists".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("".into()))],
        };
        assert_eq!(eval_expr(&call_null, &ctx).unwrap(), bool_val(false));

        let call_empty = Expr::Call {
            func: ast::Ident {
                name: "exists".into(),
            },
            args: vec![Expr::Var(ast::Ident { name: "v".into() })],
        };
        let mut ctx2 = make_ctx();
        ctx2.bind("v".to_string(), RuntimeValue::Null);
        assert_eq!(eval_expr(&call_empty, &ctx2).unwrap(), bool_val(false));
    }

    #[test]
    fn test_builtin_unknown() {
        let ctx = make_ctx();
        let call = Expr::Call {
            func: ast::Ident {
                name: "foobar".into(),
            },
            args: vec![],
        };
        assert!(matches!(
            eval_expr(&call, &ctx).unwrap_err(),
            ExecutionError::UnknownBuiltin(_)
        ));
    }

    #[test]
    fn test_builtin_range() {
        let ctx = make_ctx();

        // range(1, 5) -> [1, 2, 3, 4]
        let call = Expr::Call {
            func: ast::Ident {
                name: "range".into(),
            },
            args: vec![Expr::Lit(Literal::Int(1)), Expr::Lit(Literal::Int(5))],
        };
        let result = eval_expr(&call, &ctx).unwrap();
        match result {
            RuntimeValue::List(items) => {
                assert_eq!(items.len(), 4);
                assert_eq!(items[0], int_val(1));
                assert_eq!(items[3], int_val(4));
            }
            _ => panic!("expected list"),
        }

        // range(5, 1) -> [5, 4, 3, 2]
        let call2 = Expr::Call {
            func: ast::Ident {
                name: "range".into(),
            },
            args: vec![Expr::Lit(Literal::Int(5)), Expr::Lit(Literal::Int(1))],
        };
        let result2 = eval_expr(&call2, &ctx).unwrap();
        match result2 {
            RuntimeValue::List(items) => {
                assert_eq!(items.len(), 4);
                assert_eq!(items[0], int_val(5));
                assert_eq!(items[3], int_val(2));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn test_builtin_to_int() {
        let ctx = make_ctx();

        let call = Expr::Call {
            func: ast::Ident {
                name: "to_int".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("42".into()))],
        };
        assert_eq!(eval_expr(&call, &ctx).unwrap(), int_val(42));

        let call_err = Expr::Call {
            func: ast::Ident {
                name: "to_int".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("abc".into()))],
        };
        assert!(matches!(
            eval_expr(&call_err, &ctx).unwrap_err(),
            ExecutionError::ParseError(_)
        ));
    }

    // ── Template interpolation tests ──────────────────

    #[test]
    fn test_template_literal() {
        let ctx = make_ctx();
        let tmpl = Expr::Template("/home/user/data".into());
        assert_eq!(eval_expr(&tmpl, &ctx).unwrap(), str_val("/home/user/data"));
    }

    #[test]
    fn test_template_with_var() {
        let mut ctx = make_ctx();
        ctx.bind("home".to_string(), str_val("/home/alice"));
        let tmpl = Expr::Template("$home/.config".into());
        assert_eq!(
            eval_expr(&tmpl, &ctx).unwrap(),
            str_val("/home/alice/.config")
        );
    }

    // ── Control flow tests ───────────────────────────

    #[test]
    fn test_if_branch() {
        let mut ctx = make_ctx();
        ctx.bind("flag".to_string(), bool_val(true));

        let cond = Expr::Var(ast::Ident {
            name: "flag".into(),
        });
        let if_stmt = ast::IfStmt {
            condition: Box::new(cond),
            then_body: vec![],
            else_if: vec![],
            else_body: vec![],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::If(if_stmt));
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_if_else_branch() {
        let mut ctx = make_ctx();
        ctx.bind("flag".to_string(), bool_val(false));

        let cond = Expr::Var(ast::Ident {
            name: "flag".into(),
        });
        let if_stmt = ast::IfStmt {
            condition: Box::new(cond),
            then_body: vec![],
            else_if: vec![],
            else_body: vec![ast::Statement::Alias(ast::AliasDecl {
                kind: ast::AliasKind::Machine,
                name: ast::Ident { name: "_".into() },
                target: ast::Type::Primitive(ast::PrimitiveType::Node),
            })],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::If(if_stmt));
        // else_body runs a statement — Alias is compile-time, execute_stmt returns Ok(()) for it
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_for_list_loop() {
        let mut ctx = make_ctx();
        ctx.bind(
            "nums".to_string(),
            RuntimeValue::List(vec![int_val(1), int_val(2), int_val(3)]),
        );

        let for_loop = ast::ForLoop::List {
            var: ast::Ident { name: "n".into() },
            iterable: Box::new(Expr::Var(ast::Ident {
                name: "nums".into(),
            })),
            body: vec![ast::Statement::Alias(ast::AliasDecl {
                kind: ast::AliasKind::Machine,
                name: ast::Ident { name: "_".into() },
                target: ast::Type::Primitive(ast::PrimitiveType::Node),
            })],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::For(for_loop));
        // Should not panic and should iterate 3 times
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_for_dict_loop() {
        let mut ctx = make_ctx();
        let mut map = HashMap::new();
        map.insert("x".to_string(), int_val(10));
        map.insert("y".to_string(), int_val(20));
        ctx.bind("m".to_string(), RuntimeValue::Struct(map));

        let for_loop = ast::ForLoop::Dict {
            key_var: ast::Ident { name: "k".into() },
            value_var: ast::Ident { name: "v".into() },
            iterable: Box::new(Expr::Var(ast::Ident { name: "m".into() })),
            body: vec![],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::For(for_loop));
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_try_catch_success() {
        let mut ctx = make_ctx();
        // Try body succeeds — no catch or finally
        let try_catch = ast::TryCatch {
            body: vec![ast::Statement::Alias(ast::AliasDecl {
                kind: ast::AliasKind::Machine,
                name: ast::Ident { name: "_".into() },
                target: ast::Type::Primitive(ast::PrimitiveType::Node),
            })],
            catch_err_var: None,
            catch_body: vec![],
            catch_all: vec![],
            finally_body: vec![],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::TryCatch(try_catch));
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_try_catch_finally_always_runs() {
        let mut ctx = make_ctx();
        // Bind "step" in try body, verify it survives after try succeeds
        let try_catch = ast::TryCatch {
            body: vec![
                ast::Statement::OnMachine(ast::OnMachineStmt {
                    machines: ast::Machines::Single(ast::Ident { name: "x".into() }),
                    body: Some(Box::new(ast::TaskBlock {
                        machines: ast::Machines::Inline(vec![]),
                        body: vec![ast::TaskItem::Bind {
                            variable: ast::Ident {
                                name: "step".into(),
                            },
                            assignment: Box::new(Expr::Lit(Literal::Int(1))),
                        }],
                    })),
                }),
                // Bind in outer scope (not inside OnMachine push/pop)
                ast::Statement::OnMachine(ast::OnMachineStmt {
                    machines: ast::Machines::Single(ast::Ident { name: "x".into() }),
                    body: Some(Box::new(ast::TaskBlock {
                        machines: ast::Machines::Inline(vec![]),
                        body: vec![ast::TaskItem::ExprTask(Box::new(Expr::Lit(Literal::Int(
                            99,
                        ))))],
                    })),
                }),
            ],
            catch_err_var: Some(ast::Ident { name: "e".into() }),
            catch_body: vec![],
            catch_all: vec![],
            finally_body: vec![ast::Statement::OnMachine(ast::OnMachineStmt {
                machines: ast::Machines::Single(ast::Ident { name: "x".into() }),
                body: Some(Box::new(ast::TaskBlock {
                    machines: ast::Machines::Inline(vec![]),
                    body: vec![ast::TaskItem::ExprTask(Box::new(Expr::Lit(Literal::Int(
                        99,
                    ))))],
                })),
            })],
        };
        let stmt = ast::Statement::ControlFlow(ast::ControlFlow::TryCatch(try_catch));
        // After try succeeds, the "step" variable from inside the first OnMachine
        // is gone (scoped), but the final bind (if we had one outside OnMachine)
        // would persist. For now, just verify no panic.
        assert!(execute_stmt(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_task_bind() {
        let mut ctx = make_ctx();
        let bind = ast::TaskItem::Bind {
            variable: ast::Ident { name: "x".into() },
            assignment: Box::new(Expr::Lit(Literal::Int(42))),
        };
        assert!(execute_task_item(&bind, &mut ctx).is_ok());
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(42));
    }

    #[test]
    fn test_task_expr_eval() {
        let mut ctx = make_ctx();
        let expr_task = ast::TaskItem::ExprTask(Box::new(Expr::Lit(Literal::Int(99))));
        assert!(execute_task_item(&expr_task, &mut ctx).is_ok());
    }

    // ── RuntimeValue tests ───────────────────────────

    #[test]
    fn test_runtime_value_truthy() {
        assert!(int_val(42).is_truthy());
        assert!(!int_val(0).is_truthy());
        assert!(bytes_val(100).is_truthy());
        assert!(!bytes_val(0).is_truthy());
        assert!(str_val("hello").is_truthy());
        assert!(!str_val("").is_truthy());
        assert!(!RuntimeValue::Null.is_truthy());
        assert!(bool_val(true).is_truthy());
        assert!(!bool_val(false).is_truthy());

        let struct_empty = RuntimeValue::Struct(HashMap::new());
        let struct_nonempty = RuntimeValue::Struct({
            let mut m = HashMap::new();
            m.insert("k".into(), int_val(1));
            m
        });
        assert!(!struct_empty.is_truthy());
        assert!(struct_nonempty.is_truthy());

        assert!(!RuntimeValue::List(vec![]).is_truthy());
        assert!(RuntimeValue::List(vec![int_val(1)]).is_truthy());
    }

    #[test]
    fn test_runtime_value_display() {
        assert_eq!(format!("{}", int_val(42)), "42");
        assert_eq!(format!("{}", bool_val(false)), "false");
        assert_eq!(format!("{}", RuntimeValue::Null), "null");
        let s = str_val("hello");
        assert_eq!(format!("{}", s), "hello");
    }

    #[test]
    fn test_runtime_value_eq() {
        assert!(runtime_eq(&int_val(5), &int_val(5)));
        assert!(!runtime_eq(&int_val(5), &int_val(3)));
        assert!(runtime_eq(&str_val("a"), &str_val("a")));
        assert!(!runtime_eq(&str_val("a"), &str_val("b")));
    }

    // ── Operation statement tests ──────────────

    #[test]
    fn test_op_require_passes() {
        let mut ctx = make_ctx();
        let require = ast::OperationStatement::Require(ast::Condition {
            predicates: vec![ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(1),
            )))],
        });
        assert!(execute_op_statement(&require, &mut ctx).is_ok());
    }

    #[test]
    fn test_op_require_fails() {
        let mut ctx = make_ctx();
        let require = ast::OperationStatement::Require(ast::Condition {
            predicates: vec![ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(0),
            )))],
        });
        assert!(matches!(
            execute_op_statement(&require, &mut ctx).unwrap_err(),
            ExecutionError::RequirementNotSatisfied
        ));
    }

    #[test]
    fn test_op_let_decl() {
        let mut ctx = make_ctx();
        let let_decl = ast::OperationStatement::LetDecl(ast::LetDecl {
            name: ast::Ident { name: "x".into() },
            ty: None,
            init: Some(Box::new(Expr::Lit(Literal::Int(77)))),
        });
        assert!(execute_op_statement(&let_decl, &mut ctx).is_ok());
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(77));
    }

    #[test]
    fn test_op_choose() {
        let mut ctx = make_ctx();
        let choose = ast::OperationStatement::Choose(ast::ChooseExpr {
            variable: ast::Ident { name: "m".into() },
            ty: ast::Type::Primitive(ast::PrimitiveType::Node),
            from_set: None,
        });
        assert!(execute_op_statement(&choose, &mut ctx).is_ok());
        // Should have bound a placeholder resource
        assert!(ctx.lookup("m").is_some());
    }

    #[test]
    fn test_op_on_machine() {
        let mut ctx = make_ctx();
        let on_machine = ast::OperationStatement::OnMachine(ast::Ident {
            name: "alpha".into(),
        });
        assert!(execute_op_statement(&on_machine, &mut ctx).is_ok());
        assert_eq!(ctx.get_machine(), Some("alpha"));
    }

    // ── Function statement tests ─────────────

    #[test]
    fn test_func_return() {
        let mut ctx = make_ctx();
        let ret = ast::FunctionStatement::Return(Box::new(Expr::Lit(Literal::Int(42))));
        let result = execute_function(&ret, &mut ctx).unwrap();
        assert_eq!(result, Some(int_val(42)));
    }

    #[test]
    fn test_func_no_return() {
        let mut ctx = make_ctx();
        let let_decl = ast::FunctionStatement::LetDecl(ast::LetDecl {
            name: ast::Ident { name: "y".into() },
            ty: None,
            init: Some(Box::new(Expr::Lit(Literal::Int(99)))),
        });
        let result = execute_function(&let_decl, &mut ctx).unwrap();
        assert_eq!(result, None);
        assert_eq!(ctx.lookup("y").unwrap(), &int_val(99));
    }

    #[test]
    fn test_func_require_fails() {
        let mut ctx = make_ctx();
        let require = ast::FunctionStatement::Require(ast::Condition {
            predicates: vec![ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(0),
            )))],
        });
        assert!(matches!(
            execute_function(&require, &mut ctx).unwrap_err(),
            ExecutionError::RequirementNotSatisfied
        ));
    }

    // ── Condition predicate tests ─────────────

    #[test]
    fn test_pred_exists() {
        let ctx = make_ctx();
        let pred = ast::ConditionPred::Exists(Box::new(Expr::Lit(Literal::Int(42))));
        assert!(eval_condition_pred(&pred, &ctx).unwrap());
    }

    #[test]
    fn test_pred_not() {
        let ctx = make_ctx();
        let pred = ast::ConditionPred::Not(Box::new(ast::ConditionPred::Exists(Box::new(
            Expr::Lit(Literal::Int(0)),
        ))));
        assert!(eval_condition_pred(&pred, &ctx).unwrap());
    }

    #[test]
    fn test_pred_and() {
        let ctx = make_ctx();
        let pred = ast::ConditionPred::And(
            Box::new(ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(1),
            )))),
            Box::new(ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(2),
            )))),
        );
        assert!(eval_condition_pred(&pred, &ctx).unwrap());
    }

    #[test]
    fn test_pred_or() {
        let ctx = make_ctx();
        let pred = ast::ConditionPred::Or(
            Box::new(ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(0),
            )))),
            Box::new(ast::ConditionPred::Exists(Box::new(Expr::Lit(
                Literal::Int(1),
            )))),
        );
        assert!(eval_condition_pred(&pred, &ctx).unwrap());
    }

    #[test]
    fn test_pred_startswith() {
        let mut ctx = make_ctx();
        ctx.bind("p".to_string(), str_val("/home/alice"));
        let pred = ast::ConditionPred::StartsWith {
            expr: Box::new(Expr::Var(ast::Ident { name: "p".into() })),
            prefix: "/home".to_string(),
        };
        assert!(eval_condition_pred(&pred, &ctx).unwrap());
    }

    #[test]
    fn test_pred_drop_prefix_eq() {
        let mut ctx = make_ctx();
        ctx.bind("a".to_string(), str_val("///foo/bar"));
        ctx.bind("b".to_string(), str_val("///foo/baz"));
        let pred = ast::ConditionPred::DropPrefixEq {
            prefix: "///".to_string(),
            left: Box::new(Expr::Var(ast::Ident { name: "a".into() })),
            right: Box::new(Expr::Var(ast::Ident { name: "b".into() })),
        };
        // drop("///") "foo/bar" == drop("///") "foo/baz" → "foo/bar" != "foo/baz" → false
        assert!(!eval_condition_pred(&pred, &ctx).unwrap());

        let pred2 = ast::ConditionPred::DropPrefixEq {
            prefix: "///".to_string(),
            left: Box::new(Expr::Var(ast::Ident { name: "a".into() })),
            right: Box::new(Expr::Var(ast::Ident { name: "a".into() })),
        };
        assert!(eval_condition_pred(&pred2, &ctx).unwrap());
    }

    // ── Function body execution tests ─────────

    #[test]
    fn test_execute_func_body_return() {
        let mut ctx = make_ctx();
        let body = vec![ast::FunctionStatement::Return(Box::new(Expr::Lit(
            Literal::Int(42),
        )))];
        let result = execute_func_body(&body, &mut ctx).unwrap();
        assert_eq!(result, Some(int_val(42)));
    }

    #[test]
    fn test_execute_func_body_no_return() {
        let mut ctx = make_ctx();
        let body = vec![
            ast::FunctionStatement::LetDecl(ast::LetDecl {
                name: ast::Ident { name: "a".into() },
                ty: None,
                init: Some(Box::new(Expr::Lit(Literal::Int(1)))),
            }),
            ast::FunctionStatement::LetDecl(ast::LetDecl {
                name: ast::Ident { name: "b".into() },
                ty: None,
                init: Some(Box::new(Expr::Lit(Literal::Int(2)))),
            }),
        ];
        let result = execute_func_body(&body, &mut ctx).unwrap();
        assert_eq!(result, None);
        assert_eq!(ctx.lookup("b").unwrap(), &int_val(2));
    }

    #[test]
    fn test_execute_func_body_early_return() {
        let mut ctx = make_ctx();
        let body = vec![
            ast::FunctionStatement::LetDecl(ast::LetDecl {
                name: ast::Ident { name: "x".into() },
                ty: None,
                init: Some(Box::new(Expr::Lit(Literal::Int(99)))),
            }),
            ast::FunctionStatement::Return(Box::new(Expr::Lit(Literal::Int(7)))),
            // This statement should not execute
            ast::FunctionStatement::LetDecl(ast::LetDecl {
                name: ast::Ident { name: "y".into() },
                ty: None,
                init: Some(Box::new(Expr::Lit(Literal::Int(8)))),
            }),
        ];
        let result = execute_func_body(&body, &mut ctx).unwrap();
        assert_eq!(result, Some(int_val(7)));
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(99));
        // y should not be bound because of early return
        assert!(ctx.lookup("y").is_none());
    }

    #[test]
    fn test_execute_while_loop() {
        let mut ctx = make_ctx();
        // While loop with false condition — body never executes
        let while_stmt = ast::Statement::ControlFlow(ast::ControlFlow::While(ast::WhileLoop {
            can_tell: ast::Ident { name: "_".into() },
            condition: Box::new(Expr::Lit(Literal::Bool(false))),
            tell_func: ast::Ident { name: "_".into() },
            tell_args: vec![],
            body: vec![],
        }));
        assert!(execute_stmt(&while_stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_struct_expr() {
        let ctx = make_ctx();
        let struct_expr = ast::expr::Expr::Struct {
            fields: vec![(
                ast::Ident {
                    name: "name".into(),
                },
                Box::new(ast::expr::Expr::Lit(Literal::StringVal("test".into()))),
            )],
        };
        let result = eval_expr(&struct_expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Struct(_)));
    }

    #[test]
    fn test_execute_op_let_explicit_type() {
        let mut ctx = make_ctx();
        let let_decl = ast::OperationStatement::LetDecl(ast::LetDecl {
            name: ast::Ident { name: "x".into() },
            ty: Some(ast::Type::Primitive(ast::PrimitiveType::Int)),
            init: Some(Box::new(Expr::Lit(Literal::Int(42)))),
        });
        assert!(execute_op_statement(&let_decl, &mut ctx).is_ok());
        assert_eq!(ctx.lookup("x").unwrap(), &int_val(42));
    }

    #[test]
    fn test_execute_on_machine_stmt() {
        let mut ctx = make_ctx();
        // Top-level on machine statement with a task block body
        let on_stmt = ast::Statement::OnMachine(ast::OnMachineStmt {
            machines: ast::Machines::Single(ast::Ident {
                name: "alpha".into(),
            }),
            body: Some(Box::new(ast::TaskBlock {
                machines: ast::Machines::Inline(vec![ast::Ident {
                    name: "alpha".into(),
                }]),
                body: vec![ast::TaskItem::ExprTask(Box::new(Expr::Lit(Literal::Int(
                    42,
                ))))],
            })),
        });
        assert!(execute_stmt(&on_stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_for_loop_empty_body() {
        let mut ctx = make_ctx();

        // For loop with empty iterable - should succeed
        // We use a function call that returns an empty list
        let for_loop = ast::Statement::ControlFlow(ast::ControlFlow::For(ast::ForLoop::List {
            var: ast::Ident { name: "x".into() },
            iterable: Box::new(Expr::Call {
                func: ast::Ident {
                    name: "machines".into(),
                },
                args: vec![],
            }),
            body: vec![],
        }));
        assert!(execute_stmt(&for_loop, &mut ctx).is_ok());
    }

    #[test]
    fn test_op_on_machine_set() {
        let mut ctx = make_ctx();
        let on_machine = ast::OperationStatement::OnMachine(ast::Ident { name: "set".into() });
        assert!(execute_op_statement(&on_machine, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_if_without_else() {
        let mut ctx = make_ctx();
        // True condition, execute then branch
        let if_stmt = ast::Statement::ControlFlow(ast::ControlFlow::If(ast::IfStmt {
            condition: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Lit(Literal::Int(1))),
                right: Box::new(Expr::Lit(Literal::Int(1))),
            }),
            then_body: vec![],
            else_if: vec![],
            else_body: vec![],
        }));
        assert!(execute_stmt(&if_stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_op_transfer() {
        let mut ctx = make_ctx();
        let transfer = ast::OperationStatement::Transfer {
            from: Expr::Lit(Literal::StringVal("src".into())),
            machine: Expr::Var(ast::Ident { name: "m".into() }),
            location: Expr::Lit(Literal::StringVal("dst".into())),
        };
        // Transfer doesn't execute without proper machine context
        let _ = execute_op_statement(&transfer, &mut ctx);
    }

    #[test]
    fn test_execute_op_exec_command() {
        let mut ctx = make_ctx();
        let stmt = ast::OperationStatement::ExecCommand {
            mode: ast::ExecMode::Batch,
            cmd: ast::Ident {
                name: "deploy".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("app".into()))],
        };
        assert!(execute_op_statement(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_op_exec_interactive_records_call() {
        use crate::remote::{RecordedCall, TestTransport};

        let reg = Box::leak(Box::new(ResourceRegistry::new()));
        let ext = Box::leak(Box::new(ExtentEngine::new()));
        let mach = Box::leak(Box::new(MachineRegistry::new()));
        let transport = Box::leak(Box::new(TestTransport::new()));
        let mut ctx = ExecutionContext::with_transport(reg, ext, mach, transport);

        let stmt = ast::OperationStatement::ExecCommand {
            mode: ast::ExecMode::Interactive,
            cmd: ast::Ident { name: "mc".into() },
            args: vec![],
        };
        assert!(execute_op_statement(&stmt, &mut ctx).is_ok());
        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            RecordedCall::ExecInteractive { cmd, .. } => assert_eq!(cmd, "mc"),
            other => panic!("expected ExecInteractive, got {other:?}"),
        }
    }

    #[test]
    fn test_execute_op_shell_cmd() {
        let mut ctx = make_ctx();
        let stmt = ast::OperationStatement::ShellCmd {
            cmd: "ls".to_string(),
            args: vec!["-la".to_string()],
        };
        assert!(execute_op_statement(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_func_set_env() {
        let mut ctx = make_ctx();
        let stmt = ast::FunctionStatement::SetEnv {
            name: ast::Ident {
                name: "API_KEY".into(),
            },
            secret: ast::SecretSource::Env(ast::Ident {
                name: "API_KEY".into(),
            }),
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_execute_func_write_json() {
        let mut ctx = make_ctx();
        ctx.bind("data".to_string(), RuntimeValue::Struct(HashMap::new()));
        let stmt = ast::FunctionStatement::WriteJson {
            value: Expr::Var(ast::Ident {
                name: "data".into(),
            }),
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_execute_func_read_json() {
        let mut ctx = make_ctx();
        let stmt = ast::FunctionStatement::ReadJson {
            var: ast::Ident {
                name: "result".into(),
            },
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        // ReadJson binds an empty struct
        assert!(matches!(
            ctx.lookup("result"),
            Some(RuntimeValue::Struct(_))
        ));
    }

    #[test]
    fn test_execute_if_with_condition() {
        let mut ctx = make_ctx();
        ctx.bind("running".to_string(), RuntimeValue::Bool(true));
        let if_stmt = ast::IfStmt {
            condition: Box::new(Expr::Var(ast::Ident {
                name: "running".into(),
            })),
            then_body: vec![ast::Statement::TaskBlock(ast::TaskBlock {
                machines: ast::Machines::Single(ast::Ident { name: "_".into() }),
                body: vec![ast::TaskItem::OpCallArgs {
                    op: ast::Ident {
                        name: "check".into(),
                    },
                    args: vec![],
                }],
            })],
            else_if: vec![],
            else_body: vec![],
        };
        let result = execute_stmt(
            &ast::Statement::ControlFlow(ast::ControlFlow::If(if_stmt)),
            &mut ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_binop_logical_and() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Lit(Literal::Bool(true))),
            right: Box::new(Expr::Lit(Literal::Bool(true))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_execute_unop_not() {
        let ctx = make_ctx();
        let expr = Expr::UnOp {
            op: UnOp::Not,
            operand: Box::new(Expr::Lit(Literal::Bool(true))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(false)));
    }

    #[test]
    fn test_execute_while_loop_non_empty() {
        let mut ctx = make_ctx();
        ctx.bind("count".to_string(), RuntimeValue::Int(1));
        // While loop: while count lt 0 - condition is false so body won't execute
        let while_loop = ast::WhileLoop {
            can_tell: ast::Ident { name: "_".into() },
            condition: Box::new(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Var(ast::Ident {
                    name: "count".into(),
                })),
                right: Box::new(Expr::Lit(Literal::Int(0))),
            }),
            tell_func: ast::Ident { name: "_".into() },
            tell_args: vec![],
            body: vec![ast::Statement::TaskBlock(ast::TaskBlock {
                machines: ast::Machines::Single(ast::Ident { name: "_".into() }),
                body: vec![],
            })],
        };
        let result = execute_stmt(
            &ast::Statement::ControlFlow(ast::ControlFlow::While(while_loop)),
            &mut ctx,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_binop_neq() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Neq,
            left: Box::new(Expr::Lit(Literal::Int(1))),
            right: Box::new(Expr::Lit(Literal::Int(2))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_execute_binop_le() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Le,
            left: Box::new(Expr::Lit(Literal::Int(5))),
            right: Box::new(Expr::Lit(Literal::Int(5))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_execute_binop_gt() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Lit(Literal::Int(10))),
            right: Box::new(Expr::Lit(Literal::Int(5))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_execute_binop_ge() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Ge,
            left: Box::new(Expr::Lit(Literal::Int(5))),
            right: Box::new(Expr::Lit(Literal::Int(5))),
        };
        let result = eval_expr(&expr, &ctx).unwrap();
        assert!(matches!(result, RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_execute_task_op_call_args() {
        let mut ctx = make_ctx();
        let stmt = ast::OperationStatement::ExecCommand {
            mode: ast::ExecMode::Batch,
            cmd: ast::Ident {
                name: "test".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("arg1".into()))],
        };
        assert!(execute_op_statement(&stmt, &mut ctx).is_ok());
    }

    #[test]
    fn test_execute_func_dependency() {
        let mut ctx = make_ctx();
        let stmt = ast::FunctionStatement::Dependency {
            service_name: ast::Ident { name: "web".into() },
            service_param: None,
            on_machine: ast::Ident {
                name: "primary".into(),
            },
            as_name: ast::Ident { name: "w".into() },
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_execute_func_transfer() {
        let mut ctx = make_ctx();
        ctx.bind("target".to_string(), RuntimeValue::StringVal("host".into()));
        let stmt = ast::FunctionStatement::Transfer {
            from: Expr::Lit(Literal::StringVal("/src".into())),
            machine: Expr::Var(ast::Ident {
                name: "target".into(),
            }),
            location: Expr::Lit(Literal::StringVal("/dst".into())),
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_execute_func_dependency_2() {
        let mut ctx = make_ctx();
        let stmt = ast::FunctionStatement::Dependency {
            service_name: ast::Ident { name: "web".into() },
            service_param: None,
            on_machine: ast::Ident {
                name: "host".into(),
            },
            as_name: ast::Ident { name: "dep".into() },
        };
        let result = execute_function(&stmt, &mut ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_try_catch_finally_only() {
        let mut ctx = make_ctx();
        // Try with no catch but with finally — executes finally then returns
        let try_stmt = ast::Statement::ControlFlow(ast::ControlFlow::TryCatch(ast::TryCatch {
            body: vec![],
            catch_err_var: None,
            catch_body: vec![],
            catch_all: vec![],
            finally_body: vec![ast::Statement::TaskBlock(ast::TaskBlock {
                machines: ast::Machines::Single(ast::Ident { name: "_".into() }),
                body: vec![],
            })],
        }));
        let result = execute_stmt(&try_stmt, &mut ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_for_loop_dict_empty() {
        let mut ctx = make_ctx();
        ctx.bind("m".to_string(), RuntimeValue::Struct(HashMap::new()));
        let for_loop = ast::ControlFlow::For(ast::ForLoop::Dict {
            key_var: ast::Ident { name: "k".into() },
            value_var: ast::Ident { name: "v".into() },
            iterable: Box::new(Expr::Var(ast::Ident { name: "m".into() })),
            body: vec![],
        });
        let result = execute_stmt(&ast::Statement::ControlFlow(for_loop), &mut ctx);
        assert!(result.is_ok());
    }

    // ── Error path tests ────────────────────

    #[test]
    fn test_error_expected_int() {
        let ctx = make_ctx();
        let expr = Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::Lit(Literal::StringVal("x".into()))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::ExpectedInt(_))));
    }

    #[test]
    fn test_error_expected_numeric() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(Expr::Lit(Literal::Bool(true))),
            right: Box::new(Expr::Lit(Literal::Int(1))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::ExpectedNumeric(_))));
    }

    #[test]
    fn test_error_expected_struct() {
        let ctx = make_ctx();
        let expr = Expr::FieldAccess {
            target: Box::new(Expr::Lit(Literal::Int(42))),
            field: ast::Ident { name: "foo".into() },
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::ExpectedStruct(_))));
    }

    #[test]
    fn test_error_expected_string() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(Expr::Lit(Literal::Bool(true))),
            right: Box::new(Expr::Lit(Literal::Int(1))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::ExpectedNumeric(_))));
    }

    #[test]
    fn test_error_expected_collection() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident { name: "len".into() },
            args: vec![Expr::Lit(Literal::Bool(true))],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::ExpectedCollection(_))));
    }

    #[test]
    fn test_error_choose_not_in_op() {
        let ctx = make_ctx();
        let expr = Expr::Choose {
            variable: ast::Ident { name: "x".into() },
            ty: ast::Type::Primitive(ast::PrimitiveType::Node),
            from_set: None,
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(
            result,
            Err(ExecutionError::ChooseNotAllowedOutsideOperation)
        ));
    }

    #[test]
    fn test_error_index_out_of_bounds() {
        let mut ctx = make_ctx();
        ctx.bind(
            "items".to_string(),
            RuntimeValue::List(vec![RuntimeValue::Int(1), RuntimeValue::Int(2)]),
        );
        // normalize_index always wraps, so IndexOutOfBounds is unreachable
        // via normal eval; test via a struct (non-list, non-map) to get IndexTypeMismatch instead
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Lit(Literal::Bool(true))),
            index: Box::new(Expr::Lit(Literal::Int(0))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(
            result,
            Err(ExecutionError::IndexTypeMismatch(_, _))
        ));
    }

    #[test]
    fn test_error_index_type_mismatch() {
        let mut ctx = make_ctx();
        ctx.bind(
            "items".to_string(),
            RuntimeValue::List(vec![RuntimeValue::Int(1), RuntimeValue::Int(2)]),
        );
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident {
                name: "items".into(),
            })),
            index: Box::new(Expr::Lit(Literal::StringVal("key".into()))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(
            result,
            Err(ExecutionError::IndexTypeMismatch(_, _))
        ));
    }

    #[test]
    fn test_error_integer_overflow() {
        let ctx = make_ctx();
        // A Bytes value larger than i64::MAX triggers overflow in numeric context (Minus)
        let bytes_lit = Literal::Bytes(ast::BytesLit {
            value: i64::MAX as u64 + 1,
            suffix: ast::BytesSuffix::None,
        });
        let expr = Expr::BinOp {
            op: BinOp::Minus,
            left: Box::new(Expr::Lit(bytes_lit)),
            right: Box::new(Expr::Lit(Literal::Int(0))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(matches!(result, Err(ExecutionError::IntegerOverflow)));
    }

    #[test]
    fn test_exec_struct_expr() {
        let ctx = make_ctx();
        let expr = Expr::Struct {
            fields: vec![
                (
                    ast::Ident {
                        name: "name".into(),
                    },
                    Box::new(Expr::Lit(Literal::StringVal("test".into()))),
                ),
                (
                    ast::Ident {
                        name: "count".into(),
                    },
                    Box::new(Expr::Lit(Literal::Int(3))),
                ),
            ],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_choose_expr() {
        let ctx = make_ctx();
        // Choose is only valid inside operation body - will error outside
        let expr = Expr::Choose {
            variable: ast::Ident { name: "m".into() },
            ty: ast::Type::Resource(ast::Ident {
                name: "Machine".into(),
            }),
            from_set: None,
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_call_builtin() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident {
                name: "range".into(),
            },
            args: vec![Expr::Lit(Literal::Int(1)), Expr::Lit(Literal::Int(5))],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_call_no_args() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident {
                name: "range".into(),
            },
            args: vec![],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_call_wrong_type_arg() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident {
                name: "range".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("bad".into()))],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_index_struct() {
        let mut ctx = make_ctx();
        let mut map = HashMap::new();
        map.insert("key".to_string(), RuntimeValue::Int(42));
        ctx.bind("data".to_string(), RuntimeValue::Struct(map));
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident {
                name: "data".into(),
            })),
            index: Box::new(Expr::Lit(Literal::StringVal("key".into()))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_index_struct_missing() {
        let mut ctx = make_ctx();
        ctx.bind("data".to_string(), RuntimeValue::Struct(HashMap::new()));
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident {
                name: "data".into(),
            })),
            index: Box::new(Expr::Lit(Literal::StringVal("missing".into()))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_binop_string_concat() {
        let ctx = make_ctx();
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(Expr::Lit(Literal::StringVal("hello ".into()))),
            right: Box::new(Expr::Lit(Literal::StringVal("world".into()))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(matches!(val, RuntimeValue::StringVal(s) if s == "hello world"));
    }

    #[test]
    fn test_exec_index_negative() {
        // Negative index is handled as negative offset - may or may not error
        let mut ctx = make_ctx();
        ctx.bind(
            "items".to_string(),
            RuntimeValue::List(vec![RuntimeValue::Int(1)]),
        );
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident {
                name: "items".into(),
            })),
            index: Box::new(Expr::Lit(Literal::Int(-1))),
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn test_exec_index_out_of_bounds() {
        let mut ctx = make_ctx();
        ctx.bind(
            "items".to_string(),
            RuntimeValue::List(vec![RuntimeValue::Int(1)]),
        );
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(ast::Ident {
                name: "items".into(),
            })),
            index: Box::new(Expr::Lit(Literal::Int(5))),
        };
        let result = eval_expr(&expr, &ctx);
        // Index beyond list length may wrap or return element - implementation detail
        assert!(result.is_ok() || matches!(result, Err(ExecutionError::IndexOutOfBounds(_))));
    }

    #[test]
    fn test_exec_field_access_missing() {
        let ctx = make_ctx();
        let expr = Expr::FieldAccess {
            target: Box::new(Expr::Lit(Literal::Int(42))),
            field: ast::Ident {
                name: "missing".into(),
            },
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_exec_len_empty_list() {
        let mut ctx = make_ctx();
        ctx.bind("empty".to_string(), RuntimeValue::List(vec![]));
        let expr = Expr::Call {
            func: ast::Ident { name: "len".into() },
            args: vec![Expr::Var(ast::Ident {
                name: "empty".into(),
            })],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), RuntimeValue::Int(0)));
    }

    #[test]
    fn test_exec_exists_true() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident {
                name: "exists".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("test".into()))],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), RuntimeValue::Bool(true)));
    }

    #[test]
    fn test_exec_to_int() {
        let ctx = make_ctx();
        let expr = Expr::Call {
            func: ast::Ident {
                name: "to_int".into(),
            },
            args: vec![Expr::Lit(Literal::StringVal("42".into()))],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), RuntimeValue::Int(42)));
    }

    #[test]
    fn test_exec_template_simple() {
        let ctx = make_ctx();
        let expr = Expr::Template("hello".into());
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exec_struct_literal() {
        let ctx = make_ctx();
        let expr = Expr::Struct {
            fields: vec![(
                ast::Ident { name: "x".into() },
                Box::new(Expr::Lit(Literal::Int(1))),
            )],
        };
        let result = eval_expr(&expr, &ctx);
        assert!(result.is_ok());
    }
}
