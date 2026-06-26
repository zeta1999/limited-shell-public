//! Abstract Syntax Tree for the Limited Shell language.
//!
//! Covers all declaration types (role, resource, device, machine,
//! operation, service, function, task block, grant, alias).

use std::fmt;

// ─── Top-level ────────────────────────────────────────────────────

/// A complete LS program: items (declarations) followed by statements.
#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
    pub statements: Vec<Statement>,
}

// ─── Items (declarations) ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Item {
    Alias(AliasDecl),
    Role(RoleDecl),
    Resource(ResourceDecl),
    Device(DeviceDecl),
    Machine(MachineDecl),
    Operation(OperationDecl),
    Service(ServiceDecl),
    Function(FunctionDecl),
}

// ─── Statements (executed in order) ───────────────────────────────

#[derive(Debug, Clone)]
pub enum Statement {
    RoleDecl(RoleDecl),
    MachineDecl(MachineDecl),
    Grant(GrantDecl),
    Alias(AliasDecl),
    OnMachine(OnMachineStmt),
    TaskBlock(TaskBlock),
    ControlFlow(ControlFlow),
}

// ─── OnMachine ────────────────────────────────────────────────────

/// `on machine <name>;` or `on machine set <name> { ... };`
#[derive(Debug, Clone)]
pub struct OnMachineStmt {
    pub machines: Machines,
    pub body: Option<Box<TaskBlock>>,
}

/// Either a single machine name or a named machine set.
#[derive(Debug, Clone)]
pub enum Machines {
    Single(Ident),
    Set(Ident),
    Inline(Vec<Ident>),
}

// ─── AliasDecl ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AliasDecl {
    pub kind: AliasKind,
    pub name: Ident,
    pub target: Type,
}

#[derive(Debug, Clone)]
pub enum AliasKind {
    Machine,
    Path,
    Role,
    Generic,
}

// ─── GrantDecl ────────────────────────────────────────────────────

/// `grant <role> can <op> {<r:Res>} if <cond>;`
#[derive(Debug, Clone)]
pub struct GrantDecl {
    pub target_role: Ident,
    pub op: Ident,
    pub resource: ResourcePattern,
    pub condition: Option<Box<Condition>>,
}

// ─── RoleDecl ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RoleDecl {
    pub name: Ident,
    pub up: Option<Ident>,
    pub permissions: Vec<Permission>,
    /// `can define operation for <role_list>`
    pub define_ops_for: Vec<DefineOpsTarget>,
}

#[derive(Debug, Clone)]
pub struct Permission {
    pub deny: bool, // false = can, true = cannot
    pub op: Ident,
    pub resource: Option<ResourcePattern>,
    pub condition: Option<Box<Condition>>,
}

/// `can Op {x:File} if x.machine is salamander`
#[derive(Debug, Clone)]
pub struct ResourcePattern {
    pub variable: Ident,
    pub resource_type: Type,
}

/// `can define operation for Musashi, down, Musashi.down`
#[derive(Debug, Clone)]
pub enum DefineOpsTarget {
    Role(Ident),
    Down(Ident),
    RoleDown(Ident), // `Role.down`
}

// ─── ResourceDecl ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResourceDecl {
    pub name: Ident,
    pub capacities: Vec<Capacity>,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone)]
pub struct Capacity {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: Ident,
    pub ty: Type,
    pub default: Option<Expr>,
}

// ─── DeviceDecl ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DeviceDecl {
    pub name: Ident,
    pub parent: Option<Ident>,
    pub extents: Vec<ExtentDecl>,
    pub rates: Vec<RateDecl>,
    pub cost_rules: Vec<CostRule>,
}

/// `extent NVRAM bytes,` or `extent DISK DISK1 1TB mountpoint /`
#[derive(Debug, Clone)]
pub enum ExtentDecl {
    Simple {
        name: Ident,
        ty: ExtentType,
        default: Option<Expr>,
    },
    Disk {
        name: Ident,
        size: BytesLit,
        mountpoint: Expr,
    },
}

