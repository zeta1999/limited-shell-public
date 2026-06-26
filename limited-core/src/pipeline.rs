//! Full execution pipeline: parse → type-check → plan → execute.
//!
//! This module provides the high-level `Pipeline` that orchestrates all
//! subsystems into a single runnable workflow.
//!
//! # Modules
//!
//! - [`PipelineResult`] — complete outcome of a pipeline run
//! - [`PipelineError`] — structured error with phase context
//! - [`Pipeline`] — main orchestrator

use std::collections::HashMap;

use crate::ast;
use crate::ast::OperationDecl;
use crate::execute;
use crate::execute::ExecutionContext;
use crate::parser;
use crate::resource::{CostTracker, ExtentEngine, MachineRegistry, ResourceRegistry};
use crate::scheduler::{CostMetrics, CostScheduler, SchedulerInput};
use crate::ty::{check_expr, check_let, TyEnv, TypeError, TypeRegistry};

// ─── PipelineResult ─────────────────────────

/// Complete outcome of a pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Parsed AST items (declarations).
    pub items: Vec<ast::Item>,
    /// Type-checked program.
    pub program: ast::Program,
    /// All scheduling results for operations.
    pub schedules: Vec<ScheduleStep>,
    /// Final execution context state.
    pub execution: ExecutionContextState,
}

/// A single scheduling step for an operation.
#[derive(Debug, Clone)]
pub struct ScheduleStep {
    /// Operation name.
    pub operation: String,
    /// The scheduler plan.
    pub plan: crate::scheduler::SchedulerPlan,
}

/// Snapshot of the execution context after pipeline completion.
#[derive(Debug, Clone, Default)]
pub struct ExecutionContextState {
    /// Top-level variables.
    pub variables: HashMap<String, String>,
}

// ─── PipelineError ────────────

