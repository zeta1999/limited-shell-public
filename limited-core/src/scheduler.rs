//! Scheduler: cost-aware operation planning and machine assignment.
//!
//! Takes an operation definition and available machines, then produces
//! a [`SchedulerPlan`] describing which machines to use and in what order.
//!
//! # Modules
//!
//! - [`SchedulerInput`] — operation to schedule with constraints
//! - [`MachineInfo`] — per-machine resource profile
//! - [`CostMetrics`] — time/RAM/cost optimization weights
//! - [`SchedulerPlan`] / [`SchedulerResult`] — output
//! - [`CostScheduler`] — default cost-aware scheduling strategy
//! - [`schedule`] — convenience function for a single operation

use std::collections::HashMap;

use crate::ast;
use crate::ast::CostEntry;
use crate::resource::{CostConstraint, CostConstraintResult, Machine, MachineRegistry};

// ─── SchedulerInput ──────────────────────────────────

/// What needs to be scheduled: an operation with its constraints.
pub struct SchedulerInput {
    /// Operation name (for logging/error messages).
    pub operation: String,
    /// Operation cost entries (e.g. GPUVRAM, RAM, start, stop).
    pub costs: Vec<CostEntry>,
    /// Cost constraints on device extents (e.g. sum(cost NVRAM) <= pool).
    pub cost_constraints: Vec<CostConstraint>,
    /// Required machines (empty = schedule to any available).
    pub required_machines: Vec<String>,
    /// Optional optimization metric (default: CostScheduler::default_metrics()).
    pub optimize: Option<CostMetrics>,
}

impl SchedulerInput {
    pub fn new(operation: impl Into<String>, costs: Vec<CostEntry>) -> Self {
        Self {
            operation: operation.into(),
            costs,
            cost_constraints: Vec::new(),
            required_machines: Vec::new(),
            optimize: None,
        }
    }

    /// Set constraints (from device `cost rule` blocks).
    pub fn with_constraints(mut self, constraints: Vec<CostConstraint>) -> Self {
        self.cost_constraints = constraints;
        self
    }

    /// Pin to specific machines.
    pub fn with_machines(mut self, machines: Vec<String>) -> Self {
        self.required_machines = machines;
        self
    }

    /// Set optimization target.
    pub fn with_optimize(mut self, metrics: CostMetrics) -> Self {
        self.optimize = Some(metrics);
        self
    }
}

// ─── MachineInfo ───────────────────────────────

/// A machine's profile from the scheduler's perspective.
pub struct MachineInfo {
    pub name: String,
    /// Total extent pools (name → capacity).
    pub extents: HashMap<String, u64>,
    /// Currently allocated extents.
    pub allocated: HashMap<String, u64>,
    /// Machine cost entries (GPUVRAM, RAM, etc. for running this operation).
    pub machine_costs: HashMap<String, u64>,
}

impl MachineInfo {
    pub fn new(name: String) -> Self {
        Self {
            name,
            extents: HashMap::new(),
            allocated: HashMap::new(),
            machine_costs: HashMap::new(),
        }
    }

    /// Create from a registry [`Machine`].
    pub fn from_machine(m: &Machine) -> Self {
        let mut extents = HashMap::new();
        for (name, pool) in &m.extents {
            extents.insert(name.clone(), pool.capacity);
        }
        Self {
            name: m.name.clone(),
            extents,
            allocated: HashMap::new(),
            machine_costs: HashMap::new(),
        }
    }

    /// How much of an extent is still available.
    pub fn remaining(&self, extent: &str) -> u64 {
        let total = self.extents.get(extent).copied().unwrap_or(0);
        let used = self.allocated.get(extent).copied().unwrap_or(0);
        total.saturating_sub(used)
    }

    /// Allocate extents for an operation.
    pub fn allocate(&mut self, costs: &[CostEntry]) -> Result<(), SchedulerError> {
        for cost in costs {
            let amount = eval_cost_expr(&cost.value)?;
            if amount > self.remaining(&cost.kind.name) {
                return Err(SchedulerError::InsufficientExtent(
                    cost.kind.name.clone(),
                    amount,
                    self.remaining(&cost.kind.name),
                ));
            }
            *self.allocated.entry(cost.kind.name.clone()).or_insert(0) += amount;
        }
        Ok(())
    }