#[derive(Debug, Clone)]
pub enum ExtentType {
    Bytes,
    Count(Ident), // e.g. DISK, NVRAM as a named extent type
}

#[derive(Debug, Clone)]
pub struct RateDecl {
    pub name: Ident,
    pub rate: Type, // bytes/sec
}

#[derive(Debug, Clone)]
pub struct CostRule {
    pub constraints: Vec<CostConstraint>,
}

#[derive(Debug, Clone)]
pub enum CostConstraint {
    SumLt {
        extent: Ident,
        pool: Expr,
    },
    SumOpLt {
        op: SumOp,
        left_extent: Ident,
        right_extent: Ident,
        pool: Expr,
    },
}

/// `sum(cost left) + sum(cost right) <= pool`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumOp {
    Plus,
    Minus,
}

// ─── MachineDecl ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MachineDecl {
    pub name: Ident,
    pub extents: Vec<MachineExtent>,
    pub keys: Vec<BytesLit>, // machine credential keys
    pub devices: Vec<MachineDevice>,
}

#[derive(Debug, Clone)]
pub enum MachineExtent {
    RAM(BytesLit),
    Disk(MachineDisk),
}

#[derive(Debug, Clone)]
pub struct MachineDisk {
    pub name: Ident,
    pub size: BytesLit,
    pub mountpoint: Expr,
}

#[derive(Debug, Clone)]
pub struct MachineDevice {
    pub name: Ident,
    pub device_type: Ident,
    pub extent_bindings: Vec<(Ident, Expr)>, // named bindings like { SharedRAM = 64GB }
}

// ─── OperationDecl ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OperationDecl {
    pub name: Ident,
    pub params: Vec<NamedType>,
    pub requires: Vec<Condition>,
    pub allow: Option<Ident>, // `allow if role is <allow>`
    pub options: Vec<OperationOption>,
    pub cost: Option<OperationCost>,
}

#[derive(Debug, Clone)]
pub struct OperationOption {
    pub name: Ident,
    pub body: Vec<OperationStatement>,
}

#[derive(Debug, Clone)]
pub enum OperationStatement {
    Require(Condition),
    Choose(ChooseExpr),
    LetDecl(LetDecl),
    OnMachine(Ident),
    ExecCommand {
        cmd: Ident,
        args: Vec<Expr>,
    },
    Transfer {
        from: Expr,
        machine: Expr,
        location: Expr,
    },
    ShellCmd {
        cmd: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ChooseExpr {
    pub variable: Ident,
    pub ty: Type,
    pub from_set: Option<Ident>,
}

#[derive(Debug, Clone)]
pub struct OperationCost {
    pub costs: Vec<CostEntry>,
}

#[derive(Debug, Clone)]
pub struct CostEntry {
    pub kind: Ident, // GPUVRAM, start, stop, RAM, etc.
    pub value: Expr,
}

// ─── ServiceDecl ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ServiceDecl {
    pub name: Ident,
    pub params: Vec<NamedType>,
    pub on: Ident,
    pub costs: Vec<CostEntry>,
}

// ─── FunctionDecl ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: Ident,
    pub params: Vec<NamedType>,
    pub requires: Vec<Condition>,
    pub allow: Option<Ident>,
    pub body: Vec<FunctionStatement>,
    pub success_if: Option<Box<Condition>>,
    pub failure: Option<FailAction>,
}

