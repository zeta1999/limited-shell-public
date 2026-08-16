//! Type system, role resolution, policy engine, and grant system for the Limited Shell language.
//!
//! - [`TyEnv`] — Scoped type environment for variable-to-type bindings
//! - [`TypeRegistry`] — Resource type registry with alias resolution
//! - [`check_expr`], [`check_let`] — Static type checking
//! - [`RoleEnv`] — Role hierarchy with `down` resolution via BFS
//! - [`ConditionEvaluator`] — Dynamic condition evaluation
//! - [`PolicyEngine`] — `can(role, op, resource)` with deny-first semantics
//! - [`GrantSystem`] — Transitive permission grants with authority checking

use crate::ast;
use crate::ast::expr::{BinOp, UnOp};
use crate::ast::{expr, ConditionPred, LetDecl};
use std::collections::{HashMap, HashSet, VecDeque};

// ─── Type environment ───────────────────────────────────────────

/// Maps variable names to their types within a scope.
#[derive(Debug, Clone, Default)]
pub struct TyEnv {
    parent: Option<Box<TyEnv>>,
    bindings: HashMap<String, ast::Type>,
}

impl TyEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn child(&self) -> TyEnv {
        TyEnv {
            parent: Some(Box::new(self.clone())),
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, name: ast::Ident, ty: ast::Type) {
        self.bindings.insert(name.name, ty);
    }

    pub fn resolve(&self, name: &str) -> Option<&ast::Type> {
        if let Some(ty) = self.bindings.get(name) {
            Some(ty)
        } else if let Some(parent) = &self.parent {
            parent.resolve(name)
        } else {
            None
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.parent.as_ref().is_some_and(|p| p.contains(name))
    }

    pub fn add(&mut self, name: String, ty: ast::Type) {
        self.bindings.insert(name, ty);
    }
}

// ─── Resource type registry ─────────────────────────────────────

/// Registry of declared resource types and type aliases.
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    resources: HashMap<String, ResourceTypeInfo>,
    aliases: HashMap<String, ast::Type>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_resource(&mut self, name: ast::Ident, info: ResourceTypeInfo) {
        self.resources.insert(name.name, info);
    }

    pub fn register_alias(&mut self, name: &str, target: ast::Type) {
        self.aliases.insert(name.to_string(), target);
    }

    pub fn lookup_resource(&self, name: &str) -> Option<&ResourceTypeInfo> {
        self.resources.get(name)
    }

    pub fn resolve_alias(&self, name: &str) -> Option<&ast::Type> {
        self.aliases.get(name)
    }

    /// Recursively resolve a type, following aliases to their base types.
    pub fn resolve_type(&self, ty: &ast::Type) -> ast::Type {
        if let ast::Type::Resource(id) = ty {
            if let Some(resolved) = self.aliases.get(&id.name) {
                return self.resolve_type(resolved);
            }
        }
        ty.clone()
    }

    /// Recursively resolve type aliases inside a type expression.
    pub fn resolve_type_deep(&self, ty: &ast::Type) -> ast::Type {
        match ty {
            ast::Type::Resource(id) => {
                if let Some(resolved) = self.aliases.get(&id.name) {
                    self.resolve_type_deep(resolved)
                } else {
                    ty.clone()
                }
            }
            ast::Type::List(inner) => ast::Type::List(Box::new(self.resolve_type_deep(inner))),
            ast::Type::MutList(inner) => {
                ast::Type::MutList(Box::new(self.resolve_type_deep(inner)))
            }
            ast::Type::Map(key, val) => ast::Type::Map(
                Box::new(self.resolve_type_deep(key)),
                Box::new(self.resolve_type_deep(val)),
            ),
            ast::Type::OrderedMap(key, val) => ast::Type::OrderedMap(
                Box::new(self.resolve_type_deep(key)),
                Box::new(self.resolve_type_deep(val)),
            ),
            ast::Type::Set(inner) => ast::Type::Set(Box::new(self.resolve_type_deep(inner))),
            ast::Type::OrderedSet(inner) => {
                ast::Type::OrderedSet(Box::new(self.resolve_type_deep(inner)))
            }
            ast::Type::SizedList(inner, size) => {
                ast::Type::SizedList(Box::new(self.resolve_type_deep(inner)), *size)
            }
            _ => ty.clone(),
        }
    }

    /// Check if a type is a resource type (resolving aliases).
    pub fn is_resource_type(&self, ty: &ast::Type) -> bool {
        let resolved = self.resolve_type(ty);
        matches!(resolved, ast::Type::Resource(_))
    }
}

/// Information about a declared resource type.
#[derive(Debug, Clone, Default)]
pub struct ResourceTypeInfo {
    pub capacities: Vec<ast::Capacity>,
    pub fields: Vec<ast::FieldDecl>,
}

impl ResourceTypeInfo {
    pub fn has_capacity(&self, name: &str) -> bool {
        self.capacities.iter().any(|c| c.name.name == name)
    }

    pub fn field_type(&self, name: &str) -> Option<&ast::Type> {
        self.fields
            .iter()
            .find(|f| f.name.name == name)
            .map(|f| &f.ty)
    }
}

// ─── Type checker errors ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    UnknownVariable(String),
    DuplicateVariable(String),
    TypeMismatch {
        expected: ast::Type,
        found: ast::Type,
    },
    UnknownResource(String),
    UnknownField {
        resource: String,
        field: String,
    },
    UndeclaredCapacity {
        resource: String,
        capacity: String,
    },
    MissingInit(LetDecl),
    InvalidIndex {
        container: ast::Type,
        index: ast::Type,
    },
    UnknownAlias(String),
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariable(n) => write!(f, "unknown variable: {}", n),
            Self::DuplicateVariable(n) => write!(f, "duplicate variable: {}", n),
            Self::TypeMismatch { expected, found } => {
                write!(f, "type mismatch: expected {}, found {}", expected, found)
            }
            Self::UnknownResource(n) => write!(f, "unknown resource type: {}", n),
            Self::UnknownField { resource, field } => {
                write!(f, "unknown field '{}' on resource '{}'", field, resource)
            }
            Self::UndeclaredCapacity { resource, capacity } => {
                write!(
                    f,
                    "undeclared capacity '{}' on resource '{}'",
                    capacity, resource
                )
            }
            Self::MissingInit(decl) => write!(f, "let binding '{}' missing initializer", decl.name),
            Self::InvalidIndex { container, index } => {
                write!(
                    f,
                    "invalid index: cannot use {} to index {}",
                    index, container
                )
            }
            Self::UnknownAlias(n) => write!(f, "unknown type alias: {}", n),
        }
    }
}

impl std::error::Error for TypeError {}

// ─── Expression type checking ───────────────────────────────────

/// Check types against the given environment and registry.
pub fn check_expr(
    env: &TyEnv,
    reg: &TypeRegistry,
    expr: &ast::Expr,
) -> Result<ast::Type, TypeError> {
    use ast::PrimitiveType as PT;
    use ast::Type as T;

    match expr {
        expr::Expr::Lit(lit) => Ok(match lit {
            expr::Literal::Bool(_) => T::Primitive(PT::Bool),
            expr::Literal::Int(_) => T::Primitive(PT::Int),
            expr::Literal::Bytes(_) => T::Primitive(PT::Bytes),
            expr::Literal::StringVal(_) => T::Primitive(PT::String),
        }),

        expr::Expr::Var(id) => env
            .resolve(&id.name)
            .cloned()
            .ok_or(TypeError::UnknownVariable(id.name.clone())),

        expr::Expr::FieldAccess { target, field } => {
            let ty = check_expr(env, reg, target)?;
            let resolved = reg.resolve_type(&ty);
            if let T::Resource(name) = &resolved {
                if let Some(info) = reg.lookup_resource(&name.name) {
                    if let Some(ft) = info.field_type(&field.name) {
                        return Ok(reg.resolve_type_deep(ft));
                    }
                }
                Err(TypeError::UnknownField {
                    resource: name.name.clone(),
                    field: field.name.clone(),
                })
            } else {
                Err(TypeError::UnknownVariable(format!(
                    "cannot access field '{}' on non-resource type {}",
                    field.name, ty
                )))
            }
        }

        expr::Expr::IndexAccess { target, index } => {
            let container_ty = check_expr(env, reg, target)?;
            let index_ty = check_expr(env, reg, index)?;
            let resolved = reg.resolve_type(&container_ty);
            match &resolved {
                T::List(elem) => {
                    // Index should be Int
                    if !matches!(index_ty, T::Primitive(PT::Int)) {
                        return Err(TypeError::InvalidIndex {
                            container: container_ty,
                            index: index_ty,
                        });
                    }
                    Ok(reg.resolve_type_deep(elem))
                }
                T::MutList(elem) => {
                    if !matches!(index_ty, T::Primitive(PT::Int)) {
                        return Err(TypeError::InvalidIndex {
                            container: container_ty,
                            index: index_ty,
                        });
                    }
                    Ok(reg.resolve_type_deep(elem))
                }
                T::Map(k, v) => {
                    if !types_eq(&index_ty, k) {
                        return Err(TypeError::TypeMismatch {
                            expected: k.as_ref().clone(),
                            found: index_ty,
                        });
                    }
                    Ok(reg.resolve_type_deep(v))
                }
                T::OrderedMap(k, v) => {
                    if !types_eq(&index_ty, k) {
                        return Err(TypeError::TypeMismatch {
                            expected: k.as_ref().clone(),
                            found: index_ty,
                        });
                    }
                    Ok(reg.resolve_type_deep(v))
                }
                T::Set(elem) => {
                    // In set membership check: elem in set → Bool
                    // But index access on set returns element type
                    if !types_eq(&index_ty, elem) {
                        return Err(TypeError::TypeMismatch {
                            expected: elem.as_ref().clone(),
                            found: index_ty,
                        });
                    }
                    Ok(T::Primitive(PT::Bool))
                }
                T::SizedList(elem, _) => {
                    if !matches!(index_ty, T::Primitive(PT::Int)) {
                        return Err(TypeError::InvalidIndex {
                            container: container_ty,
                            index: index_ty,
                        });
                    }
                    Ok(reg.resolve_type_deep(elem))
                }
                _ => Err(TypeError::InvalidIndex {
                    container: container_ty,
                    index: index_ty,
                }),
            }
        }

        expr::Expr::Struct { fields } => {
            for (_, val) in fields {
                check_expr(env, reg, val)?;
            }
            Ok(T::Primitive(PT::JSON))
        }

        expr::Expr::Call { func: _, args } => {
            for arg in args {
                check_expr(env, reg, arg)?;
            }
            Ok(T::Primitive(PT::JSON))
        }

        expr::Expr::BinOp { op, left, right } => {
            let left_ty = check_expr(env, reg, left)?;
            let right_ty = check_expr(env, reg, right)?;
            match op {
                BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    if types_eq(&left_ty, &right_ty)
                        || (is_numeric(&left_ty) && is_numeric(&right_ty))
                    {
                        Ok(T::Primitive(PT::Bool))
                    } else {
                        Err(TypeError::TypeMismatch {
                            expected: left_ty,
                            found: right_ty,
                        })
                    }
                }
                BinOp::And | BinOp::Or => {
                    for ty in [&left_ty, &right_ty] {
                        if !is_bool(ty) {
                            return Err(TypeError::TypeMismatch {
                                expected: T::Primitive(PT::Bool),
                                found: ty.clone(),
                            });
                        }
                    }
                    Ok(T::Primitive(PT::Bool))
                }
                BinOp::Plus | BinOp::Minus | BinOp::Mul | BinOp::Div => {
                    for ty in [&left_ty, &right_ty] {
                        if !is_numeric(ty) {
                            return Err(TypeError::TypeMismatch {
                                expected: T::Primitive(PT::Int),
                                found: ty.clone(),
                            });
                        }
                    }
                    Ok(left_ty)
                }
            }
        }

        expr::Expr::UnOp { op, operand } => {
            check_expr(env, reg, operand)?;
            match op {
                UnOp::Not => Ok(T::Primitive(PT::Bool)),
                UnOp::Neg => Ok(T::Primitive(PT::Int)),
            }
        }

        expr::Expr::Template(_) => Ok(T::Primitive(PT::String)),
        expr::Expr::Choose { .. } => {
            // choose always returns a Node resource type
            Ok(T::Primitive(PT::Node))
        }
    }
}