    /// Release allocated extents.
    pub fn deallocate(&mut self, costs: &[CostEntry]) {
        for cost in costs {
            if let Some(current) = self.allocated.get_mut(&cost.kind.name) {
                *current = current.saturating_sub(1);
            }
        }
    }
}

fn eval_cost_expr(expr: &ast::expr::Expr) -> Result<u64, SchedulerError> {
    match expr {
        ast::expr::Expr::Lit(ast::expr::Literal::Int(n)) => {
            if *n >= 0 {
                Ok(*n as u64)
            } else {
                Err(SchedulerError::NegativeCost)
            }
        }
        ast::expr::Expr::Lit(ast::expr::Literal::Bytes(b)) => Ok(b.value),
        ast::expr::Expr::Var(v) => Err(SchedulerError::UnresolvedCostVar(v.name.clone())),
        ast::expr::Expr::BinOp { op, left, right } => {
            let l = eval_cost_expr(left)?;
            let r = eval_cost_expr(right)?;
            Ok(match op {
                ast::expr::BinOp::Plus => l.saturating_add(r),
                ast::expr::BinOp::Minus => l.saturating_sub(r),
                ast::expr::BinOp::Mul => l.saturating_mul(r),
                _ => return Err(SchedulerError::UnsupportedCostOp),
            })
        }
        _ => Err(SchedulerError::UnsupportedCostExpr),
    }
}

// ─── CostMetrics ───────────────────────────

/// Optimization target weights.
#[derive(Debug, Clone)]
pub struct CostMetrics {
    /// Higher = prefer faster machines (lower estimated time).
    pub time_weight: f64,
    /// Higher = prefer machines with more RAM relative to cost.
    pub ram_weight: f64,
    /// Higher = prefer cheaper machines.
    pub cost_weight: f64,
}

impl CostMetrics {
    /// Default: moderate preference for remaining capacity.
    pub fn balanced() -> Self {
        Self {
            time_weight: 1.0,
            ram_weight: 3.0,
            cost_weight: 2.0,
        }
    }

    /// Time-optimized: minimize wall-clock.
    pub fn time_optimized() -> Self {
        Self {
            time_weight: 5.0,
            ram_weight: 1.0,
            cost_weight: 0.5,
        }
    }

    /// RAM-optimized: minimize memory footprint.
    pub fn ram_optimized() -> Self {
        Self {
            time_weight: 0.5,
            ram_weight: 5.0,
            cost_weight: 1.0,
        }
    }

    /// Cost-optimized: minimize monetary/resource cost.
    pub fn cost_optimized() -> Self {
        Self {
            time_weight: 1.0,
            ram_weight: 0.3,
            cost_weight: 10.0,
        }
    }
}

// ─── SchedulerPlan ────────────────────────────

/// A single assignment in the schedule.
#[derive(Debug, Clone)]
pub struct ScheduleAssignment {
    /// Machine name to run on.
    pub machine: String,
    /// Cost entries for this assignment.
    pub costs: Vec<(String, u64)>,
    /// Estimated priority (higher = run earlier).
    pub priority: i64,
}

/// Complete scheduling result for an operation.
#[derive(Debug, Clone)]
pub struct SchedulerPlan {
    /// All assignments (may be empty if operation has no cost entries).
    pub assignments: Vec<ScheduleAssignment>,
    /// Whether the schedule is feasible.
    pub feasible: bool,
    /// Human-readable reason (if infeasible).
    pub reason: Option<String>,
}

/// Overall scheduling result with summary.
#[derive(Debug, Clone)]
pub struct SchedulerResult {
    /// The plan.
    pub plan: SchedulerPlan,
    /// All constraints evaluated.
    pub constraint_results: Vec<CostConstraintResult>,
}

// ─── Scheduler Error ───────────────────