#[derive(Debug, Clone)]
pub enum FunctionStatement {
    Require(Condition),
    LetDecl(LetDecl),
    OnMachine(Ident),
    SetEnv {
        name: Ident,
        secret: SecretSource,
    },
    ExecCommand {
        cmd: Ident,
        args: Vec<Expr>,
    },
    ReadJson {
        var: Ident,
    },
    WriteJson {
        value: Expr,
    },
    Transfer {
        from: Expr,
        machine: Expr,
        location: Expr,
    },
    Dependency {
        service_name: Ident,
        service_param: Option<Expr>,
        on_machine: Ident,
        as_name: Ident,
    },
    Return(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum SecretSource {
    CmdSecret { machine: Ident },
    Env(Ident),
}

#[derive(Debug, Clone)]
pub enum FailAction {
    Otherwise,
}

// ─── TaskBlock ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TaskBlock {
    pub machines: Machines,
    pub body: Vec<TaskItem>,
}

#[derive(Debug, Clone)]
pub enum TaskItem {
    Bind {
        variable: Ident,
        assignment: Box<Expr>,
    },
    OpCall {
        op: Ident,
        args: Vec<Expr>,
    },
    OpCallArgs {
        op: Ident,
        args: Vec<Ident>,
    },
    ExprTask(Box<Expr>),
    RemoteWrite {
        variable: Ident,
        machine: Ident,
        path: Expr,
    },
    Optimize {
        metric: Ident, // time, RAM, cost
    },
    Dependency {
        service_name: Ident,
        service_param: Option<Expr>,
        on_machine: Ident,
        as_name: Ident,
    },
}

// ─── LetDecl ──────────────────────────────────────────────────────

/// `let x: Type = expr` or `let x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetDecl {
    pub name: Ident,
    pub ty: Option<Type>,
    pub init: Option<Box<Expr>>,
}

// ─── ControlFlow ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ControlFlow {
    For(ForLoop),
    While(WhileLoop),
    If(IfStmt),
    TryCatch(TryCatch),
}

#[derive(Debug, Clone)]
pub enum ForLoop {
    /// for i in list { ... }
    List {
        var: Ident,
        iterable: Box<Expr>,
        body: Vec<Statement>,
    },
    /// for k, v in dict { ... }
    Dict {
        key_var: Ident,
        value_var: Ident,
        iterable: Box<Expr>,
        body: Vec<Statement>,
    },
}

#[derive(Debug, Clone)]
pub struct WhileLoop {
    pub can_tell: Ident,
    pub condition: Box<Expr>,
    pub tell_func: Ident,
    pub tell_args: Vec<Expr>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: Box<Expr>,
    pub then_body: Vec<Statement>,
    pub else_if: Vec<(Box<Expr>, Vec<Statement>)>,
    pub else_body: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct TryCatch {
    pub body: Vec<Statement>,
    pub catch_err_var: Option<Ident>,
    pub catch_body: Vec<Statement>,
    pub catch_all: Vec<Statement>,
    pub finally_body: Vec<Statement>,
}

// ─── Ident & Type ─────────────────────────────────────────────────

/// An identifier (unqualified or dotted).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
}

/// A named type annotation: `name: Type`.
#[derive(Debug, Clone)]
pub struct NamedType {
    pub name: Ident,
    pub ty: Type,
}

/// A named argument (with optional default): `name: Type` or `name = expr`.
#[derive(Debug, Clone)]
pub struct NamedArg {
    pub name: Ident,
    pub default: Option<Box<Expr>>,
}

// ─── Type ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Primitive types
    Primitive(PrimitiveType),
    /// `[T]` — list
    List(Box<Type>),
    /// `[mut T]` — mutable list
    MutList(Box<Type>),
    /// `map of K to V`
    Map(Box<Type>, Box<Type>),
    /// `ordered map of K to V`
    OrderedMap(Box<Type>, Box<Type>),
    /// `set of T`
    Set(Box<Type>),
    /// `ordered set of T`
    OrderedSet(Box<Type>),
    /// `new list of T(n)` — sized list
    SizedList(Box<Type>, usize),
    /// User-defined resource type
    Resource(Ident),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Unit,
    Bool,
    Int,
    String,
    Bytes,
    Duration,
    FilePath,
    Node,
    Role,
    Secret,
    JSON,
}

// ─── Condition ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Condition {
    pub predicates: Vec<ConditionPred>,
}