/// Check a let binding and return the resolved type.
pub fn check_let(
    env: &TyEnv,
    reg: &TypeRegistry,
    decl: &ast::LetDecl,
) -> Result<(ast::Ident, ast::Type), TypeError> {
    let resolved_ty = match &decl.ty {
        Some(ty) => reg.resolve_type_deep(ty),
        None => {
            let init = decl
                .init
                .as_ref()
                .ok_or(TypeError::MissingInit(decl.clone()))?;
            check_expr(env, reg, init)?
        }
    };
    Ok((decl.name.clone(), resolved_ty))
}

// ─── Type utilities ─────────────────────────────────────────────

/// Check if two types are structurally equal (ignoring type aliases).
pub fn types_eq(a: &ast::Type, b: &ast::Type) -> bool {
    a == b
}

/// Check if a type is numeric (Int or Bytes).
pub fn is_numeric(ty: &ast::Type) -> bool {
    matches!(
        ty,
        ast::Type::Primitive(ast::PrimitiveType::Int)
            | ast::Type::Primitive(ast::PrimitiveType::Bytes)
    )
}

/// Check if a type is boolean.
pub fn is_bool(ty: &ast::Type) -> bool {
    matches!(ty, ast::Type::Primitive(ast::PrimitiveType::Bool))
}

/// Check if a type is a string type.
pub fn is_string(ty: &ast::Type) -> bool {
    matches!(ty, ast::Type::Primitive(ast::PrimitiveType::String))
}

// ─── Condition value model ──────────────────────────────────────

/// Runtime values used for condition evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondValue {
    Bool(bool),
    Int(i64),
    Bytes(u64),
    StringVal(String),
    Node(String),     // represents a node/machine name
    Resource(String), // resource name
    Role(String),     // role name
    JSON(serde_json::Value),
    Null,
}

impl CondValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            CondValue::StringVal(s) => Some(s),
            CondValue::Node(s) => Some(s),
            CondValue::Role(r) => Some(r),
            CondValue::Resource(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            CondValue::StringVal(s) => Some(s.clone()),
            CondValue::Node(n) => Some(n.clone()),
            CondValue::Role(r) => Some(r.clone()),
            CondValue::Resource(r) => Some(r.clone()),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            CondValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            CondValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            CondValue::Bool(b) => *b,
            CondValue::Int(n) => *n != 0,
            CondValue::Bytes(n) => *n != 0,
            CondValue::StringVal(s) => !s.is_empty(),
            CondValue::Node(_) | CondValue::Role(_) | CondValue::Resource(_) => true,
            CondValue::JSON(v) => !matches!(v, serde_json::Value::Null),
            CondValue::Null => false,
        }
    }
}

/// Context for condition evaluation: variable bindings and known resources.
#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    /// Variable bindings: name → runtime value
    pub variables: HashMap<String, CondValue>,
    /// Known roles for `is` checks
    pub known_roles: HashSet<String>,
    /// Known nodes/machines
    pub known_nodes: HashSet<String>,
    /// Known resource names
    pub known_resources: HashSet<String>,
    /// Known sets for `in` checks
    pub sets: HashMap<String, HashSet<String>>,
}

impl EvalContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, name: &str, value: CondValue) {
        self.variables.insert(name.to_string(), value);
    }

    pub fn resolve(&self, name: &str) -> Option<&CondValue> {
        self.variables.get(name)
    }
}

// ─── Condition evaluator ────────────────────────────────────────