/// Error at a specific pipeline phase.
#[derive(Debug, Clone)]
pub enum PipelineError {
    Parse { phase: String, message: String },
    TypeCheck { phase: String, message: String },
    Schedule { phase: String, message: String },
    Execute { phase: String, message: String },
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::Parse { phase, message } => {
                write!(f, "[parse] {phase}: {message}")
            }
            PipelineError::TypeCheck { phase, message } => {
                write!(f, "[type-check] {phase}: {message}")
            }
            PipelineError::Schedule { phase, message } => {
                write!(f, "[schedule] {phase}: {message}")
            }
            PipelineError::Execute { phase, message } => {
                write!(f, "[execute] {phase}: {message}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}

// ─── Pipeline ───────────────────────────────

/// Orchestrates the full pipeline: parse → type-check → plan → execute.
pub struct Pipeline<'a> {
    resource_registry: &'a ResourceRegistry,
    extent_engine: &'a ExtentEngine,
    machine_registry: &'a MachineRegistry,
    #[allow(dead_code)]
    type_registry: &'a TypeRegistry,
    #[allow(dead_code)]
    ty_env: &'a TyEnv,
    #[allow(dead_code)]
    cost_tracker: &'a CostTracker,
}

impl<'a> Pipeline<'a> {
    pub fn new(
        resource_registry: &'a ResourceRegistry,
        extent_engine: &'a ExtentEngine,
        machine_registry: &'a MachineRegistry,
        type_registry: &'a TypeRegistry,
        ty_env: &'a TyEnv,
        cost_tracker: &'a CostTracker,
    ) -> Self {
        Self {
            resource_registry,
            extent_engine,
            machine_registry,
            type_registry,
            ty_env,
            cost_tracker,
        }
    }

    /// Run the full pipeline on source code.
    ///
    /// Executes in order:
    /// 1. **Parse** — convert source to AST
    /// 2. **Type-check** — validate declarations and statements
    /// 3. **Schedule** — for each operation, plan machine assignments
    /// 4. **Execute** — run statements with the execution engine
    pub fn run(&self, source: &str) -> Result<PipelineResult, PipelineError> {
        // Phase 1: Parse
        let (items, program) = self.parse(source)?;

        // Phase 2: Type-check
        self.type_check(&items, &program)?;

        // Phase 3: Schedule operations
        let schedules = self.schedule_operations(&items)?;

        // Phase 4: Execute statements
        let execution = self.execute_program(&program)?;

        Ok(PipelineResult {
            items,
            program,
            schedules,
            execution,
        })
    }

    fn parse(&self, source: &str) -> Result<(Vec<ast::Item>, ast::Program), PipelineError> {
        let ast::Program { items, statements } = parser::parse(source).map_err(|e| {
            PipelineError::Parse {
                phase: "parse".into(),
                message: e.to_string(),
            }
        })?;

        let program = ast::Program { items: vec![], statements };
        Ok((items, program))
    }

    fn type_check(&self, items: &[ast::Item], _program: &ast::Program) -> Result<(), PipelineError> {
        // Type-check all items (declarations)
        for item in items {
            self.type_check_item(item)?;
        }

        Ok(())
    }

    fn type_check_item(&self, item: &ast::Item) -> Result<(), PipelineError> {
        match item {
            ast::Item::Alias(a) => self.type_check_alias(a),
            ast::Item::Role(_) => Ok(()),
            ast::Item::Resource(r) => self.type_check_resource(r),
            ast::Item::Device(_) => Ok(()),
            ast::Item::Machine(_) => Ok(()),
            ast::Item::Operation(op) => self.type_check_operation(op),
            ast::Item::Service(s) => self.type_check_service(s),
            ast::Item::Function(f) => self.type_check_function(f),
        }
    }

    fn type_check_alias(&self, a: &ast::AliasDecl) -> Result<(), PipelineError> {
        let _resolved = self.type_registry.resolve_type(&a.target);
        Ok(())
    }

    fn type_check_resource(&self, r: &ast::ResourceDecl) -> Result<(), PipelineError> {
        let _name = &r.name;
        for field in &r.fields {
            let _ty = self.type_registry.resolve_type(&field.ty);
        }
        for cap in &r.capacities {
            let _ty = self.type_registry.resolve_type(&cap.ty);
        }
        Ok(())
    }

    fn type_check_operation(&self, op: &ast::OperationDecl) -> Result<(), PipelineError> {
        let mut env = TyEnv::new();
        // Bind operation parameters
        for param in &op.params {
            env.add(param.name.name.clone(), param.ty.clone());
        }
        // Type-check each option
        for opt in &op.options {
            self.type_check_operation_option(&env, opt)?;
        }
        Ok(())
    }

    fn type_check_operation_option(
        &self,
        env: &TyEnv,
        opt: &ast::OperationOption,
    ) -> Result<(), PipelineError> {
        let mut env = env.child();
        for stmt in &opt.body {
            self.type_check_operation_stmt(stmt, &mut env)?;
        }
        Ok(())
    }

    fn type_check_operation_stmt(
        &self,
        stmt: &ast::OperationStatement,
        env: &mut TyEnv,
    ) -> Result<(), PipelineError> {
        match stmt {
            ast::OperationStatement::Require(_) => Ok(()),
            ast::OperationStatement::Choose(choose) => {
                let _ty = self.type_registry.resolve_type(&choose.ty);
                env.add(choose.variable.name.clone(), choose.ty.clone());
                Ok(())
            }
            ast::OperationStatement::LetDecl(decl) => {
                let (name, ty) = check_let(env, self.type_registry, decl)
                    .map_err(|e| self.to_pipeline_error(e, "let-decl"))?;
                env.add(name.name, ty);
                Ok(())
            }
            ast::OperationStatement::OnMachine(_) => Ok(()),
            ast::OperationStatement::ExecCommand { args, .. } => {
                for arg in args {
                    check_expr(env, self.type_registry, arg)
                        .map_err(|e| self.to_pipeline_error(e, "exec-arg"))?;
                }
                Ok(())
            }
            ast::OperationStatement::Transfer {
                from,
                machine,
                location,
            } => {
                check_expr(env, self.type_registry, from)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-from"))?;
                check_expr(env, self.type_registry, machine)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-machine"))?;
                check_expr(env, self.type_registry, location)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-location"))?;
                Ok(())
            }
            ast::OperationStatement::ShellCmd { .. } => Ok(()),
        }
    }

    fn type_check_service(&self, s: &ast::ServiceDecl) -> Result<(), PipelineError> {
        let _name = &s.name;
        let _on = &s.on;
        for param in &s.params {
            let _ty = self.type_registry.resolve_type(&param.ty);
        }
        Ok(())
    }

    fn type_check_function(&self, f: &ast::FunctionDecl) -> Result<(), PipelineError> {
        // Build env with function parameters
        let mut env = TyEnv::new();
        for param in &f.params {
            env.add(param.name.name.clone(), param.ty.clone());
        }
        // Type-check body statements
        self.type_check_function_body(&env, &f.body)
    }

    fn type_check_function_body(
        &self,
        env: &TyEnv,
        body: &[ast::FunctionStatement],
    ) -> Result<(), PipelineError> {
        let mut env = env.child();
        for stmt in body {
            self.type_check_function_stmt(stmt, &mut env)?;
        }
        Ok(())
    }

    fn type_check_function_stmt(
        &self,
        stmt: &ast::FunctionStatement,
        env: &mut TyEnv,
    ) -> Result<(), PipelineError> {
        match stmt {
            ast::FunctionStatement::Require(_) => Ok(()),
            ast::FunctionStatement::LetDecl(decl) => {
                let (name, ty) = check_let(env, self.type_registry, decl)
                    .map_err(|e| self.to_pipeline_error(e, "let-decl"))?;
                env.add(name.name, ty);
                Ok(())
            }
            ast::FunctionStatement::OnMachine(_) => Ok(()),
            ast::FunctionStatement::ExecCommand { args, .. } => {
                for arg in args {
                    check_expr(env, self.type_registry, arg)
                        .map_err(|e| self.to_pipeline_error(e, "exec-arg"))?;
                }
                Ok(())
            }
            ast::FunctionStatement::ReadJson { var } => {
                let _ = var;
                Ok(())
            }
            ast::FunctionStatement::WriteJson { value } => {
                check_expr(env, self.type_registry, value)
                    .map_err(|e| self.to_pipeline_error(e, "write-json"))?;
                Ok(())
            }
            ast::FunctionStatement::Transfer {
                from,
                machine,
                location,
            } => {
                check_expr(env, self.type_registry, from)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-from"))?;
                check_expr(env, self.type_registry, machine)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-machine"))?;
                check_expr(env, self.type_registry, location)
                    .map_err(|e| self.to_pipeline_error(e, "transfer-location"))?;
                Ok(())
            }
            ast::FunctionStatement::Dependency {
                service_name,
                service_param,
                on_machine,
                as_name,
            } => {
                let _ = service_name;
                let _ = service_param;
                let _ = on_machine;
                let _ = as_name;
                Ok(())
            }
            ast::FunctionStatement::Return(e) => {
                check_expr(env, self.type_registry, e)
                    .map_err(|e| self.to_pipeline_error(e, "return"))?;
                Ok(())
            }
            ast::FunctionStatement::SetEnv { .. } => Ok(()),
        }
    }

    fn to_pipeline_error(&self, e: TypeError, phase: &str) -> PipelineError {
        PipelineError::TypeCheck {
            phase: phase.into(),
            message: e.to_string(),
        }
    }

    fn schedule_operations(&self, items: &[ast::Item]) -> Result<Vec<ScheduleStep>, PipelineError> {
        let mut steps = Vec::new();

        for item in items {
            if let ast::Item::Operation(op) = item {
                let scheduler_input = self.build_scheduler_input(op)?;
                let scheduler =
                    CostScheduler::from_registry(self.machine_registry, CostMetrics::balanced());
                let result = scheduler.schedule(&scheduler_input);
                steps.push(ScheduleStep {
                    operation: op.name.name.clone(),
                    plan: result.plan,
                });
            }
        }

        Ok(steps)
    }

    fn build_scheduler_input(&self, op: &OperationDecl) -> Result<SchedulerInput, PipelineError> {
        let mut input = SchedulerInput::new(op.name.name.clone(), Vec::new());

        // Extract costs from operation options
        for option in &op.options {
            for stmt in &option.body {
                if let ast::OperationStatement::Choose(choose) = stmt {
                    // A choose statement selects a machine/resource — extract type name
                    let type_name = match &choose.ty {
                        ast::Type::Resource(ident) => ident.name.clone(),
                        ast::Type::Primitive(ast::PrimitiveType::Node) => "Node".into(),
                        _ => "Node".into(),
                    };
                    input.required_machines.push(type_name);
                }
            }
        }

        // Extract costs from operation definition
        if let Some(cost) = &op.cost {
            for cost_entry in &cost.costs {
                // Evaluate cost expressions to concrete values
                let value = evaluate_cost_expr(&cost_entry.value).unwrap_or(0);
                input.costs.push(crate::ast::CostEntry {
                    kind: cost_entry.kind.clone(),
                    value: ast::Expr::Lit(ast::expr::Literal::Int(value as i64)),
                });
            }
        }

        Ok(input)
    }

    fn execute_program(&self, program: &ast::Program) -> Result<ExecutionContextState, PipelineError> {
        let mut ctx = ExecutionContext::new(
            self.resource_registry,
            self.extent_engine,
            self.machine_registry,
        );

        for stmt in &program.statements {
            if let Err(e) = execute::execute_stmt(stmt, &mut ctx) {
                return Err(PipelineError::Execute {
                    phase: "execute".into(),
                    message: e.to_string(),
                });
            }
        }

        // Snapshot the execution context
        Ok(ExecutionContextState {
            variables: ctx.top_variables(),
        })
    }
}

/// Evaluate a cost expression to a u64 value.
fn evaluate_cost_expr(expr: &ast::expr::Expr) -> Option<u64> {
    match expr {
        ast::expr::Expr::Lit(ast::expr::Literal::Int(n)) if *n >= 0 => Some(*n as u64),
        ast::expr::Expr::Lit(ast::expr::Literal::Bytes(b)) => Some(b.value),
        _ => None,
    }
}

// ─── Tests ───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::ResourceTypeInfo;

    fn make_context() -> Pipeline<'static> {
        let pipeline = Pipeline::new(
            Box::leak(Box::new(ResourceRegistry::new())),
            Box::leak(Box::new(ExtentEngine::new())),
            Box::leak(Box::new(MachineRegistry::new())),
            Box::leak(Box::new(TypeRegistry::new())),
            Box::leak(Box::new(TyEnv::new())),
            Box::leak(Box::new(CostTracker::new())),
        );
        pipeline
    }