#[derive(Debug, Clone)]
pub enum ConditionPred {
    /// `x is Role`
    Is { left: Expr, roles: Vec<RoleRef> },
    /// `can Op {x:Res}`
    Can {
        op: Ident,
        resource: Option<ResourcePattern>,
    },
    /// `x starts with "prefix"`
    StartsWith { expr: Box<Expr>, prefix: String },
    /// `x ends with "suffix"`
    EndsWith { expr: Box<Expr>, suffix: String },
    /// `drop "prefix" a is drop "prefix" b`
    DropPrefixEq {
        prefix: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `x in set_name`
    InSet { expr: Box<Expr>, set: Ident },
    /// `exists expr`
    Exists(Box<Expr>),
    /// `expr matches regex`
    Matches { expr: Box<Expr>, pattern: String },
    /// Negation
    Not(Box<ConditionPred>),
    /// Conjunction
    And(Box<ConditionPred>, Box<ConditionPred>),
    /// Disjunction
    Or(Box<ConditionPred>, Box<ConditionPred>),
}

#[derive(Debug, Clone)]
pub enum RoleRef {
    Exact(Ident),
    Down(Ident),
    RoleDown(Ident),
}

// ─── BytesLit ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytesLit {
    pub value: u64,
    pub suffix: BytesSuffix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytesSuffix {
    None,
    KB,  // 1000
    KiB, // 1024
    MB,  // 1_000_000
    MiB, // 1_048_576
    GB,  // 1_000_000_000
    GiB,
    TB,
    TiB,
}

// ─── Expr ─────────────────────────────────────────────────────────

pub mod expr {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Expr {
        /// Literal values
        Lit(Literal),
        /// Variable reference
        Var(Ident),
        /// Struct/map literal: { k: v, ... }
        Struct { fields: Vec<(Ident, Box<Expr>)> },
        /// Field access: x.field
        FieldAccess { target: Box<Expr>, field: Ident },
        /// Index access: x[key]
        IndexAccess { target: Box<Expr>, index: Box<Expr> },
        /// Function call: f(a, b)
        Call { func: Ident, args: Vec<Expr> },
        /// Binary operation
        BinOp {
            op: BinOp,
            left: Box<Expr>,
            right: Box<Expr>,
        },
        /// Unary operation
        UnOp { op: UnOp, operand: Box<Expr> },
        /// Template interpolation: `$HOME/.x/y`
        Template(String),
        /// `choose { var: Type }`
        Choose {
            variable: Ident,
            ty: Type,
            from_set: Option<Ident>,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Literal {
        Bool(bool),
        Int(i64),
        Bytes(BytesLit),
        StringVal(String),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BinOp {
        Eq,
        Neq,
        Lt,
        Le,
        Gt,
        Ge,
        And,
        Or,
        Plus,
        Minus,
        Mul,
        Div,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum UnOp {
        Not,
        Neg,
    }
}

// Make Expr the primary expression type with a module prefix
pub use expr::*;

// ─── Display helpers ──────────────────────────────────────────────

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => write!(f, "()"),
            Self::Bool => write!(f, "Bool"),
            Self::Int => write!(f, "Int"),
            Self::String => write!(f, "String"),
            Self::Bytes => write!(f, "Bytes"),
            Self::Duration => write!(f, "Duration"),
            Self::FilePath => write!(f, "FilePath"),
            Self::Node => write!(f, "Node"),
            Self::Role => write!(f, "Role"),
            Self::Secret => write!(f, "Secret"),
            Self::JSON => write!(f, "JSON"),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(p) => write!(f, "{}", p),
            Self::List(inner) => write!(f, "[{}]", inner),
            Self::MutList(inner) => write!(f, "[mut {}]", inner),
            Self::Map(key, val) => write!(f, "map of {} to {}", key, val),
            Self::OrderedMap(key, val) => write!(f, "ordered map of {} to {}", key, val),
            Self::Set(inner) => write!(f, "set of {}", inner),
            Self::OrderedSet(inner) => write!(f, "ordered set of {}", inner),
            Self::SizedList(inner, size) => write!(f, "new list of {}({})", inner, size),
            Self::Resource(name) => write!(f, "{}", name),
        }
    }
}

impl fmt::Display for BytesLit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)?;
        match self.suffix {
            BytesSuffix::None => Ok(()),
            BytesSuffix::KB => write!(f, "KB"),
            BytesSuffix::KiB => write!(f, "KiB"),
            BytesSuffix::MB => write!(f, "MB"),
            BytesSuffix::MiB => write!(f, "MiB"),
            BytesSuffix::GB => write!(f, "GB"),
            BytesSuffix::GiB => write!(f, "GiB"),
            BytesSuffix::TB => write!(f, "TB"),
            BytesSuffix::TiB => write!(f, "TiB"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ident_display() {
        let id = Ident { name: "my_var".into() };
        assert_eq!(format!("{}", id), "my_var");
    }

    #[test]
    fn test_program_empty() {
        let prog = Program {
            items: vec![],
            statements: vec![],
        };
        assert!(prog.items.is_empty());
        assert!(prog.statements.is_empty());
    }

    #[test]
    fn test_primitive_type_display() {
        assert_eq!(format!("{}", PrimitiveType::Bool), "Bool");
        assert_eq!(format!("{}", PrimitiveType::Int), "Int");
        assert_eq!(format!("{}", PrimitiveType::String), "String");
        assert_eq!(format!("{}", PrimitiveType::JSON), "JSON");
    }


    #[test]
    fn test_alias_kind_clone() {
        let kind = AliasKind::Machine;
        let _cloned = kind.clone();
    }

    #[test]
    fn test_extent_type_count() {
        let count = ExtentType::Count(Ident { name: "CPU".into() });
        assert!(matches!(count, ExtentType::Count(_)));
    }

    #[test]
    fn test_extent_type_bytes() {
        let bytes = ExtentType::Bytes;
        assert!(matches!(bytes, ExtentType::Bytes));
    }

    #[test]
    fn test_define_ops_target_clone() {
        let target = DefineOpsTarget::Role(Ident { name: "admin".into() });
        let _cloned = target.clone();
    }

    #[test]
    fn test_machine_extent_ram() {
        let ram = MachineExtent::RAM(BytesLit { value: 1024, suffix: BytesSuffix::KiB });
        match ram {
            MachineExtent::RAM(b) => assert_eq!(format!("{}", b), "1024KiB"),
            _ => panic!("expected RAM"),
        }
    }

    #[test]
    fn test_cost_constraint_sum_lt() {
        let constraint = CostConstraint::SumLt {
            extent: Ident { name: "disk".into() },
            pool: Expr::Var(Ident { name: "max_disk".into() }),
        };
        assert!(matches!(constraint, CostConstraint::SumLt { .. }));
    }

    #[test]
    fn test_cost_constraint_sum_op() {
        let constraint = CostConstraint::SumOpLt {
            op: SumOp::Plus,
            left_extent: Ident { name: "cpu".into() },
            right_extent: Ident { name: "memory".into() },
            pool: Expr::Var(Ident { name: "total".into() }),
        };
        assert!(matches!(constraint, CostConstraint::SumOpLt { op, .. } if op == SumOp::Plus));
    }

    #[test]
    fn test_sum_op_values() {
        assert!(matches!(SumOp::Plus, SumOp::Plus));
        assert!(matches!(SumOp::Minus, SumOp::Minus));
    }

    #[test]
    fn test_expr_var() {
        let expr = Expr::Var(Ident { name: "x".into() });
        assert!(matches!(expr, Expr::Var(_)));
    }

    #[test]
    fn test_expr_lit_int() {
        let expr = Expr::Lit(Literal::Int(42));
        assert!(matches!(expr, Expr::Lit(Literal::Int(42))));
    }

    #[test]
    fn test_expr_lit_bool() {
        let expr = Expr::Lit(Literal::Bool(true));
        assert!(matches!(expr, Expr::Lit(Literal::Bool(true))));
    }

    #[test]
    fn test_expr_lit_string() {
        let expr = Expr::Lit(Literal::StringVal("hello".into()));
        assert!(matches!(expr, Expr::Lit(Literal::StringVal(s)) if s == "hello"));
    }

    #[test]
    fn test_expr_binop() {
        let expr = Expr::BinOp {
            op: BinOp::Plus,
            left: Box::new(Expr::Lit(Literal::Int(1))),
            right: Box::new(Expr::Lit(Literal::Int(2))),
        };
        assert!(matches!(expr, Expr::BinOp { op: BinOp::Plus, .. }));
    }

    #[test]
    fn test_expr_unop_neg() {
        let expr = Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::Lit(Literal::Int(5))),
        };
        assert!(matches!(expr, Expr::UnOp { op: UnOp::Neg, .. }));
    }