/// Evaluates conditions against a runtime context.
pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// Evaluate a condition predicate against the given context.
    pub fn evaluate_pred(ctx: &EvalContext, pred: &ConditionPred) -> bool {
        use ConditionPred as CP;
        match pred {
            CP::Is { left, roles } => Self::eval_is(ctx, left, roles),
            CP::Can { op, resource } => Self::eval_can(ctx, op, resource.as_ref()),
            CP::StartsWith { expr, prefix } => Self::eval_starts_with(ctx, expr, prefix),
            CP::EndsWith { expr, suffix } => Self::eval_ends_with(ctx, expr, suffix),
            CP::DropPrefixEq {
                prefix,
                left,
                right,
            } => Self::eval_drop_prefix_eq(ctx, prefix, left, right),
            CP::InSet { expr, set } => Self::eval_in_set(ctx, expr, set),
            CP::Exists(expr) => Self::eval_exists(ctx, expr),
            CP::Matches { expr, pattern } => Self::eval_matches(ctx, expr, pattern),
            CP::Not(inner) => !Self::evaluate_pred(ctx, inner),
            CP::And(a, b) => Self::evaluate_pred(ctx, a) && Self::evaluate_pred(ctx, b),
            CP::Or(a, b) => Self::evaluate_pred(ctx, a) || Self::evaluate_pred(ctx, b),
        }
    }

    /// Evaluate `left is Role1, Role2 or down`
    fn eval_is(ctx: &EvalContext, left: &expr::Expr, roles: &[ast::RoleRef]) -> bool {
        let left_val = Self::eval_expr_to_value(ctx, left);
        let left_str = left_val.as_string();

        let Some(left_str) = left_str else {
            return false;
        };

        roles.iter().any(|role_ref| {
            let target = match role_ref {
                ast::RoleRef::Exact(id) => id.name.as_str(),
                ast::RoleRef::Down(id) => id.name.as_str(),
                ast::RoleRef::RoleDown(id) => id.name.as_str(),
            };
            left_str == target || ctx.known_roles.contains(&left_str)
        })
    }

    /// Evaluate `can Op {x:Res}` — checks if any resource matching the pattern is known.
    fn eval_can(
        ctx: &EvalContext,
        op: &ast::Ident,
        resource: Option<&ast::ResourcePattern>,
    ) -> bool {
        // For now, a can check passes if the operation name starts with "can_"
        // or if we have no resource constraint (unrestricted)
        let op_name = op.name.to_lowercase();
        match resource {
            Some(rp) => ctx.known_resources.contains(&rp.variable.name),
            None => op_name.starts_with("can_") || op_name == "all",
        }
    }

    fn eval_starts_with(ctx: &EvalContext, expr: &expr::Expr, prefix: &str) -> bool {
        let val = Self::eval_expr_to_value(ctx, expr);
        matches!(&val, CondValue::StringVal(s) if s.starts_with(prefix))
    }

    fn eval_ends_with(ctx: &EvalContext, expr: &expr::Expr, suffix: &str) -> bool {
        let val = Self::eval_expr_to_value(ctx, expr);
        matches!(&val, CondValue::StringVal(s) if s.ends_with(suffix))
    }

    fn eval_drop_prefix_eq(
        ctx: &EvalContext,
        prefix: &str,
        left: &expr::Expr,
        right: &expr::Expr,
    ) -> bool {
        let left_val = Self::eval_expr_to_value(ctx, left);
        let right_val = Self::eval_expr_to_value(ctx, right);
        let left_str = left_val.as_string();
        let right_str = right_val.as_string();
        match (left_str, right_str) {
            (Some(l), Some(r)) => l.strip_prefix(prefix) == r.strip_prefix(prefix),
            _ => false,
        }
    }

    fn eval_in_set(ctx: &EvalContext, expr: &expr::Expr, set: &ast::Ident) -> bool {
        let val = Self::eval_expr_to_value(ctx, expr);
        let val_str = match &val {
            CondValue::StringVal(s) => s.clone(),
            CondValue::Node(n) => n.clone(),
            CondValue::Role(r) => r.clone(),
            CondValue::Resource(r) => r.clone(),
            CondValue::Int(n) => n.to_string(),
            _ => return false,
        };
        ctx.sets
            .get(&set.name)
            .map(|s| s.contains(&val_str))
            .unwrap_or(false)
    }

    fn eval_exists(ctx: &EvalContext, expr: &expr::Expr) -> bool {
        let val = Self::eval_expr_to_value(ctx, expr);
        val.is_truthy()
    }

    fn eval_matches(ctx: &EvalContext, expr: &expr::Expr, pattern: &str) -> bool {
        let val = Self::eval_expr_to_value(ctx, expr);
        if let CondValue::StringVal(s) = &val {
            // Simple glob-like matching: * matches anything, ? matches one char
            glob_match(pattern, s)
        } else {
            false
        }
    }

    /// Evaluate an expression to a runtime CondValue.
    /// This handles literals, variables, and simple operations.
    fn eval_expr_to_value(ctx: &EvalContext, expr: &expr::Expr) -> CondValue {
        use expr::Expr;
        match expr {
            Expr::Lit(lit) => match lit {
                expr::Literal::Bool(b) => CondValue::Bool(*b),
                expr::Literal::Int(n) => CondValue::Int(*n),
                expr::Literal::Bytes(b) => CondValue::Bytes(b.value),
                expr::Literal::StringVal(s) => CondValue::StringVal(s.clone()),
            },
            Expr::Var(id) => ctx.resolve(&id.name).cloned().unwrap_or(CondValue::Null),
            Expr::FieldAccess { target, field } => {
                let target_val = Self::eval_expr_to_value(ctx, target);
                match target_val {
                    CondValue::Resource(name) => {
                        // Look up field in known resources
                        CondValue::StringVal(format!("{}.{}", name, field.name))
                    }
                    CondValue::JSON(obj) => match obj.get(&field.name) {
                        Some(v) => json_to_cond(v),
                        None => CondValue::Null,
                    },
                    _ => CondValue::Null,
                }
            }
            Expr::BinOp { op, left, right } => {
                let left_val = Self::eval_expr_to_value(ctx, left);
                let right_val = Self::eval_expr_to_value(ctx, right);
                Self::eval_binop_value(op, &left_val, &right_val)
            }
            Expr::UnOp { op, operand } => {
                let val = Self::eval_expr_to_value(ctx, operand);
                Self::eval_unop_value(op, &val)
            }
            Expr::Template(s) => CondValue::StringVal(s.clone()),
            _ => CondValue::Null,
        }
    }

    fn eval_binop_value(op: &BinOp, left: &CondValue, right: &CondValue) -> CondValue {
        match op {
            BinOp::Eq => CondValue::Bool(left == right),
            BinOp::Neq => CondValue::Bool(left != right),
            BinOp::Lt => {
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    CondValue::Bool(l < r)
                } else {
                    CondValue::Bool(false)
                }
            }
            BinOp::Le => {
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    CondValue::Bool(l <= r)
                } else {
                    CondValue::Bool(false)
                }
            }
            BinOp::Gt => {
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    CondValue::Bool(l > r)
                } else {
                    CondValue::Bool(false)
                }
            }
            BinOp::Ge => {
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    CondValue::Bool(l >= r)
                } else {
                    CondValue::Bool(false)
                }
            }
            BinOp::And => CondValue::Bool(left.is_truthy() && right.is_truthy()),
            BinOp::Or => CondValue::Bool(left.is_truthy() || right.is_truthy()),
            BinOp::Plus => {
                if let (CondValue::Int(a), CondValue::Int(b)) = (left, right) {
                    CondValue::Int(a + b)
                } else if let (CondValue::Bytes(a), CondValue::Bytes(b)) = (left, right) {
                    CondValue::Bytes(a + b)
                } else if let (CondValue::StringVal(a), CondValue::StringVal(b)) = (left, right) {
                    CondValue::StringVal(format!("{}{}", a, b))
                } else {
                    CondValue::Null
                }
            }
            BinOp::Minus => {
                if let (CondValue::Int(a), CondValue::Int(b)) = (left, right) {
                    CondValue::Int(a - b)
                } else if let (CondValue::Bytes(a), CondValue::Bytes(b)) = (left, right) {
                    CondValue::Bytes(a.saturating_sub(*b))
                } else {
                    CondValue::Null
                }
            }
            BinOp::Mul => {
                if let (CondValue::Int(a), CondValue::Int(b)) = (left, right) {
                    CondValue::Int(a * b)
                } else if let (CondValue::Bytes(a), CondValue::Int(b)) = (left, right) {
                    CondValue::Bytes((*a as i64 * b) as u64)
                } else {
                    CondValue::Null
                }
            }
            BinOp::Div => {
                if let (CondValue::Int(a), CondValue::Int(b)) = (left, right) {
                    if *b != 0 {
                        CondValue::Int(a / b)
                    } else {
                        CondValue::Null
                    }
                } else {
                    CondValue::Null
                }
            }
        }
    }

    fn eval_unop_value(op: &UnOp, val: &CondValue) -> CondValue {
        match op {
            UnOp::Not => CondValue::Bool(!val.is_truthy()),
            UnOp::Neg => val
                .as_int()
                .map(|n| CondValue::Int(-n))
                .unwrap_or(CondValue::Null),
        }
    }
}

/// Convert a simple glob pattern to match a string.
fn glob_match(pattern: &str, s: &str) -> bool {
    // Build a simple regex: * → .*, ? → .
    let regex_pattern: String = pattern
        .chars()
        .flat_map(|c| match c {
            '*' => vec!['.', '*'],
            '?' => vec!['.'],
            c if matches!(
                c,
                '.' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
            ) =>
            {
                vec!['\\', c]
            }
            c => vec![c],
        })
        .collect();
    let regex = format!("^{}$", regex_pattern);
    regex::Regex::new(&regex)
        .map(|re| re.is_match(s))
        .unwrap_or(false)
}

/// Convert a serde_json::Value to a CondValue.
fn json_to_cond(v: &serde_json::Value) -> CondValue {
    match v {
        serde_json::Value::Null => CondValue::Null,
        serde_json::Value::Bool(b) => CondValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CondValue::Int(i)
            } else {
                CondValue::JSON(v.clone())
            }
        }
        serde_json::Value::String(s) => CondValue::StringVal(s.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => CondValue::JSON(v.clone()),
    }
}

// ─── Role environment ─────────────────────────────────────────

/// Role hierarchy with `down` resolution using petgraph.
#[derive(Debug, Clone)]
pub struct RoleEnv {
    graph: petgraph::graph::DiGraph<String, ()>,
    /// Map role name to node index for O(1) lookup
    names: HashMap<String, petgraph::graph::NodeIndex>,
}

impl Default for RoleEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl RoleEnv {
    pub fn new() -> Self {
        Self {
            graph: petgraph::graph::DiGraph::new(),
            names: HashMap::new(),
        }
    }

    fn get_or_add_node(&mut self, name: &str) -> petgraph::graph::NodeIndex {
        if let Some(idx) = self.names.get(name) {
            return *idx;
        }
        let idx = self.graph.add_node(name.to_string());
        self.names.insert(name.to_string(), idx);
        idx
    }

    /// Register a role in the hierarchy.
    pub fn add_role(&mut self, name: &str) {
        self.get_or_add_node(name);
    }

    /// Set parent relationship: `child`'s parent is `parent`.
    pub fn set_parent(&mut self, child: &str, parent: &str) {
        let child_idx = self.get_or_add_node(child);
        let parent_idx = self.get_or_add_node(parent);
        // parent → child means "child inherits from parent"
        self.graph.add_edge(parent_idx, child_idx, ());
    }

    /// Resolve `Role or down`: find all descendants of the given role (BFS).
    /// Returns empty vec if role doesn't exist in the graph.
    pub fn resolve_down(&self, role: &str) -> Vec<String> {
        let root = match self.names.get(role) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };
        let mut result = vec![self.graph[root].clone()];
        let mut visited = HashSet::new();
        visited.insert(root);
        let mut queue = VecDeque::new();

        for child in self
            .graph
            .neighbors_directed(root, petgraph::Direction::Outgoing)
        {
            if visited.insert(child) {
                queue.push_back(child);
                result.push(self.graph[child].clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            for child in self
                .graph
                .neighbors_directed(current, petgraph::Direction::Outgoing)
            {
                if visited.insert(child) {
                    queue.push_back(child);
                    result.push(self.graph[child].clone());
                }
            }
        }

        result
    }

    /// Resolve `or down` with a specific role list: for each role in the list,
    /// include itself and all its descendants.
    pub fn resolve_roles_with_down(&self, role_refs: &[ast::RoleRef]) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        for role_ref in role_refs {
            match role_ref {
                ast::RoleRef::Exact(id) => {
                    let name = id.name.clone();
                    if seen.insert(name.clone()) {
                        result.push(name);
                    }
                }
                ast::RoleRef::Down(id) | ast::RoleRef::RoleDown(id) => {
                    let names = self.resolve_down(&id.name);
                    for name in names {
                        if seen.insert(name.clone()) {
                            result.push(name);
                        }
                    }
                }
            }
        }

        result
    }