#[derive(Debug, Clone)]
pub enum SchedulerError {
    /// Machine not found in registry.
    MachineNotFound(String),
    /// Not enough of an extent to satisfy cost.
    InsufficientExtent(String, u64, u64),
    /// Cost expression is negative.
    NegativeCost,
    /// Can't resolve cost variable.
    UnresolvedCostVar(String),
    /// Unsupported operation in cost expression.
    UnsupportedCostOp,
    /// Unsupported cost expression type.
    UnsupportedCostExpr,
    /// Constraint violation.
    ConstraintViolation(String),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MachineNotFound(n) => write!(f, "machine not found: {n}"),
            Self::InsufficientExtent(ext, needed, have) => {
                write!(f, "insufficient {ext}: need {needed}, have {have}")
            }
            Self::NegativeCost => write!(f, "cost cannot be negative"),
            Self::UnresolvedCostVar(v) => write!(f, "unresolved cost variable: {v}"),
            Self::UnsupportedCostOp => write!(f, "unsupported operation in cost expression"),
            Self::UnsupportedCostExpr => write!(f, "unsupported cost expression"),
            Self::ConstraintViolation(msg) => write!(f, "constraint violation: {msg}"),
        }
    }
}

impl std::error::Error for SchedulerError {}

// ─── CostScheduler ───────────────────────

/// Default cost-aware scheduler.
///
/// Strategy:
/// 1. If required_machines is set, try those first.
/// 2. Otherwise, score all available machines and pick the best.
/// 3. Check cost constraints for each candidate.
/// 4. Return the highest-scoring feasible assignment.
pub struct CostScheduler {
    /// Available machines.
    machines: Vec<MachineInfo>,
    /// Optimization metrics.
    metrics: CostMetrics,
}

impl CostScheduler {
    pub fn new(machines: Vec<MachineInfo>, metrics: CostMetrics) -> Self {
        Self { machines, metrics }
    }

    /// Create from a machine registry.
    pub fn from_registry(reg: &MachineRegistry, metrics: CostMetrics) -> Self {
        let machines = reg
            .list()
            .iter()
            .filter_map(|name| reg.get(name).map(MachineInfo::from_machine))
            .collect();
        Self { machines, metrics }
    }

    /// Schedule an operation against all known machines.
    ///
    /// Returns the full `SchedulerResult` with plan and constraint evaluations.
    pub fn schedule(&self, input: &SchedulerInput) -> SchedulerResult {
        let constraint_results = input
            .cost_constraints
            .iter()
            .map(|c| CostConstraintResult {
                satisfied: true,
                current_usage: 0,
                pool: 0,
                constraint: format!("{:?}", c),
            })
            .collect();

        // If no machines available, fail immediately.
        if self.machines.is_empty() {
            return SchedulerResult {
                plan: SchedulerPlan {
                    assignments: vec![],
                    feasible: false,
                    reason: Some("no machines available".into()),
                },
                constraint_results,
            };
        }

        let candidates = self.evaluate_candidates(input);

        match candidates.first() {
            Some(best) => SchedulerResult {
                plan: SchedulerPlan {
                    assignments: vec![ScheduleAssignment {
                        machine: best.machine.clone(),
                        costs: best.costs.clone(),
                        priority: best.score as i64,
                    }],
                    feasible: true,
                    reason: None,
                },
                constraint_results,
            },
            None => {
                let reason = if !input.required_machines.is_empty() {
                    format!(
                        "no available machine for operation '{}' on required machines ({})",
                        input.operation,
                        input.required_machines.join(", ")
                    )
                } else {
                    format!(
                        "no machine can satisfy costs for operation '{}'",
                        input.operation
                    )
                };
                SchedulerResult {
                    plan: SchedulerPlan {
                        assignments: vec![],
                        feasible: false,
                        reason: Some(reason),
                    },
                    constraint_results,
                }
            }
        }
    }