    #[test]
    fn test_expr_unop_not() {
        let expr = Expr::UnOp {
            op: UnOp::Not,
            operand: Box::new(Expr::Lit(Literal::Bool(true))),
        };
        assert!(matches!(expr, Expr::UnOp { op: UnOp::Not, .. }));
    }

    #[test]
    fn test_expr_call() {
        let expr = Expr::Call {
            func: Ident { name: "len".into() },
            args: vec![Expr::Var(Ident { name: "items".into() })],
        };
        assert!(matches!(expr, Expr::Call { func, .. } if func.name == "len"));
    }

    #[test]
    fn test_expr_field_access() {
        let expr = Expr::FieldAccess {
            target: Box::new(Expr::Var(Ident { name: "machine".into() })),
            field: Ident { name: "name".into() },
        };
        assert!(matches!(expr, Expr::FieldAccess { field, .. } if field.name == "name"));
    }

    #[test]
    fn test_expr_index_access() {
        let expr = Expr::IndexAccess {
            target: Box::new(Expr::Var(Ident { name: "items".into() })),
            index: Box::new(Expr::Lit(Literal::Int(0))),
        };
        assert!(matches!(expr, Expr::IndexAccess { .. }));
    }

    #[test]
    fn test_expr_struct() {
        let expr = Expr::Struct {
            fields: vec![
                (Ident { name: "name".into() }, Box::new(Expr::Lit(Literal::StringVal("test".into())))),
            ],
        };
        assert!(matches!(expr, Expr::Struct { fields } if fields.len() == 1));
    }