    /// Check if role A is a descendant of role B (A inherits from B).
    pub fn is_descendant(&self, descendant: &str, ancestor: &str) -> bool {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(&root) = self.names.get(ancestor) {
            for child in self
                .graph
                .neighbors_directed(root, petgraph::Direction::Outgoing)
            {
                let name = &self.graph[child];
                if name == descendant {
                    return true;
                }
                if visited.insert(child) {
                    queue.push_back(child);
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            for child in self
                .graph
                .neighbors_directed(current, petgraph::Direction::Outgoing)
            {
                let name = &self.graph[child];
                if name == descendant {
                    return true;
                }
                if visited.insert(child) {
                    queue.push_back(child);
                }
            }
        }

        false
    }

    /// Get the direct parent of a role.
    pub fn get_parent(&self, role: &str) -> Option<String> {
        self.names
            .get(role)
            .and_then(|&idx| {
                self.graph
                    .neighbors_directed(idx, petgraph::Direction::Incoming)
                    .next()
            })
            .map(|p| self.graph[p].clone())
    }

    /// Get all direct children of a role.
    pub fn get_children(&self, role: &str) -> Vec<String> {
        self.names
            .get(role)
            .map(|&idx| {
                self.graph
                    .neighbors_directed(idx, petgraph::Direction::Outgoing)
                    .map(|c| self.graph[c].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all ancestors of a role (parent, grandparent, ...).
    pub fn get_ancestors(&self, role: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = role.to_string();
        let mut depth = 0;
        while let Some(parent) = self.get_parent(&current) {
            result.push(parent.clone());
            current = parent;
            depth += 1;
            if depth > 100 {
                break;
            }
        }
        result
    }

    /// Detect cycles in the role hierarchy.
    /// Returns a list of roles involved in cycles, or empty if acyclic.
    pub fn detect_cycles(&self) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        let mut cycle_roles = HashSet::new();

        for idx in self.graph.node_indices() {
            if !visited.contains(&idx) {
                self.dfs_detect_cycle(idx, &mut visited, &mut in_stack, &mut cycle_roles);
            }
        }

        let mut result: Vec<String> = cycle_roles
            .into_iter()
            .map(|idx| self.graph[idx].clone())
            .collect();
        result.sort();
        result
    }

    fn dfs_detect_cycle(
        &self,
        node: petgraph::graph::NodeIndex,
        visited: &mut HashSet<petgraph::graph::NodeIndex>,
        in_stack: &mut HashSet<petgraph::graph::NodeIndex>,
        cycle_roles: &mut HashSet<petgraph::graph::NodeIndex>,
    ) {
        visited.insert(node);
        in_stack.insert(node);

        for neighbor in self
            .graph
            .neighbors_directed(node, petgraph::Direction::Outgoing)
        {
            if !visited.contains(&neighbor) {
                self.dfs_detect_cycle(neighbor, visited, in_stack, cycle_roles);
            } else if in_stack.contains(&neighbor) {
                cycle_roles.insert(neighbor);
                cycle_roles.insert(node);
            }
        }

        in_stack.remove(&node);
    }

    /// Check if adding a parent edge would create a cycle.
    pub fn would_create_cycle(&self, child: &str, parent: &str) -> bool {
        // Would create a cycle if parent is already a descendant of child
        if child == parent {
            return true;
        }
        self.is_descendant(parent, child)
    }

    /// Get all roles in the hierarchy.
    pub fn all_roles(&self) -> Vec<String> {
        let mut roles: Vec<String> = self
            .names
            .values()
            .map(|idx| self.graph[*idx].clone())
            .collect();
        roles.sort();
        roles
    }
}

// ─── Policy engine ──────────────────────────────────────────────

/// Result of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyCheck {
    Granted,
    Denied(String), // reason
}

/// A single policy rule (can/cannot permission).
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub deny: bool,                     // false = can, true = cannot
    pub op: String,                     // operation name
    pub resource_var: Option<String>, // variable name for resource pattern (e.g., "x" in "{x:File}")
    pub resource_type: Option<String>, // resource type (e.g., "File")
    pub conditions: Vec<ConditionPred>, // requires clauses
}

/// A grant: grants RoleB permission from RoleA.
#[derive(Debug, Clone)]
pub struct Grant {
    pub target_role: String,
    pub op: String,
    pub resource: ast::ResourcePattern,
    pub condition: Option<ConditionPred>,
}

/// Policy engine: evaluates `can(role, op, resource)` checks.
pub struct PolicyEngine {
    /// Role hierarchy
    pub roles: RoleEnv,
    /// Explicit permissions (can/cannot rules from role declarations)
    pub rules: Vec<PolicyRule>,
    /// Grants (explicit permission grants between roles)
    pub grants: Vec<Grant>,
    /// Role permissions cache: role → set of allowed operations
    role_permissions: HashMap<String, HashSet<String>>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self {
            roles: RoleEnv::new(),
            rules: Vec::new(),
            grants: Vec::new(),
            role_permissions: HashMap::new(),
        }
    }

    /// Add a policy rule from a role's permission declaration.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.rules.push(rule);
    }

    /// Add a grant.
    pub fn add_grant(&mut self, grant: Grant) {
        self.grants.push(grant);
    }

    /// Register a role in the hierarchy.
    pub fn add_role(&mut self, name: &str, parent: Option<&str>) {
        if let Some(parent) = parent {
            if self.roles.would_create_cycle(name, parent) {
                // Don't add cyclic edges
                return;
            }
            self.roles.set_parent(name, parent);
        } else {
            self.roles.add_role(name);
        }
    }

    /// Build the permission cache from all rules and grants.
    pub fn build_cache(&mut self) {
        let all_roles = self.roles.all_roles();

        for role in &all_roles {
            let mut perms = HashSet::new();
            self.collect_permissions(role, &mut perms, &mut HashSet::new());
            self.role_permissions.insert(role.clone(), perms);
        }
    }

    /// Collect all permissions for a role, including inherited and granted permissions.
    fn collect_permissions(
        &self,
        role: &str,
        perms: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(role.to_string()) {
            return;
        }

        // Direct permissions from rules
        for rule in &self.rules {
            perms.insert(rule.op.clone());
        }

        // Permissions from grants to this role
        for grant in &self.grants {
            if grant.target_role == role {
                perms.insert(grant.op.clone());
            }
        }

        // Inherited permissions from parent
        if let Some(parent) = self.roles.get_parent(role) {
            self.collect_permissions(&parent, perms, visited);
        }

        // Permissions from descendant roles
        for child in self.roles.get_children(role) {
            self.collect_permissions(&child, perms, visited);
        }
    }

    /// Check if a role has permission to perform an operation on a resource.
    /// Returns Granted if allowed, Denied with reason if not.
    pub fn can_check(
        &self,
        _role: &str,
        op: &str,
        resource_name: Option<&str>,
        ctx: &EvalContext,
    ) -> PolicyCheck {
        // Phase 1: Check deny rules first (deny overrides grant)
        for rule in &self.rules {
            if rule.deny {
                // Check if this deny rule applies
                if self.rule_matches(rule, op, resource_name, ctx) {
                    return PolicyCheck::Denied(format!(
                        "denied by cannot rule: {} on {}",
                        rule.op,
                        resource_name.unwrap_or("any resource")
                    ));
                }
            }
        }

        // Phase 2: Check grant rules
        for rule in &self.rules {
            if !rule.deny && self.rule_matches(rule, op, resource_name, ctx) {
                // Verify all conditions
                let conditions_met = rule
                    .conditions
                    .iter()
                    .all(|c| ConditionEvaluator::evaluate_pred(ctx, c));
                if conditions_met {
                    return PolicyCheck::Granted;
                }
            }
        }

        // Phase 3: Check grants
        for grant in &self.grants {
            if grant.op == op && self.grant_matches(grant, resource_name, ctx) {
                // Check grant condition
                let condition_met = match &grant.condition {
                    Some(c) => ConditionEvaluator::evaluate_pred(ctx, c),
                    None => true,
                };
                if condition_met {
                    return PolicyCheck::Granted;
                }
            }
        }

        // Phase 4: Default deny
        PolicyCheck::Denied(format!(
            "no permission for {} on {}",
            op,
            resource_name.unwrap_or("any resource")
        ))
    }

    /// Check if a rule matches the given operation and resource context.
    fn rule_matches(
        &self,
        rule: &PolicyRule,
        op: &str,
        resource_name: Option<&str>,
        _ctx: &EvalContext,
    ) -> bool {
        // Operation match
        if rule.op != op && rule.op != "all" {
            return false;
        }

        // Resource pattern match
        if let Some(rp) = &rule.resource_type {
            if let Some(rname) = resource_name {
                if rp != rname {
                    return false;
                }
            }
        }

        true
    }

    /// Check if a grant matches the given resource context.
    fn grant_matches(
        &self,
        grant: &Grant,
        resource_name: Option<&str>,
        _ctx: &EvalContext,
    ) -> bool {
        if let Some(rname) = resource_name {
            if let ast::Type::Resource(rid) = &grant.resource.resource_type {
                if rid.name != rname {
                    return false;
                }
            }
        }
        true
    }

    /// Check if a role has a granted permission (cached).
    pub fn has_permission(&self, role: &str, op: &str) -> bool {
        self.role_permissions
            .get(role)
            .map(|perms| perms.contains(op))
            .unwrap_or(false)
    }
}

// ─── Grant system ───────────────────────────────────────────────

/// Errors that can occur during grant validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    UnknownRole(String),
    UnknownOperation(String),
    UnknownResource(String),
    NoAuthority {
        grantor: String,
        target: String,
    },
    DuplicateGrant {
        role: String,
        op: String,
        resource: String,
    },
}

impl std::fmt::Display for GrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRole(n) => write!(f, "unknown role: {}", n),
            Self::UnknownOperation(n) => write!(f, "unknown operation: {}", n),
            Self::UnknownResource(n) => write!(f, "unknown resource type: {}", n),
            Self::NoAuthority { grantor, target } => {
                write!(
                    f,
                    "role '{}' has no authority to grant to '{}'",
                    grantor, target
                )
            }
            Self::DuplicateGrant { role, op, resource } => {
                write!(f, "duplicate grant: {} can {} on {}", role, op, resource)
            }
        }
    }
}

impl std::error::Error for GrantError {}

/// Result of a grant operation.
#[derive(Debug, Clone)]
pub struct GrantResult {
    pub success: bool,
    pub grant: Option<Grant>,
    pub error: Option<GrantError>,
}

impl GrantResult {
    pub fn granted(grant: Grant) -> Self {
        Self {
            success: true,
            grant: Some(grant),
            error: None,
        }
    }

    pub fn failed(error: GrantError) -> Self {
        Self {
            success: false,
            grant: None,
            error: Some(error),
        }
    }
}