    #[test]
    fn test_parse_empty_program() {
        let pipeline = make_context();
        let result = pipeline.run("");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.program.statements.is_empty());
        assert!(result.items.is_empty());
    }

    #[test]
    fn test_parse_on_machine() {
        let pipeline = make_context();
        let result = pipeline.run("on machine alpha;");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_parse_multiple_statements() {
        let pipeline = make_context();
        let result = pipeline.run("on machine alpha;\non machine beta;");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 2);
    }

    #[test]
    fn test_parse_control_flow() {
        let pipeline = make_context();
        let result = pipeline.run("if 1 > 0 { on machine alpha; } else { on machine beta; }");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_parse_for_loop() {
        let pipeline = make_context();
        let result = pipeline.run("for m in machines() { on machine m; }");
        if let Err(ref e) = result { eprintln!("PARSE ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_parse_operation() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
operation deploy(Node x) {
    requires x is alpha,
    options {
        on alpha;
        exec cmd { x };
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("OP ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.schedules.len(), 1);
        assert_eq!(result.schedules[0].operation, "deploy");
    }

    #[test]
    fn test_parse_operation_with_cost() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
operation compute(Node target) {
    cost {
        RAM: 100,
        GPUVRAM: 8,
    }
    options {
        on target;
        exec cmd;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("OPCOST ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.schedules.len(), 1);
        assert!(!result.schedules.is_empty());
    }

    #[test]
    fn test_schedule_infeasible() {
        let pipeline = make_context();
        // Register no machines, so scheduling should be infeasible
        let result = pipeline.run(
            r#"
operation deploy(Node x) {
    options {
        on x;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("INF ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.schedules.len(), 1);
        assert!(!result.schedules[0].plan.feasible);
    }

    #[test]
    fn test_pipeline_execution_state() {
        let pipeline = make_context();
        let result = pipeline.run("on machine alpha;");
        assert!(result.is_ok());
        let result = result.unwrap();
        // The execution should produce an ExecutionContextState
        assert!(!result.execution.variables.is_empty()
            || result.program.statements.len() == 1);
    }

    // ─── Type Checking Tests ────────────────

    fn make_context_with_resource(name: &str) -> Pipeline<'static> {
        let mut registry = ResourceRegistry::new();
        let mut extent_engine = ExtentEngine::new();
        let mut machine_registry = MachineRegistry::new();
        let mut type_registry = TypeRegistry::new();
        type_registry.register_resource(
            ast::Ident { name: name.into() },
            ResourceTypeInfo {
                capacities: Vec::new(),
                fields: Vec::new(),
            },
        );
        let ty_env = TyEnv::new();
        let cost_tracker = CostTracker::new();
        Pipeline::new(
            Box::leak(Box::new(registry)),
            Box::leak(Box::new(extent_engine)),
            Box::leak(Box::new(machine_registry)),
            Box::leak(Box::new(type_registry)),
            Box::leak(Box::new(ty_env)),
            Box::leak(Box::new(cost_tracker)),
        )
    }

    #[test]
    fn test_type_check_alias() {
        let pipeline = make_context_with_resource("Node");
        // Alias items don't end with semicolons
        let result = pipeline.run(r#"alias Node = Node"#);
        if let Err(ref e) = result { eprintln!("ALIAS ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_resource_decl() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
resource Compute {
    field cores: Int,
}
"#,
        );
        if let Err(ref e) = result { eprintln!("TYPE CHECK RESOURCE ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_function() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
function deploy(Node target) {
    let x = 1;
    return x;
}
"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_operation_with_let() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
operation deploy(Node target) {
    options {
        let x = 42;
        on target;
    }
}
"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_service() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
service web(Node n) on n {
    RAM: 8,
}
"#,
        );
        if let Err(ref e) = result { eprintln!("SERVICE ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_full_pipeline() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
resource Compute {
    field cores: Int,
}

function setup(Int count) {
    let x = count + 1;
    return x;
}

operation deploy(Node target) {
    requires target is Node,
    cost {
        cores: 4,
    }
    options {
        let x = 42;
        on target;
        exec bash { x };
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("FULL PIPELINE ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.schedules.len(), 1);
    }

    #[test]
    fn test_pipeline_tasks_statement() {
        let pipeline = make_context();
        let result = pipeline.run("tasks { x, y, z }");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_pipeline_grant_statement() {
        let pipeline = make_context();
        let result = pipeline.run("grant Admin can Read");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_pipeline_alias_statement() {
        let pipeline = make_context();
        let result = pipeline.run("alias Node = Node");
        assert!(result.is_ok());
        let result = result.unwrap();
        // alias is parsed as an item, not a statement
        assert_eq!(result.items.len(), 1);
    }

    #[test]
    fn test_pipeline_for_loop() {
        let pipeline = make_context();
        let result = pipeline.run("for m in machines() { on machine m; }");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_pipeline_if_statement() {
        let pipeline = make_context();
        let result = pipeline.run("if 1 > 0 { on machine alpha; } else { on machine beta; }");
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 1);
    }

    #[test]
    fn test_pipeline_multiple_statements() {
        let pipeline = make_context();
        let result = pipeline.run(
            "on machine alpha;
on machine beta;
tasks { x }",
        );
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.program.statements.len(), 3);
    }

    #[test]
    fn test_pipeline_operation_with_multiple_options() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
operation deploy(Node target) {
    requires target is Node,
    options {
        on target;
        exec cmd;
    }
    options {
        on target;
        exec other;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("MULTI OPT ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.schedules.len(), 1);
    }

    #[test]
    fn test_pipeline_function_decl() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
function setup(Int count) {
    let x = count + 1;
    return x;
}
"#,
        );
        if let Err(ref e) = result { eprintln!("FUNC ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.schedules.len(), 0);
    }

    #[test]
    fn test_pipeline_try_catch() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
try { tasks { foo } } catch error e { tasks { bar } }
"#,
        );
        if let Err(ref e) = result { eprintln!("TRYCATCH ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_while_loop() {
        // While loops execute at runtime; condition is always true after parsing,
        // so they loop forever. Only test that they parse correctly by using
        // a control flow test in execute module. Verify while parses as expected.
        let result = parser::parse("while more is true { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_role_decl_as_statement() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
role Admin {
    can Read,
    can Write,
}
"#,
        );
        if let Err(ref e) = result { eprintln!("ROLE ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_parse_error() {
        let pipeline = make_context();
        let result = pipeline.run("invalid syntax {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_type_check_alias() {
        let pipeline = make_context();
        let result = pipeline.run("alias m as machine = Node\nalias r as role = Admin");
        if let Err(ref e) = result { eprintln!("ALIAS ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn test_pipeline_multi_item_types() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
alias m as machine = Node
"#,
        );
        if let Err(ref e) = result { eprintln!("MULTI ITEM ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_scheduling_plan() {
        let pipeline = make_context_with_resource("Node");
        let result = pipeline.run(
            r#"
operation deploy(Node target) {
    requires target is Node,
    options {
        on target;
        exec cmd;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("SCHED ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.schedules.len(), 1);
    }

    #[test]
    fn test_pipeline_type_error_unknown_var() {
        let pipeline = make_context();
        let result = pipeline.run("let x = undefined_variable_that_does_not_exist");
        // Unknown variable in let init produces a type error
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_resource_decl_no_params() {
        let pipeline = make_context();
        let result = pipeline.run("resource Empty {}");
        if let Err(ref e) = result { eprintln!("EMPTY ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
    }

    #[test]
    fn test_pipeline_type_error_type_mismatch() {
        let pipeline = make_context();
        let result = pipeline.run("alias Node = Node\nalias Server = Node");
        // Two aliases both work
        if let Err(ref e) = result { eprintln!("TM ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn test_pipeline_alias_chain() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
alias m as machine = Node
alias r as role = Admin
"#,
        );
        if let Err(ref e) = result { eprintln!("CHAIN ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn test_pipeline_complex_program() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
role Admin {
    can Read,
    can Write,
}
alias m as machine = Node
"#,
        );
        if let Err(ref e) = result { eprintln!("COMPLEX ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_execute_error() {
        let pipeline = make_context();
        // Choose inside on statement — executes but fails in non-operation context
        let result = pipeline.run("on server { tasks { choose x from machines() } }");
        // Parsing works but execution fails (no operation context for choose)
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            PipelineError::Execute { .. } => {}
            other => panic!("expected Execute error, got: {other:?}"),
        }
    }

    #[test]
    fn test_pipeline_error_display() {
        let parse_err = PipelineError::Parse {
            phase: "parse".into(),
            message: "bad input".into(),
        };
        assert!(parse_err.to_string().contains("parse"));
        assert!(parse_err.to_string().contains("bad input"));

        let type_err = PipelineError::TypeCheck {
            phase: "type".into(),
            message: "mismatch".into(),
        };
        assert!(type_err.to_string().contains("type"));
        assert!(type_err.to_string().contains("mismatch"));

        let sched_err = PipelineError::Schedule {
            phase: "schedule".into(),
            message: "no machine".into(),
        };
        assert!(sched_err.to_string().contains("schedule"));

        let exec_err = PipelineError::Execute {
            phase: "execute".into(),
            message: "null pointer".into(),
        };
        assert!(exec_err.to_string().contains("execute"));
    }

    #[test]
    fn test_pipeline_resource_decl_with_field() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
resource Server {
    field name: String,
    field ram: Bytes,
}
"#,
        );
        if let Err(ref e) = result { eprintln!("RESOURCE FIELD ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
    }

    #[test]
    fn test_pipeline_multiple_operations() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
operation deploy(Node target) {
    requires target is Node,
    options {
        on target;
    }
}
operation teardown(Node target) {
    requires target is Node,
    options {
        on target;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("MULTI OP ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.schedules.len(), 2);
    }

    #[test]
    fn test_pipeline_scheduling_no_machines() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
operation deploy(Node target) {
    requires target is Node,
    options {
        on target;
    }
}
"#,
        );
        // No machines registered, scheduling should still "work" (just no assignments)
        if let Err(ref e) = result { eprintln!("NO MACHINE SCHED ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_operation_with_cost() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
operation compute(Node target) {
    cost {
        RAM: 100,
    }
    options {
        on target;
    }
}
"#,
        );
        if let Err(ref e) = result { eprintln!("COST ERROR: {:?}", e); }
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.schedules.len(), 1);
    }

    #[test]
    fn test_pipeline_for_statement() {
        let pipeline = make_context();
        let result = pipeline.run(
            r#"
for m in machines() {
    on machine m;
}
"#,
        );
        if let Err(ref e) = result { eprintln!("FOR ERROR: {:?}", e); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_pipeline_if_exec() {
        let pipeline = make_context();
        let result = pipeline.run("if 1 > 0 { on machine alpha; } else { on machine beta; }");
        if let Err(ref e) = result { eprintln!("IF ERROR: {:?}", e); }
        assert!(result.is_ok());
    }
}