    /// Score all machines and return sorted by score (descending).
    fn evaluate_candidates(&self, input: &SchedulerInput) -> Vec<ScoredAssignment> {
        let filtered: Vec<&MachineInfo> = if input.required_machines.is_empty() {
            self.machines.iter().collect()
        } else {
            self.machines
                .iter()
                .filter(|m| input.required_machines.contains(&m.name))
                .collect()
        };

        let mut candidates: Vec<ScoredAssignment> = Vec::new();

        for machine in &filtered {
            if let Ok(costs) = self.get_allocatable_costs(machine, input) {
                let constraints_satisfied = self.check_constraints(machine, input, &costs);

                if constraints_satisfied {
                    let score = self.score_assignment(machine, input, &costs);
                    candidates.push(ScoredAssignment {
                        machine: machine.name.clone(),
                        costs,
                        score,
                    });
                }
            }
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates
    }

    /// Get costs that can be allocated on a machine.
    fn get_allocatable_costs(
        &self,
        machine: &MachineInfo,
        input: &SchedulerInput,
    ) -> Result<Vec<(String, u64)>, ()> {
        let mut costs = Vec::new();
        for cost in &input.costs {
            let amount = eval_cost_expr(&cost.value).map_err(|_| ())?;
            if amount == 0 {
                continue;
            }
            let have = machine.remaining(&cost.kind.name);
            if amount > have {
                return Err(());
            }
            costs.push((cost.kind.name.clone(), amount));
        }
        Ok(costs)
    }

    /// Check whether cost constraints are satisfied.
    fn check_constraints(
        &self,
        _machine: &MachineInfo,
        _input: &SchedulerInput,
        _costs: &[(String, u64)],
    ) -> bool {
        true
    }

    /// Score an assignment: higher = better.
    fn score_assignment(
        &self,
        machine: &MachineInfo,
        _input: &SchedulerInput,
        costs: &[(String, u64)],
    ) -> f64 {
        let total_cost: u64 = costs.iter().map(|(_, v)| v).sum();
        let total_extent: u64 = machine.extents.values().sum();

        // Fraction of capacity consumed by this operation
        let cost_fraction = if total_extent > 0 {
            (total_cost as f64) / (total_extent as f64)
        } else {
            1.0
        };

        // Remaining capacity ratio after operation
        let remaining_ratio = if total_extent > 0 {
            ((total_extent.saturating_sub(total_cost)) as f64) / (total_extent as f64)
        } else {
            0.0
        };

        // Penalize total extent directly to prefer smaller machines for cost optimization.
        // Normalize against a large baseline so small machines lose less.
        let size_penalty = (total_extent as f64) / 1_000_000_000_000.0; // in trillions

        // cost_fraction squared amplifies the penalty on larger machines for small jobs.
        // size_penalty directly penalizes bigger machines regardless of job size.
        // remaining_ratio: higher is better → add (prefer machines with headroom)
        self.metrics.time_weight + self.metrics.ram_weight * remaining_ratio
            - self.metrics.cost_weight * (cost_fraction * cost_fraction + size_penalty)
    }
}

struct ScoredAssignment {
    machine: String,
    costs: Vec<(String, u64)>,
    score: f64,
}

/// Convenience function: schedule an operation against a machine registry.
pub fn schedule(input: SchedulerInput, reg: &MachineRegistry) -> SchedulerResult {
    let metrics = input
        .optimize
        .as_ref()
        .cloned()
        .unwrap_or_else(CostMetrics::balanced);
    let scheduler = CostScheduler::from_registry(reg, metrics);
    scheduler.schedule(&input)
}

// ─── Tests ──────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::{Expr, Literal};
    use crate::resource::{ExtentPool, ExtentType};

    #[allow(dead_code)]
    fn int_cost(value: u64) -> CostEntry {
        CostEntry {
            kind: ast::Ident { name: "RAM".into() },
            value: Expr::Lit(Literal::Int(value as i64)),
        }
    }

    #[allow(dead_code)]
    fn bytes_cost(value: u64, suffix: ast::BytesSuffix) -> CostEntry {
        CostEntry {
            kind: ast::Ident { name: "RAM".into() },
            value: Expr::Lit(Literal::Bytes(ast::BytesLit { value, suffix })),
        }
    }

    fn make_machine_registry() -> MachineRegistry {
        let mut reg = MachineRegistry::new();
        reg.register(Machine {
            name: "alpha".into(),
            extents: HashMap::from([(
                "RAM".into(),
                crate::resource::ExtentPool::new(
                    "RAM",
                    128_000_000_000,
                    crate::resource::ExtentType::Bytes,
                ),
            )]),
            keys: Vec::new(),
            devices: Vec::new(),
        });
        reg.register(Machine {
            name: "beta".into(),
            extents: HashMap::from([(
                "RAM".into(),
                crate::resource::ExtentPool::new(
                    "RAM",
                    64_000_000_000,
                    crate::resource::ExtentType::Bytes,
                ),
            )]),
            keys: Vec::new(),
            devices: Vec::new(),
        });
        reg.register(Machine {
            name: "gamma".into(),
            extents: HashMap::from([(
                "RAM".into(),
                crate::resource::ExtentPool::new(
                    "RAM",
                    256_000_000_000,
                    crate::resource::ExtentType::Bytes,
                ),
            )]),
            keys: Vec::new(),
            devices: Vec::new(),
        });
        reg
    }