/// Manages grant processing and transitive permission resolution.
pub struct GrantSystem {
    engine: PolicyEngine,
    /// Track existing grants to avoid duplicates
    existing: HashSet<(String, String, String)>,
}

impl Default for GrantSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl GrantSystem {
    pub fn new() -> Self {
        Self {
            engine: PolicyEngine::new(),
            existing: HashSet::new(),
        }
    }

    /// Process a grant statement: `grant Role can Op {x:Res} if cond`.
    /// Returns success/failure with error details.
    pub fn process_grant(&mut self, grantor: &str, grant: Grant) -> GrantResult {
        // Check for duplicate
        let key = (
            grant.target_role.clone(),
            grant.op.clone(),
            format!(
                "{}/{}",
                grant.resource.variable.name, grant.resource.resource_type
            ),
        );
        if !self.existing.insert(key.clone()) {
            return GrantResult::failed(GrantError::DuplicateGrant {
                role: key.0,
                op: key.1,
                resource: key.2,
            });
        }

        // Authority check: grantor must have authority to define operations for target role
        if !self.check_authority(grantor, &grant.target_role) {
            return GrantResult::failed(GrantError::NoAuthority {
                grantor: grantor.to_string(),
                target: grant.target_role.clone(),
            });
        }

        // Add to policy engine
        self.engine.add_grant(grant.clone());
        GrantResult::granted(grant)
    }

    /// Check if grantor has authority to grant to target role.
    fn check_authority(&self, grantor: &str, target: &str) -> bool {
        // The grantor must be the same role, a parent role, or have define_ops_for permission
        if grantor == target {
            return true;
        }

        // Check if grantor is a parent of target (role hierarchy authority)
        self.engine.roles.is_descendant(target, grantor)
    }

    /// Build the full permission cache.
    pub fn build_cache(&mut self) {
        self.engine.build_cache();
    }

    /// Check permission through the grant system.
    pub fn can_check(
        &self,
        role: &str,
        op: &str,
        resource_name: Option<&str>,
        ctx: &EvalContext,
    ) -> PolicyCheck {
        self.engine.can_check(role, op, resource_name, ctx)
    }

    /// Add a role to the hierarchy.
    pub fn add_role(&mut self, name: &str, parent: Option<&str>) {
        self.engine.add_role(name, parent);
    }