    #[test]
    fn test_expr_choose() {
        let expr = Expr::Choose {
            variable: Ident { name: "m".into() },
            ty: Type::Resource(Ident { name: "Machine".into() }),
            from_set: None,
        };
        assert!(matches!(expr, Expr::Choose { variable, .. } if variable.name == "m"));
    }

    #[test]
    fn test_expr_template() {
        let expr = Expr::Template("hello".into());
        assert!(matches!(expr, Expr::Template(s) if s == "hello"));
    }

    #[test]
    fn test_binop_values() {
        assert!(matches!(BinOp::Eq, BinOp::Eq));
        assert!(matches!(BinOp::Neq, BinOp::Neq));
        assert!(matches!(BinOp::Lt, BinOp::Lt));
        assert!(matches!(BinOp::Le, BinOp::Le));
        assert!(matches!(BinOp::Gt, BinOp::Gt));
        assert!(matches!(BinOp::Ge, BinOp::Ge));
        assert!(matches!(BinOp::And, BinOp::And));
        assert!(matches!(BinOp::Or, BinOp::Or));
        assert!(matches!(BinOp::Plus, BinOp::Plus));
        assert!(matches!(BinOp::Minus, BinOp::Minus));
        assert!(matches!(BinOp::Mul, BinOp::Mul));
        assert!(matches!(BinOp::Div, BinOp::Div));
    }