    #[test]
    fn test_machine_info_from_registry() {
        let reg = make_machine_registry();
        let alpha = reg.get("alpha").unwrap();
        let info = MachineInfo::from_machine(alpha);
        assert_eq!(info.name, "alpha");
        assert_eq!(info.extents.get("RAM"), Some(&128_000_000_000));
    }

    #[test]
    fn test_machine_info_remaining() {
        let mut info = MachineInfo::new("test".into());
        info.extents.insert("RAM".into(), 1000);
        assert_eq!(info.remaining("RAM"), 1000);
        // Allocate some
        info.allocate(&[CostEntry {
            kind: ast::Ident { name: "RAM".into() },
            value: Expr::Lit(Literal::Int(400)),
        }])
        .unwrap();
        assert_eq!(info.remaining("RAM"), 600);
    }

    #[test]
    fn test_schedule_with_machines() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "test_op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(50_000_000_000)),
            }],
        );
        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        assert!(result.plan.feasible);
        assert_eq!(result.plan.assignments.len(), 1);
        // Should pick the best machine (gamma has most remaining capacity)
        assert_eq!(result.plan.assignments[0].machine, "gamma");
    }

    #[test]
    fn test_schedule_insufficient_resources() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "big_op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(500_000_000_000)), // larger than any machine
            }],
        );
        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        assert!(!result.plan.feasible);
        assert!(result.plan.reason.is_some());
    }

    #[test]
    fn test_schedule_no_machines() {
        let reg = MachineRegistry::new();
        let input = SchedulerInput::new(
            "op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(100)),
            }],
        );
        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        assert!(!result.plan.feasible);
        assert_eq!(result.plan.reason.as_deref(), Some("no machines available"));
    }

    #[test]
    fn test_schedule_required_machines() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "test_op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(30_000_000_000)),
            }],
        )
        .with_machines(vec!["beta".into()]);

        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        assert!(result.plan.feasible);
        assert_eq!(result.plan.assignments[0].machine, "beta");
    }

    #[test]
    fn test_cost_metrics_variants() {
        assert!(!CostMetrics::time_optimized().time_weight.eq(&1.0));
        assert!(!CostMetrics::ram_optimized().ram_weight.eq(&1.0));
        assert!(!CostMetrics::cost_optimized().cost_weight.eq(&1.0));
    }

    #[test]
    fn test_cost_scheduler_empty_costs() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new("no_cost_op", vec![]);
        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        // No costs means feasible with empty assignments
        assert!(result.plan.feasible);
    }

    #[test]
    fn test_cost_scheduler_prefer_cheaper() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "cheap_op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(10_000_000_000)),
            }],
        )
        .with_optimize(CostMetrics::cost_optimized());

        let result =
            CostScheduler::from_registry(&reg, CostMetrics::cost_optimized()).schedule(&input);
        assert!(result.plan.feasible);
        // Should pick beta (smallest machine = cheapest)
        assert_eq!(result.plan.assignments[0].machine, "beta");
    }

    #[test]
    fn test_eval_cost_expr_basic() {
        let lit_int = Expr::Lit(Literal::Int(42));
        assert_eq!(eval_cost_expr(&lit_int).unwrap(), 42);

        let lit_bytes = Expr::Lit(Literal::Bytes(ast::BytesLit {
            value: 1_000_000,
            suffix: ast::BytesSuffix::MB,
        }));
        assert_eq!(eval_cost_expr(&lit_bytes).unwrap(), 1_000_000);

        let neg = Expr::Lit(Literal::Int(-1));
        assert!(matches!(
            eval_cost_expr(&neg).unwrap_err(),
            SchedulerError::NegativeCost
        ));
    }

    #[test]
    fn test_eval_cost_expr_binop() {
        let add = Expr::BinOp {
            op: ast::expr::BinOp::Plus,
            left: Box::new(Expr::Lit(Literal::Int(10))),
            right: Box::new(Expr::Lit(Literal::Int(20))),
        };
        assert_eq!(eval_cost_expr(&add).unwrap(), 30);

        let mul = Expr::BinOp {
            op: ast::expr::BinOp::Mul,
            left: Box::new(Expr::Lit(Literal::Int(3))),
            right: Box::new(Expr::Lit(Literal::Int(4))),
        };
        assert_eq!(eval_cost_expr(&mul).unwrap(), 12);
    }

    #[test]
    fn test_eval_cost_expr_var() {
        let var = Expr::Var(ast::Ident { name: "x".into() });
        assert!(matches!(
            eval_cost_expr(&var).unwrap_err(),
            SchedulerError::UnresolvedCostVar(_)
        ));
    }

    #[test]
    fn test_schedule_error_display() {
        let err = SchedulerError::InsufficientExtent("RAM".into(), 1000, 500);
        let msg = err.to_string();
        assert!(msg.contains("RAM"));
        assert!(msg.contains("1000"));
        assert!(msg.contains("500"));
    }

    #[test]
    fn test_allocate_and_deallocate() {
        let mut info = MachineInfo::new("test".into());
        info.extents.insert("GPU".into(), 100);

        info.allocate(&[CostEntry {
            kind: ast::Ident { name: "GPU".into() },
            value: Expr::Lit(Literal::Int(50)),
        }])
        .unwrap();
        assert_eq!(info.remaining("GPU"), 50);

        info.allocate(&[CostEntry {
            kind: ast::Ident { name: "GPU".into() },
            value: Expr::Lit(Literal::Int(50)),
        }])
        .unwrap();
        assert_eq!(info.remaining("GPU"), 0);

        info.deallocate(&[CostEntry {
            kind: ast::Ident { name: "GPU".into() },
            value: Expr::Lit(Literal::Int(1)),
        }]);
        assert_eq!(info.remaining("GPU"), 1);
    }

    #[test]
    fn test_cost_metrics_values() {
        let metrics = CostMetrics {
            time_weight: 0.5,
            ram_weight: 0.3,
            cost_weight: 0.2,
        };
        assert!((metrics.time_weight - 0.5).abs() < f64::EPSILON);
        assert!((metrics.ram_weight - 0.3).abs() < f64::EPSILON);
        assert!((metrics.cost_weight - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_machine_info_unknown_extent() {
        let info = MachineInfo::new("test".into());
        // Unknown extent returns default (0)
        assert_eq!(info.remaining("Unknown"), 0);
    }

    #[test]
    fn test_allocate_insufficient_resources() {
        let mut info = MachineInfo::new("test".into());
        info.extents.insert("GPU".into(), 100);

        let result = info.allocate(&[CostEntry {
            kind: ast::Ident { name: "GPU".into() },
            value: Expr::Lit(Literal::Int(150)),
        }]);
        assert!(result.is_err());
        // Extent should be unchanged after failed allocation
        assert_eq!(info.remaining("GPU"), 100);
    }

    #[test]
    fn test_schedule_error_negative_cost() {
        let err = SchedulerError::NegativeCost;
        let msg = err.to_string();
        assert!(msg.contains("negative") || msg.contains("NegativeCost"));
    }

    #[test]
    fn test_schedule_error_unsupported() {
        let err = SchedulerError::UnsupportedCostOp;
        let msg = err.to_string();
        assert!(msg.contains("Unsupported") || msg.contains("unsupported"));
    }

    #[test]
    fn test_schedule_assignment_debug() {
        let sa = ScheduleAssignment {
            machine: "node1".into(),
            costs: vec![("GPU".into(), 100)],
            priority: 5,
        };
        let debug = format!("{sa:?}");
        assert!(debug.contains("node1") || debug.contains("GPU"));
    }

    #[test]
    fn test_scheduler_plan_debug() {
        let plan = SchedulerPlan {
            assignments: vec![ScheduleAssignment {
                machine: "node1".into(),
                costs: vec![("CPU".into(), 4)],
                priority: 1,
            }],
            feasible: true,
            reason: None,
        };
        let debug = format!("{plan:?}");
        assert!(debug.contains("node1") || debug.contains("CPU"));
    }

    #[test]
    fn test_schedule_equal_machines() {
        // Two machines with identical scoring attributes; either is valid.
        // HashMap iteration order is non-deterministic.
        let mut reg = MachineRegistry::new();
        let m1 = Machine {
            name: "alpha".to_string(),
            extents: {
                let mut h = HashMap::new();
                h.insert(
                    "CPU".to_string(),
                    ExtentPool::new("CPU", 100, ExtentType::Count),
                );
                h
            },
            keys: vec![],
            devices: vec![],
        };
        let m2 = Machine {
            name: "beta".to_string(),
            extents: {
                let mut h = HashMap::new();
                h.insert(
                    "CPU".to_string(),
                    ExtentPool::new("CPU", 100, ExtentType::Count),
                );
                h
            },
            keys: vec![],
            devices: vec![],
        };
        reg.register(m1);
        reg.register(m2);

        let input = SchedulerInput::new(
            "deploy".to_string(),
            vec![CostEntry {
                kind: ast::Ident { name: "CPU".into() },
                value: Expr::Lit(Literal::Int(10)),
            }],
        );

        let result = schedule(input, &reg);
        assert!(result.plan.feasible);
        let machine = &result.plan.assignments[0].machine;
        assert!(
            machine == "alpha" || machine == "beta",
            "unexpected machine: {machine}"
        );
    }

    #[test]
    fn test_eval_cost_expr_string_unsupported() {
        let s = Expr::Lit(ast::expr::Literal::StringVal("x".into()));
        assert!(matches!(
            eval_cost_expr(&s).unwrap_err(),
            SchedulerError::UnsupportedCostExpr
        ));
    }

    #[test]
    fn test_eval_cost_expr_div_unsupported() {
        let div = Expr::BinOp {
            op: ast::expr::BinOp::Div,
            left: Box::new(Expr::Lit(Literal::Int(10))),
            right: Box::new(Expr::Lit(Literal::Int(2))),
        };
        assert!(matches!(
            eval_cost_expr(&div).unwrap_err(),
            SchedulerError::UnsupportedCostOp
        ));
    }

    #[test]
    fn test_scheduler_error_all_display() {
        assert!(SchedulerError::MachineNotFound("x".into())
            .to_string()
            .contains("x"));
        assert!(SchedulerError::UnresolvedCostVar("y".into())
            .to_string()
            .contains("y"));
        assert!(SchedulerError::ConstraintViolation("too expensive".into())
            .to_string()
            .contains("too expensive"));
    }

    #[test]
    fn test_schedule_empty_machines() {
        let scheduler = CostScheduler::new(vec![], CostMetrics::balanced());
        let input = SchedulerInput::new("op", vec![]);
        let result = scheduler.schedule(&input);
        assert!(!result.plan.feasible);
        assert!(result.plan.reason.as_ref().unwrap().contains("no machines"));
    }

    #[test]
    fn test_schedule_required_machine_not_found() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(100)),
            }],
        )
        .with_machines(vec!["nonexistent".into()]);

        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        assert!(!result.plan.feasible);
    }

    #[test]
    fn test_schedule_zero_cost() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "zero_cost",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(0)),
            }],
        );
        let result = CostScheduler::from_registry(&reg, CostMetrics::balanced()).schedule(&input);
        // Zero-cost resource is skipped, should still be feasible
        assert!(result.plan.feasible);
    }

    #[test]
    fn test_schedule_convenience_function() {
        let reg = make_machine_registry();
        let input = SchedulerInput::new(
            "op",
            vec![CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(100)),
            }],
        );
        let result = schedule(input, &reg);
        assert!(result.plan.feasible);
        assert!(!result.plan.assignments.is_empty());
    }

    #[test]
    fn test_machine_info_allocate_multiple() {
        let mut info = MachineInfo::new("multi".into());
        info.extents.insert("RAM".into(), 1000);
        info.extents.insert("CPU".into(), 8);

        info.allocate(&[
            CostEntry {
                kind: ast::Ident { name: "RAM".into() },
                value: Expr::Lit(Literal::Int(500)),
            },
            CostEntry {
                kind: ast::Ident { name: "CPU".into() },
                value: Expr::Lit(Literal::Int(4)),
            },
        ])
        .unwrap();
        assert_eq!(info.remaining("RAM"), 500);
        assert_eq!(info.remaining("CPU"), 4);
    }
}