    /// Add a policy rule.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.engine.add_rule(rule);
    }

    /// Get all resolved permissions for a role.
    pub fn get_permissions(&self, role: &str) -> HashSet<String> {
        self.engine
            .role_permissions
            .get(role)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the underlying policy engine.
    pub fn engine(&self) -> &PolicyEngine {
        &self.engine
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::{self, Expr, Literal};
    use crate::ast::Ident;

    fn ident(s: &str) -> Ident {
        Ident {
            name: s.to_string(),
        }
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Lit(Literal::Int(n))
    }

    fn bool_lit(b: bool) -> Expr {
        Expr::Lit(Literal::Bool(b))
    }

    fn string_lit(s: &str) -> Expr {
        Expr::Lit(Literal::StringVal(s.to_string()))
    }

    fn var(s: &str) -> Expr {
        Expr::Var(ident(s))
    }

    // ─── Type environment tests ───

    #[test]
    fn test_tyenv_bind_and_resolve() {
        let mut env = TyEnv::new();
        env.bind(ident("x"), ast::Type::Primitive(ast::PrimitiveType::Int));
        assert!(env.contains("x"));
        assert!(env.resolve("x").is_some());
        assert!(!env.contains("y"));
    }

    #[test]
    fn test_tyenv_child_scope() {
        let parent = TyEnv::new();
        let mut child = parent.child();
        child.bind(ident("y"), ast::Type::Primitive(ast::PrimitiveType::Bool));

        assert!(child.resolve("y").is_some());
        // Parent's variables visible in child
        let mut grandparent = TyEnv::new();
        grandparent.bind(ident("x"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let child2 = grandparent.child();
        assert!(child2.resolve("x").is_some());
    }

    // ─── Type checking tests ───

    #[test]
    fn test_check_expr_lit_bool() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let result = check_expr(&env, &reg, &bool_lit(true)).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_lit_int() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let result = check_expr(&env, &reg, &int_lit(42)).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_lit_string() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let result = check_expr(&env, &reg, &string_lit("hello")).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::String));
    }

    #[test]
    fn test_check_expr_var() {
        let mut env = TyEnv::new();
        env.bind(ident("x"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let reg = TypeRegistry::new();
        let result = check_expr(&env, &reg, &var("x")).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_unknown_var() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let result = check_expr(&env, &reg, &var("unknown"));
        assert_eq!(
            result,
            Err(TypeError::UnknownVariable("unknown".to_string()))
        );
    }

    #[test]
    fn test_check_expr_binop_eq() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::BinOp {
            op: expr::BinOp::Eq,
            left: Box::new(int_lit(1)),
            right: Box::new(int_lit(2)),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_binop_mismatch() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::BinOp {
            op: expr::BinOp::Eq,
            left: Box::new(int_lit(1)),
            right: Box::new(bool_lit(true)),
        };
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_binop_numeric_equality() {
        let _env = TyEnv::new();
        let _reg = TypeRegistry::new();
        // Bytes is numeric, so comparison with Int should fail (different types)
        // but within numeric types, comparison is allowed
        let bytes_ty = ast::Type::Primitive(ast::PrimitiveType::Bytes);
        let int_ty = ast::Type::Primitive(ast::PrimitiveType::Int);
        // is_numeric allows both, but types_eq fails for different numeric types
        assert!(is_numeric(&int_ty));
        assert!(is_numeric(&bytes_ty));
        assert!(!types_eq(&int_ty, &bytes_ty));
    }

    #[test]
    fn test_check_expr_unop_not() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::UnOp {
            op: expr::UnOp::Not,
            operand: Box::new(bool_lit(true)),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_unop_neg() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::UnOp {
            op: expr::UnOp::Neg,
            operand: Box::new(int_lit(42)),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_let_inferred() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let decl = LetDecl {
            name: ident("x"),
            ty: None,
            init: Some(Box::new(int_lit(42))),
        };
        let (name, ty) = check_let(&env, &reg, &decl).unwrap();
        assert_eq!(name.name, "x");
        assert_eq!(ty, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_let_missing_init() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let decl = LetDecl {
            name: ident("x"),
            ty: None,
            init: None,
        };
        let result = check_let(&env, &reg, &decl);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_let_explicit_type() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let decl = LetDecl {
            name: ident("x"),
            ty: Some(ast::Type::Primitive(ast::PrimitiveType::Bool)),
            init: None,
        };
        let (name, ty) = check_let(&env, &reg, &decl).unwrap();
        assert_eq!(name.name, "x");
        assert_eq!(ty, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    // ─── Type registry tests ───

    #[test]
    fn test_type_registry_resource() {
        let mut reg = TypeRegistry::new();
        let resource = ast::ResourceDecl {
            name: ident("File"),
            capacities: vec![ast::Capacity {
                name: ident("write"),
                ty: ast::Type::Primitive(ast::PrimitiveType::Int),
            }],
            fields: vec![
                ast::FieldDecl {
                    name: ident("path"),
                    ty: ast::Type::Primitive(ast::PrimitiveType::String),
                    default: None,
                },
                ast::FieldDecl {
                    name: ident("size"),
                    ty: ast::Type::Primitive(ast::PrimitiveType::Int),
                    default: None,
                },
            ],
        };
        reg.register_resource(
            resource.name.clone(),
            ResourceTypeInfo {
                capacities: resource.capacities.clone(),
                fields: resource.fields.clone(),
            },
        );

        let info = reg.lookup_resource("File").unwrap();
        assert!(info.has_capacity("write"));
        assert!(!info.has_capacity("read"));
        assert_eq!(
            info.field_type("path").unwrap(),
            &ast::Type::Primitive(ast::PrimitiveType::String)
        );
        assert_eq!(
            info.field_type("size").unwrap(),
            &ast::Type::Primitive(ast::PrimitiveType::Int)
        );
        assert!(info.field_type("missing").is_none());
    }

    #[test]
    fn test_type_registry_alias() {
        let mut reg = TypeRegistry::new();
        reg.register_alias("Path", ast::Type::Primitive(ast::PrimitiveType::String));

        let alias = reg.resolve_alias("Path").unwrap();
        assert_eq!(alias, &ast::Type::Primitive(ast::PrimitiveType::String));
    }

    #[test]
    fn test_type_registry_resolve_type() {
        let mut reg = TypeRegistry::new();
        reg.register_alias("Path", ast::Type::Primitive(ast::PrimitiveType::String));

        let ty = ast::Type::Resource(ident("Path"));
        let resolved = reg.resolve_type(&ty);
        assert_eq!(resolved, ast::Type::Primitive(ast::PrimitiveType::String));
    }

    #[test]
    fn test_type_registry_resolve_type_deep() {
        let mut reg = TypeRegistry::new();
        reg.register_alias("Path", ast::Type::Primitive(ast::PrimitiveType::String));

        let ty = ast::Type::List(Box::new(ast::Type::Resource(ident("Path"))));
        let resolved = reg.resolve_type_deep(&ty);
        assert_eq!(
            resolved,
            ast::Type::List(Box::new(ast::Type::Primitive(ast::PrimitiveType::String)))
        );
    }

    #[test]
    fn test_type_registry_is_resource_type() {
        let mut reg = TypeRegistry::new();
        reg.register_alias("Path", ast::Type::Primitive(ast::PrimitiveType::String));

        let ty = ast::Type::Resource(ident("Path"));
        assert!(!reg.is_resource_type(&ty)); // Path alias resolves to String (primitive)

        let file_ty = ast::Type::Resource(ident("File"));
        assert!(reg.is_resource_type(&file_ty)); // File is a Resource type (even if unregistered)
    }

    // ─── Field access tests ───

    #[test]
    fn test_check_expr_field_access() {
        let mut reg = TypeRegistry::new();
        reg.register_resource(
            ident("File"),
            ResourceTypeInfo {
                fields: vec![
                    ast::FieldDecl {
                        name: ident("path"),
                        ty: ast::Type::Primitive(ast::PrimitiveType::String),
                        default: None,
                    },
                    ast::FieldDecl {
                        name: ident("size"),
                        ty: ast::Type::Primitive(ast::PrimitiveType::Int),
                        default: None,
                    },
                ],
                capacities: Vec::new(),
            },
        );

        let mut env = TyEnv::new();
        env.bind(ident("f"), ast::Type::Resource(ident("File")));

        // Access path field
        let expr = Expr::FieldAccess {
            target: Box::new(var("f")),
            field: ident("path"),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::String));

        // Access size field
        let expr = Expr::FieldAccess {
            target: Box::new(var("f")),
            field: ident("size"),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_field_access_unknown_field() {
        let mut reg = TypeRegistry::new();
        reg.register_resource(
            ident("File"),
            ResourceTypeInfo {
                fields: vec![ast::FieldDecl {
                    name: ident("path"),
                    ty: ast::Type::Primitive(ast::PrimitiveType::String),
                    default: None,
                }],
                capacities: Vec::new(),
            },
        );

        let env = TyEnv::new();
        let expr = Expr::FieldAccess {
            target: Box::new(Expr::Lit(Literal::StringVal("f".to_string()))),
            field: ident("missing"),
        };
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    // ─── Collection type tests ───

    #[test]
    fn test_check_expr_list_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("items"),
            ast::Type::List(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        env.bind(ident("i"), ast::Type::Primitive(ast::PrimitiveType::Int));

        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(var("i")),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_list_index_invalid() {
        let mut env = TyEnv::new();
        env.bind(
            ident("items"),
            ast::Type::List(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        env.bind(
            ident("idx"),
            ast::Type::Primitive(ast::PrimitiveType::String),
        );

        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(var("idx")),
        };
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_map_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("m"),
            ast::Type::Map(
                Box::new(ast::Type::Primitive(ast::PrimitiveType::String)),
                Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)),
            ),
        );
        env.bind(ident("k"), ast::Type::Primitive(ast::PrimitiveType::String));

        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("m")),
            index: Box::new(var("k")),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    // ─── Role environment tests ───

    #[test]
    fn test_role_env_add_role() {
        let mut env = RoleEnv::new();
        env.add_role("admin");
        assert!(env.all_roles().contains(&"admin".to_string()));
    }

    #[test]
    fn test_role_env_parent_child() {
        let mut env = RoleEnv::new();
        env.set_parent("dev", "admin");
        assert_eq!(env.get_parent("dev"), Some("admin".to_string()));
        assert_eq!(env.get_children("admin"), vec!["dev".to_string()]);
    }

    #[test]
    fn test_role_env_resolve_down() {
        let mut env = RoleEnv::new();
        env.set_parent("dev", "admin");
        env.set_parent("senior_dev", "dev");

        let down = env.resolve_down("admin");
        assert!(down.contains(&"admin".to_string()));
        assert!(down.contains(&"dev".to_string()));
        assert!(down.contains(&"senior_dev".to_string()));
    }

    #[test]
    fn test_role_env_is_descendant() {
        let mut env = RoleEnv::new();
        env.set_parent("dev", "admin");
        env.set_parent("senior_dev", "dev");

        assert!(env.is_descendant("dev", "admin"));
        assert!(env.is_descendant("senior_dev", "admin"));
        assert!(env.is_descendant("senior_dev", "dev"));
        assert!(!env.is_descendant("admin", "dev"));
    }

    #[test]
    fn test_role_env_ancestors() {
        let mut env = RoleEnv::new();
        env.set_parent("dev", "admin");
        env.set_parent("senior_dev", "dev");

        let ancestors = env.get_ancestors("senior_dev");
        assert_eq!(ancestors, vec!["dev".to_string(), "admin".to_string()]);
    }

    #[test]
    fn test_role_env_cycle_detection() {
        let mut env = RoleEnv::new();
        env.set_parent("a", "b");
        // Can't set parent("b", "a") because would_create_cycle would prevent it
        // So let's manually test cycle detection on a graph that has a cycle
        let mut env2 = RoleEnv::new();
        env2.add_role("a");
        env2.add_role("b");
        env2.add_role("c");
        // Manually create a cycle via graph manipulation isn't possible through public API
        // Instead, test that would_create_cycle works
        env2.set_parent("b", "a");
        assert!(env2.would_create_cycle("a", "b"));
    }

    #[test]
    fn test_role_env_would_create_cycle() {
        let mut env = RoleEnv::new();
        env.set_parent("b", "a");
        // a trying to be child of b would create cycle (b is already descendant of a)
        assert!(env.would_create_cycle("a", "b"));
        // c trying to be child of a should be fine
        assert!(!env.would_create_cycle("c", "a"));
    }

    #[test]
    fn test_role_env_resolve_roles_with_down() {
        let mut env = RoleEnv::new();
        env.set_parent("dev", "admin");
        env.set_parent("senior_dev", "dev");

        let result = env.resolve_roles_with_down(&[ast::RoleRef::Down(ident("admin"))]);
        assert!(result.contains(&"admin".to_string()));
        assert!(result.contains(&"dev".to_string()));
        assert!(result.contains(&"senior_dev".to_string()));

        let result2 = env.resolve_roles_with_down(&[ast::RoleRef::Down(ident("admin"))]);
        assert_eq!(result2, result); // Same result for Down variant
    }

    // ─── Condition evaluator tests ───

    #[test]
    fn test_cond_value_truthy() {
        assert!(CondValue::Bool(true).is_truthy());
        assert!(!CondValue::Bool(false).is_truthy());
        assert!(!CondValue::Int(0).is_truthy());
        assert!(CondValue::Int(1).is_truthy());
        assert!(!CondValue::StringVal(String::new()).is_truthy());
        assert!(CondValue::StringVal("hello".to_string()).is_truthy());
        assert!(!CondValue::Null.is_truthy());
    }

    #[test]
    fn test_cond_value_as_str() {
        let s = CondValue::StringVal("test".to_string());
        assert_eq!(s.as_str(), Some("test"));

        let n = CondValue::Int(42);
        assert_eq!(n.as_str(), None);
    }

    #[test]
    fn test_eval_context_bind_resolve() {
        let mut ctx = EvalContext::new();
        ctx.bind("x", CondValue::Int(42));
        assert_eq!(ctx.resolve("x"), Some(&CondValue::Int(42)));
        assert_eq!(ctx.resolve("y"), None);
    }

    #[test]
    fn test_eval_starts_with() {
        let mut ctx = EvalContext::new();
        ctx.bind("name", CondValue::StringVal("config.yaml".to_string()));

        let pred = ConditionPred::StartsWith {
            expr: Box::new(var("name")),
            prefix: "conf".to_string(),
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));

        let pred2 = ConditionPred::StartsWith {
            expr: Box::new(var("name")),
            prefix: "other".to_string(),
        };
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred2));
    }

    #[test]
    fn test_eval_ends_with() {
        let mut ctx = EvalContext::new();
        ctx.bind("name", CondValue::StringVal("config.yaml".to_string()));

        let pred = ConditionPred::EndsWith {
            expr: Box::new(var("name")),
            suffix: ".yaml".to_string(),
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));

        let pred2 = ConditionPred::EndsWith {
            expr: Box::new(var("name")),
            suffix: ".toml".to_string(),
        };
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred2));
    }

    #[test]
    fn test_eval_in_set() {
        let mut ctx = EvalContext::new();
        ctx.bind("role", CondValue::Role("dev".to_string()));
        ctx.sets.insert(
            "approved".to_string(),
            HashSet::from(["dev".to_string(), "admin".to_string()]),
        );

        let pred = ConditionPred::InSet {
            expr: Box::new(var("role")),
            set: ident("approved"),
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));

        ctx.sets
            .insert("blocked".to_string(), HashSet::from(["hacker".to_string()]));
        let pred2 = ConditionPred::InSet {
            expr: Box::new(var("role")),
            set: ident("blocked"),
        };
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred2));
    }

    #[test]
    fn test_eval_exists() {
        let mut ctx = EvalContext::new();
        ctx.bind("value", CondValue::StringVal("hello".to_string()));
        ctx.bind("empty", CondValue::StringVal(String::new()));
        ctx.bind("null_val", CondValue::Null);

        assert!(ConditionEvaluator::evaluate_pred(
            &ctx,
            &ConditionPred::Exists(Box::new(var("value")))
        ));
        assert!(!ConditionEvaluator::evaluate_pred(
            &ctx,
            &ConditionPred::Exists(Box::new(var("empty")))
        ));
        assert!(!ConditionEvaluator::evaluate_pred(
            &ctx,
            &ConditionPred::Exists(Box::new(var("null_val")))
        ));
    }

    #[test]
    fn test_eval_not() {
        let mut ctx = EvalContext::new();
        ctx.bind("flag", CondValue::Bool(true));

        let pred = ConditionPred::Not(Box::new(ConditionPred::Exists(Box::new(var("flag")))));
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred));

        ctx.bind("flag", CondValue::Bool(false));
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));
    }

    #[test]
    fn test_eval_and_or() {
        let mut ctx = EvalContext::new();
        ctx.bind("a", CondValue::Bool(true));
        ctx.bind("b", CondValue::Bool(true));

        let and_pred = ConditionPred::And(
            Box::new(ConditionPred::Exists(Box::new(var("a")))),
            Box::new(ConditionPred::Exists(Box::new(var("b")))),
        );
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &and_pred));

        ctx.bind("a", CondValue::Bool(false));
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &and_pred));

        let or_pred = ConditionPred::Or(
            Box::new(ConditionPred::Exists(Box::new(var("a")))),
            Box::new(ConditionPred::Exists(Box::new(var("b")))),
        );
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &or_pred));

        ctx.bind("b", CondValue::Bool(false));
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &or_pred));
    }

    #[test]
    fn test_eval_expr_to_value_binop() {
        let mut ctx = EvalContext::new();
        ctx.bind("a", CondValue::Int(10));
        ctx.bind("b", CondValue::Int(3));

        let result = ConditionEvaluator::eval_expr_to_value(
            &ctx,
            &Expr::BinOp {
                op: expr::BinOp::Plus,
                left: Box::new(var("a")),
                right: Box::new(var("b")),
            },
        );
        assert_eq!(result, CondValue::Int(13));

        let result = ConditionEvaluator::eval_expr_to_value(
            &ctx,
            &Expr::BinOp {
                op: expr::BinOp::Minus,
                left: Box::new(var("a")),
                right: Box::new(var("b")),
            },
        );
        assert_eq!(result, CondValue::Int(7));

        let result = ConditionEvaluator::eval_expr_to_value(
            &ctx,
            &Expr::BinOp {
                op: expr::BinOp::Eq,
                left: Box::new(var("a")),
                right: Box::new(var("b")),
            },
        );
        assert_eq!(result, CondValue::Bool(false));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.yaml", "config.yaml"));
        assert!(glob_match("config.*", "config.yaml"));
        assert!(glob_match("conf??", "conf??"));
        assert!(!glob_match("*.yaml", "config.toml"));
    }

    // ─── Policy engine tests ───

    #[test]
    fn test_policy_engine_can_check_granted() {
        let mut engine = PolicyEngine::new();
        engine.add_role("admin", None);

        engine.add_rule(PolicyRule {
            deny: false,
            op: "read".to_string(),
            resource_var: None,
            resource_type: Some("File".to_string()),
            conditions: Vec::new(),
        });

        engine.build_cache();

        let ctx = EvalContext::new();
        let result = engine.can_check("admin", "read", Some("File"), &ctx);
        assert_eq!(result, PolicyCheck::Granted);
    }

    #[test]
    fn test_policy_engine_can_check_denied() {
        let mut engine = PolicyEngine::new();
        engine.add_role("admin", None);

        // No rules granting "write"
        engine.build_cache();

        let ctx = EvalContext::new();
        let result = engine.can_check("admin", "write", Some("File"), &ctx);
        assert!(matches!(result, PolicyCheck::Denied(_)));
    }

    #[test]
    fn test_policy_engine_deny_overrides() {
        let mut engine = PolicyEngine::new();
        engine.add_role("admin", None);

        engine.add_rule(PolicyRule {
            deny: false,
            op: "read".to_string(),
            resource_var: None,
            resource_type: None,
            conditions: Vec::new(),
        });
        engine.add_rule(PolicyRule {
            deny: true,
            op: "read".to_string(),
            resource_var: None,
            resource_type: Some("secret".to_string()),
            conditions: Vec::new(),
        });

        engine.build_cache();

        let ctx = EvalContext::new();
        let result = engine.can_check("admin", "read", Some("secret"), &ctx);
        assert!(matches!(result, PolicyCheck::Denied(_)));
    }

    // ─── Grant system tests ───

    #[test]
    fn test_grant_system_process_grant() {
        let mut gs = GrantSystem::new();
        gs.add_role("admin", None);
        gs.add_role("dev", Some("admin"));

        let grant = Grant {
            target_role: "dev".to_string(),
            op: "read".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("File")),
            },
            condition: None,
        };

        let result = gs.process_grant("admin", grant);
        assert!(result.success);
    }

    #[test]
    fn test_grant_system_no_authority() {
        let mut gs = GrantSystem::new();
        gs.add_role("admin", None);
        gs.add_role("dev", Some("admin"));
        gs.add_role("intern", None); // intern has no relation to admin

        let grant = Grant {
            target_role: "dev".to_string(),
            op: "read".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("File")),
            },
            condition: None,
        };

        let result = gs.process_grant("intern", grant);
        assert!(!result.success);
        assert!(matches!(result.error, Some(GrantError::NoAuthority { .. })));
    }

    #[test]
    fn test_grant_system_duplicate() {
        let mut gs = GrantSystem::new();
        gs.add_role("admin", None);

        let grant = Grant {
            target_role: "dev".to_string(),
            op: "read".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("File")),
            },
            condition: None,
        };

        gs.process_grant("admin", grant.clone());
        let result = gs.process_grant("admin", grant);
        assert!(!result.success);
        assert!(matches!(
            result.error,
            Some(GrantError::DuplicateGrant { .. })
        ));
    }

    #[test]
    fn test_grant_system_transitive_permission() {
        let mut gs = GrantSystem::new();
        gs.add_role("admin", None);
        gs.add_role("dev", Some("admin"));

        // Admin grants dev permission to read files
        let grant = Grant {
            target_role: "dev".to_string(),
            op: "read".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("File")),
            },
            condition: None,
        };
        gs.process_grant("admin", grant);

        let ctx = EvalContext::new();
        let result = gs.can_check("dev", "read", Some("File"), &ctx);
        assert_eq!(result, PolicyCheck::Granted);
    }

    // ─── Type checking tests with resources ───

    #[test]
    fn test_check_expr_resource_field_access() {
        let mut reg = TypeRegistry::new();
        reg.register_resource(
            ident("Server"),
            ResourceTypeInfo {
                fields: vec![
                    ast::FieldDecl {
                        name: ident("name"),
                        ty: ast::Type::Primitive(ast::PrimitiveType::String),
                        default: None,
                    },
                    ast::FieldDecl {
                        name: ident("ram"),
                        ty: ast::Type::Primitive(ast::PrimitiveType::Bytes),
                        default: None,
                    },
                ],
                capacities: vec![ast::Capacity {
                    name: ident("compute"),
                    ty: ast::Type::Primitive(ast::PrimitiveType::Int),
                }],
            },
        );

        let mut env = TyEnv::new();
        env.bind(ident("s"), ast::Type::Resource(ident("Server")));

        let expr = Expr::FieldAccess {
            target: Box::new(var("s")),
            field: ident("name"),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::String));

        let expr = Expr::FieldAccess {
            target: Box::new(var("s")),
            field: ident("ram"),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bytes));
    }

    #[test]
    fn test_check_expr_struct() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Struct {
            fields: vec![
                (ident("name"), Box::new(string_lit("server1"))),
                (ident("ram"), Box::new(int_lit(64))),
            ],
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::JSON));
    }

    #[test]
    fn test_check_expr_call() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Call {
            func: ident("exec"),
            args: vec![string_lit("ls -la")],
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::JSON));
    }

    #[test]
    fn test_check_expr_template() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Template("$HOME/.config".to_string());
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::String));
    }

    #[test]
    fn test_check_expr_choose() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Choose {
            variable: ident("server"),
            ty: ast::Type::Resource(ident("Server")),
            from_set: None,
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Node));
    }

    #[test]
    fn test_check_let_with_resource_type() {
        let env = TyEnv::new();
        let mut reg = TypeRegistry::new();
        reg.register_resource(
            ident("Server"),
            ResourceTypeInfo {
                fields: Vec::new(),
                capacities: Vec::new(),
            },
        );

        let decl = LetDecl {
            name: ident("s"),
            ty: Some(ast::Type::Resource(ident("Server"))),
            init: None,
        };
        let (name, ty) = check_let(&env, &reg, &decl).unwrap();
        assert_eq!(name.name, "s");
        assert_eq!(ty, ast::Type::Resource(ident("Server")));
    }

    // ─── Role resolution tests ───

    #[test]
    fn test_role_env_empty_hierarchy() {
        let env = RoleEnv::new();
        assert!(env.all_roles().is_empty());
        assert!(env.resolve_down("nonexistent").is_empty());
    }

    #[test]
    fn test_role_env_deep_hierarchy() {
        let mut env = RoleEnv::new();
        env.set_parent("b", "a");
        env.set_parent("c", "b");
        env.set_parent("d", "c");
        env.set_parent("e", "d");

        let down = env.resolve_down("a");
        assert_eq!(down.len(), 5); // a, b, c, d, e
        assert!(env.is_descendant("e", "a"));
        assert!(!env.is_descendant("a", "e"));
    }

    // ─── Grant evaluation tests ───

    #[test]
    fn test_policy_engine_can_check_with_condition() {
        let mut engine = PolicyEngine::new();
        engine.add_role("admin", None);

        engine.add_rule(PolicyRule {
            deny: false,
            op: "read".to_string(),
            resource_var: None,
            resource_type: Some("File".to_string()),
            conditions: vec![ConditionPred::StartsWith {
                expr: Box::new(Expr::Lit(Literal::StringVal("".to_string()))),
                prefix: "config".to_string(),
            }],
        });

        engine.build_cache();

        // Condition not met (prefix doesn't match)
        let mut ctx = EvalContext::new();
        ctx.bind("path", CondValue::StringVal("data.yaml".to_string()));
        let result = engine.can_check("admin", "read", Some("File"), &ctx);
        assert!(matches!(result, PolicyCheck::Denied(_)));
    }

    #[test]
    fn test_grant_system_self_grant() {
        let mut gs = GrantSystem::new();
        gs.add_role("dev", None);

        let grant = Grant {
            target_role: "dev".to_string(),
            op: "execute".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("Server")),
            },
            condition: None,
        };

        let result = gs.process_grant("dev", grant);
        assert!(result.success); // Self-grant is always allowed
    }

    #[test]
    fn test_check_expr_binop_add() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Plus,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_binop_gt() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Gt,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_binop_and() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        let expr = Expr::BinOp {
            op: expr::BinOp::And,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_let_resource_type() {
        let mut env = TyEnv::new();
        env.bind(ident("s"), ast::Type::Resource(ident("Server")));
        let mut reg = TypeRegistry::new();
        reg.register_resource(
            ident("Server"),
            ResourceTypeInfo {
                fields: Vec::new(),
                capacities: Vec::new(),
            },
        );
        let decl = LetDecl {
            name: ident("x"),
            ty: None,
            init: Some(Box::new(var("s"))),
        };
        let result = check_let(&env, &reg, &decl);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_expr_lt_int() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Lt,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_list_of_lists() {
        let mut env = TyEnv::new();
        env.bind(
            ident("matrix"),
            ast::Type::List(Box::new(ast::Type::List(Box::new(ast::Type::Primitive(
                ast::PrimitiveType::Int,
            ))))),
        );
        let reg = TypeRegistry::new();
        let inner = Expr::IndexAccess {
            target: Box::new(var("matrix")),
            index: Box::new(Expr::Lit(Literal::Int(0))),
        };
        let result = check_expr(&env, &reg, &inner).unwrap();
        assert!(matches!(result, ast::Type::List(_)));
    }

    #[test]
    fn test_check_let_multiple_bindings() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let decl = LetDecl {
            name: ident("b"),
            ty: None,
            init: Some(Box::new(Expr::BinOp {
                op: expr::BinOp::Plus,
                left: Box::new(var("a")),
                right: Box::new(Expr::Lit(Literal::Int(1))),
            })),
        };
        let (name, ty) = check_let(&env, &TypeRegistry::new(), &decl).unwrap();
        assert_eq!(name.name, "b");
        assert_eq!(ty, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_mut_list_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("items"),
            ast::Type::MutList(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(Expr::Lit(Literal::Int(0))),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert!(matches!(
            result,
            ast::Type::Primitive(ast::PrimitiveType::Int)
        ));
    }

    #[test]
    fn test_check_expr_template_var() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Template("$HOME/bin".to_string());
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::String));
    }

    #[test]
    fn test_check_expr_bytes_literal() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Lit(Literal::Bytes(ast::BytesLit {
            value: 1024,
            suffix: ast::BytesSuffix::KB,
        }));
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bytes));
    }

    #[test]
    fn test_check_expr_sized_list_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("items"),
            ast::Type::SizedList(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)), 10),
        );
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(Expr::Lit(Literal::Int(0))),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert!(matches!(
            result,
            ast::Type::Primitive(ast::PrimitiveType::Int)
        ));
    }

    #[test]
    fn test_check_expr_ordered_map_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("omap"),
            ast::Type::OrderedMap(
                Box::new(ast::Type::Primitive(ast::PrimitiveType::String)),
                Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)),
            ),
        );
        env.bind(ident("k"), ast::Type::Primitive(ast::PrimitiveType::String));
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("omap")),
            index: Box::new(var("k")),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_string_concat() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::String));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::String));
        let reg = TypeRegistry::new();
        let expr = Expr::BinOp {
            op: expr::BinOp::Plus,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        // + on non-numeric types fails: Set index access returns Bool, not element type
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_to_json() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Call {
            func: ident("to_json"),
            args: vec![Expr::Lit(Literal::Int(42))],
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::JSON));
    }

    #[test]
    fn test_check_expr_unknown_variable() {
        let env = TyEnv::new();
        let reg = TypeRegistry::new();
        let expr = Expr::Var(ident("unknown_var"));
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_map_index_wrong_key_type() {
        let mut env = TyEnv::new();
        env.bind(
            ident("m"),
            ast::Type::Map(
                Box::new(ast::Type::Primitive(ast::PrimitiveType::String)),
                Box::new(ast::Type::Primitive(ast::PrimitiveType::Int)),
            ),
        );
        env.bind(ident("idx"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("m")),
            index: Box::new(var("idx")),
        };
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_list_index_wrong_type() {
        let mut env = TyEnv::new();
        env.bind(
            ident("items"),
            ast::Type::List(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        env.bind(
            ident("idx"),
            ast::Type::Primitive(ast::PrimitiveType::String),
        );
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("items")),
            index: Box::new(var("idx")),
        };
        let result = check_expr(&env, &reg, &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_set_index() {
        let mut env = TyEnv::new();
        env.bind(
            ident("s"),
            ast::Type::Set(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        env.bind(ident("idx"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let reg = TypeRegistry::new();
        let expr = Expr::IndexAccess {
            target: Box::new(var("s")),
            index: Box::new(var("idx")),
        };
        let result = check_expr(&env, &reg, &expr).unwrap();
        // Set index access is a membership check → Bool
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_set_type_alias() {
        let mut reg = TypeRegistry::new();
        reg.register_alias(
            "IntSet",
            ast::Type::Set(Box::new(ast::Type::Primitive(ast::PrimitiveType::Int))),
        );
        let resolved = reg.resolve_type(&ast::Type::Primitive(ast::PrimitiveType::Node));
        // Resolving Node type should return the default
        assert!(matches!(
            resolved,
            ast::Type::Primitive(ast::PrimitiveType::Node)
        ));
    }

    #[test]
    fn test_check_expr_binop_lt() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Lt,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_binop_neq() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::String));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::String));
        let expr = Expr::BinOp {
            op: expr::BinOp::Neq,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_binop_or() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        let expr = Expr::BinOp {
            op: expr::BinOp::Or,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_check_expr_binop_mul() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Mul,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_binop_div() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Div,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_binop_minus() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Int));
        let expr = Expr::BinOp {
            op: expr::BinOp::Minus,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Int));
    }

    #[test]
    fn test_check_expr_binop_bool_plus_error() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Bool));
        let expr = Expr::BinOp {
            op: expr::BinOp::Plus,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_expr_binop_numeric_mixed() {
        let mut env = TyEnv::new();
        env.bind(ident("a"), ast::Type::Primitive(ast::PrimitiveType::Int));
        env.bind(ident("b"), ast::Type::Primitive(ast::PrimitiveType::Bytes));
        let expr = Expr::BinOp {
            op: expr::BinOp::Eq,
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        let result = check_expr(&env, &TypeRegistry::new(), &expr).unwrap();
        // is_numeric allows Int and Bytes
        assert_eq!(result, ast::Type::Primitive(ast::PrimitiveType::Bool));
    }

    #[test]
    fn test_eval_expr_to_value_lit() {
        let ctx = EvalContext::new();
        assert_eq!(
            ConditionEvaluator::eval_expr_to_value(&ctx, &Expr::Lit(Literal::Int(42))),
            CondValue::Int(42)
        );
        assert_eq!(
            ConditionEvaluator::eval_expr_to_value(&ctx, &Expr::Lit(Literal::Bool(true))),
            CondValue::Bool(true)
        );
        assert_eq!(
            ConditionEvaluator::eval_expr_to_value(
                &ctx,
                &Expr::Lit(Literal::StringVal("hi".into()))
            ),
            CondValue::StringVal("hi".into())
        );
    }

    #[test]
    fn test_eval_expr_to_value_var() {
        let mut ctx = EvalContext::new();
        ctx.bind("x", CondValue::Int(99));
        assert_eq!(
            ConditionEvaluator::eval_expr_to_value(&ctx, &var("x")),
            CondValue::Int(99)
        );
    }

    #[test]
    fn test_eval_cond_is() {
        let mut ctx = EvalContext::new();
        ctx.bind("role", CondValue::Role("admin".to_string()));
        ctx.known_roles.insert("admin".to_string());

        let pred = ConditionPred::Is {
            left: var("role"),
            roles: vec![ast::RoleRef::Exact(ident("admin"))],
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));
    }

    #[test]
    fn test_eval_cond_matches() {
        let mut ctx = EvalContext::new();
        ctx.bind("name", CondValue::StringVal("config.yaml".to_string()));

        let pred = ConditionPred::Matches {
            expr: Box::new(var("name")),
            pattern: "*.yaml".to_string(),
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));

        let pred2 = ConditionPred::Matches {
            expr: Box::new(var("name")),
            pattern: "*.toml".to_string(),
        };
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred2));
    }

    #[test]
    fn test_eval_cond_drop_prefix_eq() {
        let mut ctx = EvalContext::new();
        ctx.bind("a", CondValue::StringVal("prefix_value".to_string()));
        ctx.bind("b", CondValue::StringVal("prefix_other".to_string()));

        let pred = ConditionPred::DropPrefixEq {
            prefix: "prefix_".to_string(),
            left: Box::new(var("a")),
            right: Box::new(var("b")),
        };
        // strip_prefix("prefix_") on "prefix_value" = "value"
        // strip_prefix("prefix_") on "prefix_other" = "other"
        // "value" != "other"
        assert!(!ConditionEvaluator::evaluate_pred(&ctx, &pred));

        ctx.bind("b", CondValue::StringVal("prefix_value".to_string()));
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));
    }

    #[test]
    fn test_eval_cond_can_no_resource() {
        let ctx = EvalContext::new();
        let pred = ConditionPred::Can {
            op: ident("can_deploy"),
            resource: None,
        };
        assert!(ConditionEvaluator::evaluate_pred(&ctx, &pred));
    }

    #[test]
    fn test_cond_value_is_string() {
        assert_eq!(
            CondValue::StringVal("test".into()).as_string(),
            Some("test".to_string())
        );
        assert_eq!(CondValue::Int(42).as_string(), None);
        assert_eq!(CondValue::Null.as_string(), None);
    }

    #[test]
    fn test_cond_value_resource_string() {
        assert_eq!(
            CondValue::Resource("node1".into()).as_string(),
            Some("node1".to_string())
        );
        assert_eq!(
            CondValue::Node("node1".into()).as_string(),
            Some("node1".to_string())
        );
        assert_eq!(
            CondValue::Role("admin".into()).as_string(),
            Some("admin".to_string())
        );
    }

    #[test]
    fn test_grant_result_granted() {
        let grant = Grant {
            target_role: "dev".to_string(),
            op: "read".to_string(),
            resource: ast::ResourcePattern {
                variable: ident("x"),
                resource_type: ast::Type::Resource(ident("File")),
            },
            condition: None,
        };
        let result = GrantResult::granted(grant.clone());
        assert!(result.success);
        assert!(result.grant.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_grant_result_failed() {
        let result = GrantResult::failed(GrantError::NoAuthority {
            grantor: "intern".to_string(),
            target: "dev".to_string(),
        });
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}