    #[test]
    fn test_type_list() {
        let t = Type::List(Box::new(Type::Primitive(PrimitiveType::JSON)));
        assert!(matches!(t, Type::List(inner) if matches!(*inner, Type::Primitive(PrimitiveType::JSON))));
    }

    #[test]
    fn test_type_mut_list() {
        let t = Type::MutList(Box::new(Type::Primitive(PrimitiveType::Int)));
        assert!(matches!(t, Type::MutList(_)));
    }

    #[test]
    fn test_type_map() {
        let t = Type::Map(
            Box::new(Type::Primitive(PrimitiveType::String)),
            Box::new(Type::Primitive(PrimitiveType::Int)),
        );
        assert!(matches!(t, Type::Map(_, _)));
    }

    #[test]
    fn test_type_ordered_map() {
        let t = Type::OrderedMap(
            Box::new(Type::Primitive(PrimitiveType::Int)),
            Box::new(Type::Primitive(PrimitiveType::JSON)),
        );
        assert!(matches!(t, Type::OrderedMap(_, _)));
    }

    #[test]
    fn test_type_set() {
        let t = Type::Set(Box::new(Type::Primitive(PrimitiveType::JSON)));
        assert!(matches!(t, Type::Set(_)));
    }

    #[test]
    fn test_type_ordered_set() {
        let t = Type::OrderedSet(Box::new(Type::Primitive(PrimitiveType::Bool)));
        assert!(matches!(t, Type::OrderedSet(_)));
    }

    #[test]
    fn test_type_sized_list() {
        let t = Type::SizedList(Box::new(Type::Primitive(PrimitiveType::Int)), 10);
        assert!(matches!(t, Type::SizedList(_, 10)));
    }

    #[test]
    fn test_type_resource() {
        let t = Type::Resource(Ident { name: "Server".into() });
        assert!(matches!(t, Type::Resource(name) if name.name == "Server"));
    }

    #[test]
    fn test_bytes_lit_display_all() {
        assert_eq!(format!("{}", BytesLit { value: 1024, suffix: BytesSuffix::KiB }), "1024KiB");
        assert_eq!(format!("{}", BytesLit { value: 500, suffix: BytesSuffix::None }), "500");
        assert_eq!(format!("{}", BytesLit { value: 256, suffix: BytesSuffix::MB }), "256MB");
        assert_eq!(format!("{}", BytesLit { value: 1, suffix: BytesSuffix::GiB }), "1GiB");
    }

    #[test]
    fn test_ident_hash() {
        use std::collections::HashSet;
        let id1 = Ident { name: "x".into() };
        let id2 = Ident { name: "x".into() };
        let mut set = HashSet::new();
        set.insert(id1);
        assert!(set.contains(&id2));
    }

    #[test]
    fn test_literal_debug() {
        assert!(format!("{:?}", Literal::Bool(true)).contains("Bool"));
        assert!(format!("{:?}", Literal::Int(-42)).contains("Int"));
        assert!(format!("{:?}", Literal::StringVal("hi".into())).contains("StringVal"));
    }

    #[test]
    fn test_bytes_suffix_variants() {
        assert!(matches!(BytesSuffix::None, BytesSuffix::None));
        assert!(matches!(BytesSuffix::KB, BytesSuffix::KB));
        assert!(matches!(BytesSuffix::KiB, BytesSuffix::KiB));
        assert!(matches!(BytesSuffix::MB, BytesSuffix::MB));
        assert!(matches!(BytesSuffix::MiB, BytesSuffix::MiB));
        assert!(matches!(BytesSuffix::GB, BytesSuffix::GB));
        assert!(matches!(BytesSuffix::GiB, BytesSuffix::GiB));
        assert!(matches!(BytesSuffix::TB, BytesSuffix::TB));
        assert!(matches!(BytesSuffix::TiB, BytesSuffix::TiB));
    }
}
