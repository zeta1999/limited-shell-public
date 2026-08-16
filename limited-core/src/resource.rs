//! Runtime resource management, extent engine, machine registry, and cost tracking.
//!
//! - [`RuntimeValue`] — runtime values for actual resource instances
//! - [`ResourceInstance`] — a named resource with typed field values
//! - [`ResourceRegistry`] — registry of resource instances with field accessors
//! - [`ExtentPool`] — named extent pool with allocation tracking
//! - [`Device`] — device with extents, rates, and inherited extents
//! - [`ExtentEngine`] — extent allocation, constraint evaluation, device inheritance
//! - [`Machine`] — machine declaration with extents, keys, and devices
//! - [`MachineRegistry`] — registry of machines with set and inheritance support
//! - [`CostTracker`] — per-operation cost consumption tracking

use crate::ast;
use std::collections::{HashMap, HashSet};

// ─── Runtime value model ───────────────────────────────────────

/// Runtime values for actual resource instances (as opposed to AST types).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RuntimeValue {
    Null,
    Bool(bool),
    Int(i64),
    Bytes(u64),
    StringVal(String),
    /// A resource instance: (name, field_name → value map)
    Resource(String, HashMap<String, RuntimeValue>),
    /// A list of runtime values
    List(Vec<RuntimeValue>),
    /// An anonymous struct/map: field_name → value
    Struct(HashMap<String, RuntimeValue>),
}

impl RuntimeValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            RuntimeValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            RuntimeValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<u64> {
        match self {
            RuntimeValue::Bytes(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            RuntimeValue::StringVal(s) => Some(s),
            _ => None,
        }
    }

    pub fn resource_type(&self) -> Option<&str> {
        match self {
            RuntimeValue::Resource(name, _) => Some(name),
            _ => None,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            RuntimeValue::Null => false,
            RuntimeValue::Bool(b) => *b,
            RuntimeValue::Int(n) => *n != 0,
            RuntimeValue::Bytes(n) => *n != 0,
            RuntimeValue::StringVal(s) => !s.is_empty(),
            RuntimeValue::Resource(_, fields) => !fields.is_empty(),
            RuntimeValue::List(items) => !items.is_empty(),
            RuntimeValue::Struct(map) => !map.is_empty(),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            RuntimeValue::Null => "Null",
            RuntimeValue::Bool(_) => "Bool",
            RuntimeValue::Int(_) => "Int",
            RuntimeValue::Bytes(_) => "Bytes",
            RuntimeValue::StringVal(_) => "String",
            RuntimeValue::Resource(_, _) => "Resource",
            RuntimeValue::List(_) => "List",
            RuntimeValue::Struct(_) => "Struct",
        }
    }
}

impl std::fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeValue::Null => write!(f, "null"),
            RuntimeValue::Bool(b) => write!(f, "{b}"),
            RuntimeValue::Int(n) => write!(f, "{n}"),
            RuntimeValue::Bytes(n) => write!(f, "{n}B"),
            RuntimeValue::StringVal(s) => write!(f, "{s}"),
            RuntimeValue::Resource(name, fields) => {
                write!(f, "{name}(")?;
                let mut first = true;
                for (k, v) in fields {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}={v}")?;
                    first = false;
                }
                write!(f, ")")
            }
            RuntimeValue::List(items) => {
                write!(f, "[")?;
                let mut first = true;
                for item in items {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                    first = false;
                }
                write!(f, "]")
            }
            RuntimeValue::Struct(map) => {
                write!(f, "{{")?;
                let mut first = true;
                for (k, v) in map {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                    first = false;
                }
                write!(f, "}}")
            }
        }
    }
}

/// A named resource instance with field values.
#[derive(Debug, Clone)]
pub struct ResourceInstance {
    pub name: String,
    /// resource type name (e.g., "File", "Server")
    pub resource_type: String,
    /// field_name → runtime value
    pub fields: HashMap<String, RuntimeValue>,
}

impl ResourceInstance {
    pub fn new(name: impl Into<String>, resource_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            resource_type: resource_type.into(),
            fields: HashMap::new(),
        }
    }

    pub fn set_field(&mut self, field: impl Into<String>, value: RuntimeValue) {
        self.fields.insert(field.into(), value);
    }

    pub fn get_field(&self, field: &str) -> Option<&RuntimeValue> {
        self.fields.get(field)
    }
}

// ─── Resource registry ─────────────────────────────────────────

/// Registry of resource instances with field accessors.
#[derive(Debug, Clone, Default)]
pub struct ResourceRegistry {
    /// name → resource instance
    instances: HashMap<String, ResourceInstance>,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new resource instance.
    pub fn register(&mut self, instance: ResourceInstance) {
        self.instances.insert(instance.name.clone(), instance);
    }

    /// Get a resource instance by name.
    pub fn get(&self, name: &str) -> Option<&ResourceInstance> {
        self.instances.get(name)
    }

    /// Get a mutable reference to a resource instance.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut ResourceInstance> {
        self.instances.get_mut(name)
    }

    /// Get a typed field value from a resource.
    pub fn field_value(&self, resource: &str, field: &str) -> Option<&RuntimeValue> {
        self.instances
            .get(resource)
            .and_then(|r| r.get_field(field))
    }

    /// Set a field on a resource.
    pub fn set_field(
        &mut self,
        resource: &str,
        field: &str,
        value: RuntimeValue,
    ) -> Result<(), String> {
        self.instances
            .get_mut(resource)
            .map(|r| r.set_field(field, value))
            .ok_or_else(|| format!("resource '{}' not found", resource))
    }

    /// List all registered resource names.
    pub fn list(&self) -> Vec<&str> {
        self.instances.keys().map(|k| k.as_str()).collect()
    }

    /// Check if a resource exists.
    pub fn contains(&self, name: &str) -> bool {
        self.instances.contains_key(name)
    }

    /// Check if a resource has a given type.
    pub fn has_type(&self, name: &str, resource_type: &str) -> bool {
        self.instances
            .get(name)
            .map(|r| r.resource_type == resource_type)
            .unwrap_or(false)
    }
}

// ─── Extent engine ─────────────────────────────────────────────

/// A named extent pool that can be allocated and freed.
#[derive(Debug, Clone)]
pub struct ExtentPool {
    /// extent name
    pub name: String,
    /// total capacity
    pub capacity: u64,
    /// amount currently allocated
    pub allocated: u64,
    /// type of extent (bytes, count)
    pub extent_type: ExtentType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtentType {
    Bytes,
    Count,
}

impl ExtentPool {
    pub fn new(name: impl Into<String>, capacity: u64, extent_type: ExtentType) -> Self {
        Self {
            name: name.into(),
            capacity,
            allocated: 0,
            extent_type,
        }
    }

    /// Check if this pool can accommodate the given amount.
    pub fn can_alloc(&self, amount: u64) -> bool {
        self.capacity - self.allocated >= amount
    }

    /// Allocate from this pool.
    pub fn alloc(&mut self, amount: u64) -> Result<u64, String> {
        if !self.can_alloc(amount) {
            return Err(format!(
                "extent '{}' exhausted: need {} but only {} available (total {})",
                self.name,
                amount,
                self.capacity - self.allocated,
                self.capacity
            ));
        }
        self.allocated += amount;
        Ok(self.allocated)
    }

    /// Free allocated amount from this pool.
    pub fn free(&mut self, amount: u64) {
        self.allocated = self.allocated.saturating_sub(amount);
    }

    /// Remaining capacity.
    pub fn remaining(&self) -> u64 {
        self.capacity - self.allocated
    }
}

/// A device with extents, rates, and inherited extents.
#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    /// extent pools: name → pool
    pub extents: HashMap<String, ExtentPool>,
    /// bandwidth rates: name → rate (bytes/sec)
    pub rates: HashMap<String, u64>,
    /// inherited extents from parent device
    pub inherited_extents: HashMap<String, ExtentPool>,
}

impl Device {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            extents: HashMap::new(),
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        }
    }

    /// Add an extent pool to this device.
    pub fn add_extent(&mut self, pool: ExtentPool) {
        self.extents.insert(pool.name.clone(), pool);
    }

    /// Add a rate definition.
    pub fn add_rate(&mut self, name: impl Into<String>, rate: u64) {
        self.rates.insert(name.into(), rate);
    }

    /// Add an inherited extent from a parent device.
    pub fn add_inherited_extent(&mut self, pool: ExtentPool) {
        self.inherited_extents.insert(pool.name.clone(), pool);
    }

    /// Get the effective capacity for an extent, including inherited.
    pub fn total_capacity(&self, extent_name: &str) -> u64 {
        let direct = self
            .extents
            .get(extent_name)
            .map(|p| p.capacity)
            .unwrap_or(0);
        let inherited = self
            .inherited_extents
            .get(extent_name)
            .map(|p| p.capacity)
            .unwrap_or(0);
        direct + inherited
    }

    /// Check if an extent exists (direct or inherited).
    pub fn has_extent(&self, name: &str) -> bool {
        self.extents.contains_key(name) || self.inherited_extents.contains_key(name)
    }

    /// Get all extent names.
    pub fn extent_names(&self) -> Vec<String> {
        let mut names: HashSet<String> = self.extents.keys().cloned().collect();
        for name in self.inherited_extents.keys() {
            names.insert(name.clone());
        }
        names.into_iter().collect()
    }
}

/// Constraint expression for cost rules.
#[derive(Debug, Clone)]
pub enum CostConstraint {
    /// sum(cost extent) <= pool
    SumLt { extent: String, pool: u64 },
    /// sum(cost left_extent) op sum(cost right_extent) <= pool
    SumOpLt {
        op: SumOp,
        left_extent: String,
        right_extent: String,
        pool: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumOp {
    Plus,
    Minus,
}

impl std::fmt::Display for SumOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SumOp::Plus => write!(f, "+"),
            SumOp::Minus => write!(f, "-"),
        }
    }
}

/// Result of a cost constraint evaluation.
#[derive(Debug, Clone)]
pub struct CostConstraintResult {
    pub satisfied: bool,
    pub current_usage: u64,
    pub pool: u64,
    pub constraint: String,
}

/// Extent engine: manages device extents, allocation, and cost constraints.
#[derive(Debug, Clone, Default)]
pub struct ExtentEngine {
    /// devices: name → device
    devices: HashMap<String, Device>,
    /// parent → child relationships for device inheritance
    device_parents: HashMap<String, String>,
}

impl ExtentEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a device with its extent pools and rates.
    pub fn add_device(&mut self, device: Device) {
        self.devices.insert(device.name.clone(), device);
    }

    /// Set parent relationship: `child` inherits extents from `parent`.
    pub fn set_device_parent(&mut self, child: &str, parent: &str) {
        self.device_parents
            .insert(child.to_string(), parent.to_string());
    }

    /// Resolve inherited extents: copy parent extents to child.
    pub fn resolve_inheritance(&mut self) {
        // Collect all (child, parent) pairs
        let pairs: Vec<(String, String)> = self
            .device_parents
            .iter()
            .map(|(c, p)| (c.clone(), p.clone()))
            .collect();

        // Multiple passes until all inheritances are resolved
        let mut changed = true;
        while changed {
            changed = false;
            for (child_name, parent_name) in &pairs {
                // Clone parent data first to avoid borrow conflict
                let parent_extents: Vec<(String, ExtentPool)> = self
                    .devices
                    .get(parent_name)
                    .map(|dev| {
                        dev.extents
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

                // Now borrow child_mut
                if let Some(child) = self.devices.get_mut(child_name) {
                    let mut new_extents = Vec::new();
                    for (ext_name, pool) in parent_extents {
                        if !child.has_extent(&ext_name) {
                            new_extents.push((ext_name.clone(), pool.clone()));
                        }
                    }
                    for pool in new_extents.into_iter().map(|(_, p)| p) {
                        child.add_inherited_extent(pool);
                        changed = true;
                    }
                }
            }
        }
    }

    /// Allocate from a device's extent pool.
    pub fn allocate(&mut self, device: &str, extent: &str, amount: u64) -> Result<u64, String> {
        let device = self
            .devices
            .get_mut(device)
            .ok_or_else(|| format!("device '{}' not found", device))?;

        // Check if extent exists (direct or inherited)
        let pool = device
            .extents
            .get_mut(extent)
            .or_else(|| device.inherited_extents.get_mut(extent))
            .ok_or_else(|| format!("extent '{}' not found on device '{}'", extent, device.name))?;

        pool.alloc(amount)
    }

    /// Free allocated amount from a device's extent pool.
    pub fn deallocate(&mut self, device: &str, extent: &str, amount: u64) {
        if let Some(device) = self.devices.get_mut(device) {
            if let Some(pool) = device.extents.get_mut(extent) {
                pool.free(amount);
            }
            if let Some(pool) = device.inherited_extents.get_mut(extent) {
                pool.free(amount);
            }
        }
    }

    /// Evaluate a cost constraint. Returns whether the constraint is satisfied
    /// and current usage statistics.
    pub fn evaluate_constraint(&self, constraint: &CostConstraint) -> CostConstraintResult {
        match constraint {
            CostConstraint::SumLt { extent, pool } => {
                let current_usage = self.total_usage(extent);
                CostConstraintResult {
                    satisfied: current_usage + pool <= *pool || current_usage <= *pool,
                    current_usage,
                    pool: *pool,
                    constraint: format!("sum(cost {}) <= {}", extent, pool),
                }
            }
            CostConstraint::SumOpLt {
                op,
                left_extent,
                right_extent,
                pool,
            } => {
                let left = self.total_usage(left_extent);
                let right = self.total_usage(right_extent);
                let combined = match op {
                    SumOp::Plus => left.saturating_add(right),
                    SumOp::Minus => left.saturating_sub(right),
                };
                CostConstraintResult {
                    satisfied: combined <= *pool,
                    current_usage: combined,
                    pool: *pool,
                    constraint: format!(
                        "sum(cost {}) {} sum(cost {}) <= {}",
                        left_extent, op, right_extent, pool
                    ),
                }
            }
        }
    }

    /// Check if a constraint can be satisfied given current usage and a new allocation.
    pub fn can_satisfy(&self, constraint: &CostConstraint, new_allocation: u64) -> bool {
        let result = self.evaluate_constraint(constraint);
        match constraint {
            CostConstraint::SumLt { pool, .. } => result.current_usage + new_allocation <= *pool,
            CostConstraint::SumOpLt { pool, .. } => result.current_usage + new_allocation <= *pool,
        }
    }

    /// Get total allocated usage for an extent across all devices.
    fn total_usage(&self, extent: &str) -> u64 {
        self.devices
            .values()
            .flat_map(|d| {
                d.extents
                    .values()
                    .chain(d.inherited_extents.values())
                    .filter(|p| p.name == extent)
            })
            .map(|p| p.allocated)
            .sum()
    }

    /// Get all devices.
    pub fn devices(&self) -> Vec<&Device> {
        self.devices.values().collect()
    }

    /// Get a device by name.
    pub fn get_device(&self, name: &str) -> Option<&Device> {
        self.devices.get(name)
    }

    /// Get the parent of a device.
    pub fn get_device_parent(&self, device: &str) -> Option<&str> {
        self.device_parents.get(device).map(|s| s.as_str())
    }
}

// ─── Machine registry ──────────────────────────────────────────

/// A machine with extents, keys, and devices.
#[derive(Debug, Clone)]
pub struct Machine {
    pub name: String,
    /// extent pools
    pub extents: HashMap<String, ExtentPool>,
    /// machine credential keys
    pub keys: Vec<ast::BytesLit>,
    /// devices attached to this machine: name → device type
    pub devices: Vec<MachineDevice>,
}

/// A device attached to a machine with extent bindings.
#[derive(Debug, Clone)]
pub struct MachineDevice {
    pub name: String,
    pub device_type: String,
    /// extent bindings: extent_name → value
    pub extent_bindings: Vec<(ast::Ident, ast::Expr)>,
}

/// A named set of machines.
#[derive(Debug, Clone, Default)]
pub struct MachineSet {
    pub name: String,
    pub members: HashSet<String>,
}

impl MachineSet {
    pub fn new(name: impl Into<String>, members: HashSet<String>) -> Self {
        Self {
            name: name.into(),
            members,
        }
    }
}

/// Registry of machines with set support.
#[derive(Debug, Clone, Default)]
pub struct MachineRegistry {
    /// machine_name → machine
    machines: HashMap<String, Machine>,
    /// set_name → machine set
    sets: HashMap<String, MachineSet>,
}

impl MachineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a machine.
    pub fn register(&mut self, machine: Machine) {
        self.machines.insert(machine.name.clone(), machine);
    }

    /// Get a machine by name.
    pub fn get(&self, name: &str) -> Option<&Machine> {
        self.machines.get(name)
    }

    /// Get a mutable reference to a machine.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Machine> {
        self.machines.get_mut(name)
    }

    /// List all machine names.
    pub fn list(&self) -> Vec<&str> {
        self.machines.keys().map(|k| k.as_str()).collect()
    }

    /// Check if a machine exists.
    pub fn contains(&self, name: &str) -> bool {
        self.machines.contains_key(name)
    }

    /// Create a named machine set.
    pub fn create_set(&mut self, name: impl Into<String>, members: HashSet<String>) {
        let name_str: String = name.into();
        self.sets
            .insert(name_str.clone(), MachineSet::new(name_str, members));
    }

    /// Get a machine set by name.
    pub fn get_set(&self, name: &str) -> Option<&MachineSet> {
        self.sets.get(name)
    }

    /// Get all machine members in a set.
    pub fn get_set_members(&self, name: &str) -> Option<Vec<String>> {
        self.sets
            .get(name)
            .map(|s| s.members.iter().cloned().collect())
    }

    /// Resolve extent bindings for a machine device against the machine's extents.
    /// Returns a map of extent_name → value for each binding.
    pub fn resolve_extent_bindings(
        &self,
        machine_name: &str,
        bindings: &[(ast::Ident, ast::Expr)],
    ) -> Result<HashMap<String, u64>, String> {
        let _machine = self
            .get(machine_name)
            .ok_or_else(|| format!("machine '{}' not found", machine_name))?;

        let mut result = HashMap::new();
        for (extent_name, expr) in bindings {
            let value = self.resolve_expr_to_bytes(machine_name, expr)?;
            result.insert(extent_name.name.clone(), value);
        }
        Ok(result)
    }

    /// Resolve an expression to a bytes value (u64).
    fn resolve_expr_to_bytes(&self, _machine_name: &str, expr: &ast::Expr) -> Result<u64, String> {
        use ast::expr::{Expr, Literal};
        match expr {
            Expr::Lit(Literal::Int(n)) => Ok(*n as u64),
            Expr::Lit(Literal::Bytes(b)) => Ok(b.value),
            Expr::Var(_) => {
                // In a real implementation, this would look up the variable
                // in the runtime context. For now, return 0 as placeholder.
                Ok(0)
            }
            _ => Err("cannot resolve expression to bytes value".into()),
        }
    }
}

// ─── Cost tracker ──────────────────────────────────────────────

/// Tracks cost consumption per operation and machine.
#[derive(Debug, Clone, Default)]
pub struct CostTracker {
    /// operation_name → (machine_name, kind → cost)
    operations: HashMap<String, HashMap<String, HashMap<String, u64>>>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a cost entry for an operation on a machine.
    pub fn track(&mut self, operation: &str, machine: &str, kind: &str, amount: u64) {
        self.operations
            .entry(operation.to_string())
            .or_default()
            .entry(machine.to_string())
            .or_default()
            .entry(kind.to_string())
            .and_modify(|e| *e += amount)
            .or_insert(amount);
    }

    /// Get the cost for an operation on a machine for a given kind.
    pub fn get_cost(&self, operation: &str, machine: &str, kind: &str) -> Option<u64> {
        self.operations
            .get(operation)?
            .get(machine)?
            .get(kind)
            .copied()
    }

    /// Get all costs for an operation on a machine.
    pub fn get_all_costs(&self, operation: &str, machine: &str) -> HashMap<String, u64> {
        self.operations
            .get(operation)
            .and_then(|m| m.get(machine))
            .cloned()
            .unwrap_or_default()
    }

    /// Get total cost across all kinds for an operation.
    pub fn total_cost(&self, operation: &str) -> u64 {
        self.operations
            .get(operation)
            .map(|machines| machines.values().flat_map(|kinds| kinds.values()).sum())
            .unwrap_or(0)
    }

    /// Check if an operation has been tracked.
    pub fn has_operation(&self, operation: &str) -> bool {
        self.operations.contains_key(operation)
    }

    /// Clear costs for a specific operation.
    pub fn clear_operation(&mut self, operation: &str) {
        self.operations.remove(operation);
    }

    /// Reset all tracked costs.
    pub fn reset(&mut self) {
        self.operations.clear();
    }

    /// List all tracked operations.
    pub fn operations(&self) -> Vec<&str> {
        self.operations.keys().map(|k| k.as_str()).collect()
    }
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::{Expr, Literal};

    #[allow(dead_code)]
    fn int_lit(n: i64) -> Expr {
        Expr::Lit(Literal::Int(n))
    }

    fn bytes_lit(value: u64, suffix: ast::BytesSuffix) -> ast::BytesLit {
        ast::BytesLit { value, suffix }
    }

    #[allow(dead_code)]
    fn bytes_expr(value: u64) -> Expr {
        Expr::Lit(Literal::Bytes(bytes_lit(value, ast::BytesSuffix::None)))
    }

    fn string_val(s: &str) -> RuntimeValue {
        RuntimeValue::StringVal(s.to_string())
    }

    // ─── Runtime value tests ───

    #[test]
    fn test_runtime_value_truthy() {
        assert!(RuntimeValue::Bool(true).is_truthy());
        assert!(!RuntimeValue::Bool(false).is_truthy());
        assert!(!RuntimeValue::Int(0).is_truthy());
        assert!(RuntimeValue::Int(42).is_truthy());
        assert!(!RuntimeValue::Null.is_truthy());
        assert!(RuntimeValue::StringVal("hi".to_string()).is_truthy());
        assert!(!RuntimeValue::StringVal(String::new()).is_truthy());
        assert!(!RuntimeValue::Resource("X".to_string(), HashMap::new()).is_truthy());
    }

    // ─── Resource registry tests ───

    #[test]
    fn test_resource_registry_register_and_get() {
        let mut reg = ResourceRegistry::new();
        let mut instance = ResourceInstance::new("file1", "File");
        instance.set_field("path", string_val("/home/user"));
        instance.set_field("size", RuntimeValue::Int(1024));
        reg.register(instance);

        let r = reg.get("file1").unwrap();
        assert_eq!(r.resource_type, "File");
        assert_eq!(r.get_field("path").unwrap(), &string_val("/home/user"));
        assert_eq!(r.get_field("size").unwrap(), &RuntimeValue::Int(1024));
    }

    #[test]
    fn test_resource_registry_field_value() {
        let mut reg = ResourceRegistry::new();
        let mut instance = ResourceInstance::new("server1", "Server");
        instance.set_field("ram", RuntimeValue::Bytes(64 * 1024 * 1024 * 1024));
        reg.register(instance);

        let ram = reg.field_value("server1", "ram").unwrap();
        assert_eq!(ram.as_bytes(), Some(64 * 1024 * 1024 * 1024));

        assert!(reg.field_value("server1", "missing").is_none());
        assert!(reg.field_value("nonexistent", "ram").is_none());
    }

    #[test]
    fn test_resource_registry_set_field() {
        let mut reg = ResourceRegistry::new();
        let instance = ResourceInstance::new("file1", "File");
        reg.register(instance);

        reg.set_field("file1", "path", string_val("/new/path"))
            .unwrap();
        assert_eq!(
            reg.field_value("file1", "path").unwrap(),
            &string_val("/new/path")
        );

        assert!(reg.set_field("missing", "path", string_val("x")).is_err());
    }

    #[test]
    fn test_resource_registry_list_and_contains() {
        let mut reg = ResourceRegistry::new();
        reg.register(ResourceInstance::new("a", "TypeA"));
        reg.register(ResourceInstance::new("b", "TypeB"));

        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"a"));
        assert!(list.contains(&"b"));
        assert!(reg.contains("a"));
        assert!(!reg.contains("c"));
        assert!(reg.has_type("a", "TypeA"));
        assert!(!reg.has_type("a", "TypeB"));
    }

    // ─── Extent pool tests ───

    #[test]
    fn test_extent_pool_alloc_free() {
        let mut pool = ExtentPool::new("RAM", 1024, ExtentType::Bytes);
        assert!(pool.can_alloc(500));
        assert!(!pool.can_alloc(1025));

        pool.alloc(500).unwrap();
        assert_eq!(pool.remaining(), 524);
        assert!(!pool.can_alloc(525));

        pool.free(300);
        assert_eq!(pool.remaining(), 824);
        assert!(pool.can_alloc(525));
    }

    #[test]
    fn test_extent_pool_exhausted() {
        let mut pool = ExtentPool::new("NVRAM", 100, ExtentType::Bytes);
        assert!(pool.alloc(100).is_ok());
        assert!(pool.alloc(1).is_err());
    }

    // ─── Device tests ───

    #[test]
    fn test_device_extents() {
        let mut device = Device::new("gpu0");
        device.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        device.add_extent(ExtentPool::new("RAM", 64 * 1024, ExtentType::Bytes));
        device.add_rate("compute", 1000);

        assert!(device.has_extent("GPUVRAM"));
        assert!(!device.has_extent("DISK"));
        assert_eq!(device.total_capacity("GPUVRAM"), 16 * 1024);
        assert_eq!(device.total_capacity("nonexistent"), 0);

        let names = device.extent_names();
        assert!(names.contains(&"GPUVRAM".to_string()));
        assert!(names.contains(&"RAM".to_string()));
    }

    #[test]
    fn test_device_inheritance() {
        let mut parent = Device::new("parent");
        parent.add_extent(ExtentPool::new("SharedRAM", 1024, ExtentType::Bytes));

        let mut child = Device::new("child");
        child.add_extent(ExtentPool::new("LocalRAM", 512, ExtentType::Bytes));
        child.add_inherited_extent(ExtentPool::new("SharedRAM", 1024, ExtentType::Bytes));

        assert_eq!(child.total_capacity("SharedRAM"), 1024);
        assert_eq!(child.total_capacity("LocalRAM"), 512);
        assert_eq!(child.total_capacity("nonexistent"), 0);

        let names = child.extent_names();
        assert!(names.contains(&"SharedRAM".to_string()));
        assert!(names.contains(&"LocalRAM".to_string()));
    }

    // ─── Extent engine tests ───

    #[test]
    fn test_extent_engine_add_and_allocate() {
        let mut engine = ExtentEngine::new();
        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        engine.add_device(gpu);

        let alloc = engine.allocate("gpu0", "GPUVRAM", 1000).unwrap();
        assert!(alloc > 0);

        let device = engine.get_device("gpu0").unwrap();
        let pool = device.extents.get("GPUVRAM").unwrap();
        assert_eq!(pool.allocated, 1000);
    }

    #[test]
    fn test_extent_engine_allocate_exhausted() {
        let mut engine = ExtentEngine::new();
        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 1000, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.allocate("gpu0", "GPUVRAM", 800).unwrap();
        let result = engine.allocate("gpu0", "GPUVRAM", 300);
        assert!(result.is_err());
    }

    #[test]
    fn test_extent_engine_deallocate() {
        let mut engine = ExtentEngine::new();
        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.allocate("gpu0", "GPUVRAM", 5000).unwrap();
        engine.deallocate("gpu0", "GPUVRAM", 3000);

        let device = engine.get_device("gpu0").unwrap();
        let pool = device.extents.get("GPUVRAM").unwrap();
        assert_eq!(pool.allocated, 2000);
    }

    #[test]
    fn test_extent_engine_device_inheritance() {
        let mut engine = ExtentEngine::new();

        let mut shared = Device::new("shared_pool");
        shared.add_extent(ExtentPool::new("SharedRAM", 4096, ExtentType::Bytes));
        engine.add_device(shared);

        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.set_device_parent("gpu0", "shared_pool");
        engine.resolve_inheritance();

        // gpu0 now inherits SharedRAM from shared_pool
        let gpu_dev = engine.get_device("gpu0").unwrap();
        assert!(gpu_dev.has_extent("SharedRAM"));
        assert_eq!(gpu_dev.total_capacity("SharedRAM"), 4096);
        assert_eq!(gpu_dev.total_capacity("GPUVRAM"), 16 * 1024);
    }

    #[test]
    fn test_extent_engine_constraint_evaluation() {
        let mut engine = ExtentEngine::new();

        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.allocate("gpu0", "GPUVRAM", 1000).unwrap();

        let constraint = CostConstraint::SumLt {
            extent: "GPUVRAM".to_string(),
            pool: 5000,
        };
        let result = engine.evaluate_constraint(&constraint);
        assert!(result.satisfied);
        assert_eq!(result.current_usage, 1000);
        assert_eq!(result.pool, 5000);
    }

    #[test]
    fn test_extent_engine_sumop_constraint() {
        let mut engine = ExtentEngine::new();

        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        gpu.add_extent(ExtentPool::new("SharedRAM", 4096, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.allocate("gpu0", "GPUVRAM", 2000).unwrap();
        engine.allocate("gpu0", "SharedRAM", 1000).unwrap();

        let constraint = CostConstraint::SumOpLt {
            op: SumOp::Plus,
            left_extent: "GPUVRAM".to_string(),
            right_extent: "SharedRAM".to_string(),
            pool: 4000,
        };
        let result = engine.evaluate_constraint(&constraint);
        assert!(result.satisfied); // 2000 + 1000 = 3000 <= 4000
        assert_eq!(result.current_usage, 3000);
    }

    #[test]
    fn test_extent_engine_can_satisfy() {
        let mut engine = ExtentEngine::new();

        let mut gpu = Device::new("gpu0");
        gpu.add_extent(ExtentPool::new("GPUVRAM", 16 * 1024, ExtentType::Bytes));
        engine.add_device(gpu);

        engine.allocate("gpu0", "GPUVRAM", 1000).unwrap();

        let constraint = CostConstraint::SumLt {
            extent: "GPUVRAM".to_string(),
            pool: 5000,
        };
        assert!(engine.can_satisfy(&constraint, 4000)); // 1000 + 4000 = 5000 <= 5000
        assert!(!engine.can_satisfy(&constraint, 4001)); // 1000 + 4001 > 5000
    }

    // ─── Machine registry tests ───

    #[test]
    fn test_machine_registry_register_and_get() {
        let mut reg = MachineRegistry::new();
        let mut machine = Machine {
            name: "node1".to_string(),
            extents: HashMap::new(),
            keys: vec![bytes_lit(1234, ast::BytesSuffix::None)],
            devices: vec![MachineDevice {
                name: "gpu0".to_string(),
                device_type: "NVIDIA_A100".to_string(),
                extent_bindings: vec![],
            }],
        };
        machine.extents.insert(
            "RAM".to_string(),
            ExtentPool::new("RAM", 64 * 1024, ExtentType::Bytes),
        );
        reg.register(machine);

        let m = reg.get("node1").unwrap();
        assert_eq!(m.name, "node1");
        assert_eq!(m.devices.len(), 1);
        assert_eq!(m.devices[0].device_type, "NVIDIA_A100");
    }

    #[test]
    fn test_machine_registry_sets() {
        let mut reg = MachineRegistry::new();
        reg.create_set(
            "gpu_cluster",
            HashSet::from(["node1".to_string(), "node2".to_string()]),
        );

        let members = reg.get_set_members("gpu_cluster").unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"node1".to_string()));
        assert!(members.contains(&"node2".to_string()));

        assert!(reg.get_set("nonexistent").is_none());
    }

    #[test]
    fn test_machine_registry_list_and_contains() {
        let mut reg = MachineRegistry::new();
        reg.register(Machine {
            name: "a".to_string(),
            extents: HashMap::new(),
            keys: vec![],
            devices: vec![],
        });
        reg.register(Machine {
            name: "b".to_string(),
            extents: HashMap::new(),
            keys: vec![],
            devices: vec![],
        });

        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"a"));
        assert!(list.contains(&"b"));
        assert!(reg.contains("a"));
        assert!(!reg.contains("c"));
    }

    // ─── Cost tracker tests ───

    #[test]
    fn test_cost_tracker_track_and_query() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 4096);
        tracker.track("deploy", "node1", "GPUVRAM", 8192);
        tracker.track("deploy", "node2", "RAM", 2048);

        assert_eq!(tracker.get_cost("deploy", "node1", "RAM"), Some(4096));
        assert_eq!(tracker.get_cost("deploy", "node1", "GPUVRAM"), Some(8192));
        assert_eq!(tracker.get_cost("deploy", "node1", "DISK"), None);
    }

    #[test]
    fn test_cost_tracker_accumulation() {
        let mut tracker = CostTracker::new();
        tracker.track("train", "gpu0", "GPUVRAM", 4000);
        tracker.track("train", "gpu0", "GPUVRAM", 3000);

        assert_eq!(tracker.get_cost("train", "gpu0", "GPUVRAM"), Some(7000));
    }

    #[test]
    fn test_cost_tracker_total_and_reset() {
        let mut tracker = CostTracker::new();
        tracker.track("a", "m1", "RAM", 100);
        tracker.track("a", "m1", "GPU", 200);
        tracker.track("b", "m1", "RAM", 50);

        assert_eq!(tracker.total_cost("a"), 300);
        assert_eq!(tracker.total_cost("b"), 50);

        tracker.clear_operation("a");
        assert!(!tracker.has_operation("a"));
        assert!(tracker.has_operation("b"));

        tracker.reset();
        assert!(tracker.operations().is_empty());
    }

    #[test]
    fn test_cost_tracker_all_costs() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 4096);
        tracker.track("deploy", "node1", "GPUVRAM", 8192);
        tracker.track("deploy", "node1", "start", 100);

        let costs = tracker.get_all_costs("deploy", "node1");
        assert_eq!(costs.len(), 3);
        assert_eq!(costs.get("RAM"), Some(&4096));
        assert_eq!(costs.get("GPUVRAM"), Some(&8192));
        assert_eq!(costs.get("start"), Some(&100));
    }

    #[test]
    fn test_resource_registry_get_nonexistent() {
        let registry = ResourceRegistry::new();
        assert!(registry.get("NonExistent").is_none());
    }

    #[test]
    fn test_cost_tracker_query_nonexistent() {
        let tracker = CostTracker::new();
        assert!(tracker.get_cost("deploy", "node1", "RAM").is_none());
    }

    #[test]
    fn test_machine_registry_operations() {
        let mut registry = MachineRegistry::new();
        let machine = Machine {
            name: "node1".to_string(),
            extents: HashMap::new(),
            keys: vec![],
            devices: vec![],
        };
        registry.register(machine);
        let machine2 = Machine {
            name: "node2".to_string(),
            extents: HashMap::new(),
            keys: vec![],
            devices: vec![],
        };
        registry.register(machine2);

        assert!(registry.contains("node1"));
        assert!(!registry.contains("node3"));

        let machines = registry.list();
        assert_eq!(machines.len(), 2);
        assert!(machines.contains(&"node1"));
        assert!(machines.contains(&"node2"));
    }

    #[test]
    fn test_extent_pool_zero_allocation() {
        let mut pool = ExtentPool::new("CPU", 4, ExtentType::Count);
        pool.alloc(0).unwrap();
        assert_eq!(pool.allocated, 0);
        assert_eq!(pool.remaining(), 4);
    }

    #[test]
    fn test_extent_engine_get_nonexistent_device() {
        let engine = ExtentEngine::new();
        assert!(engine.get_device("nonexistent").is_none());
    }

    #[test]
    fn test_cost_tracker_update_existing() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "GPU", 500);
        tracker.track("deploy", "node1", "GPU", 100);

        assert_eq!(tracker.get_cost("deploy", "node1", "GPU"), Some(600));
    }

    #[test]
    fn test_runtime_value_accessors() {
        let bool_val = RuntimeValue::Bool(true);
        assert_eq!(bool_val.as_bool(), Some(true));
        assert_eq!(bool_val.as_int(), None);

        let int_val = RuntimeValue::Int(42);
        assert_eq!(int_val.as_int(), Some(42));
        assert_eq!(int_val.as_string(), None);

        let bytes_val = RuntimeValue::Bytes(1024);
        assert_eq!(bytes_val.as_bytes(), Some(1024));
        assert_eq!(bytes_val.as_bool(), None);

        let str_val = RuntimeValue::StringVal("hello".to_string());
        assert_eq!(str_val.as_string(), Some("hello"));
        assert_eq!(str_val.as_int(), None);

        let res_val = RuntimeValue::Resource("Node".to_string(), HashMap::new());
        assert_eq!(res_val.resource_type(), Some("Node"));
        assert_eq!(res_val.as_bool(), None);
    }

    #[test]
    fn test_runtime_value_type_name() {
        assert_eq!(RuntimeValue::Null.type_name(), "Null");
        assert_eq!(RuntimeValue::Bool(true).type_name(), "Bool");
        assert_eq!(RuntimeValue::Int(42).type_name(), "Int");
        assert_eq!(RuntimeValue::Bytes(1024).type_name(), "Bytes");
        assert_eq!(RuntimeValue::StringVal("x".into()).type_name(), "String");
        assert_eq!(
            RuntimeValue::Resource("Node".into(), HashMap::new()).type_name(),
            "Resource"
        );
        assert_eq!(RuntimeValue::List(vec![]).type_name(), "List");
        assert_eq!(RuntimeValue::Struct(HashMap::new()).type_name(), "Struct");
    }

    #[test]
    fn test_runtime_value_truthy_edge_cases() {
        assert!(!RuntimeValue::Null.is_truthy());
        assert!(!RuntimeValue::Bool(false).is_truthy());
        assert!(!RuntimeValue::Int(0).is_truthy());
        assert!(!RuntimeValue::Bytes(0).is_truthy());
        assert!(!RuntimeValue::StringVal("".into()).is_truthy());
        assert!(RuntimeValue::Int(-1).is_truthy());
        assert!(RuntimeValue::List(vec![RuntimeValue::Int(0)]).is_truthy());
    }

    #[test]
    fn test_machine_set() {
        let members: HashSet<String> = HashSet::from_iter(["s1".into(), "s2".into()]);
        let set = MachineSet::new("servers".to_string(), members);
        assert_eq!(set.members.len(), 2);
        assert!(set.members.contains("s1"));
        assert!(set.members.contains("s2"));
        assert!(!set.members.contains("s3"));
    }

    #[test]
    fn test_machine_device_new() {
        let mut extents = HashMap::new();
        extents.insert(
            "CPU".to_string(),
            ExtentPool::new("CPU", 4, ExtentType::Count),
        );
        let device = Device {
            name: "gpu0".to_string(),
            extents,
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        };
        assert_eq!(device.name, "gpu0");
        assert_eq!(device.extents.get("CPU").map(|p| p.remaining()), Some(4));
    }

    #[test]
    fn test_runtime_value_null_truthy() {
        assert!(!RuntimeValue::Null.is_truthy());
    }

    #[test]
    fn test_runtime_value_list_truthy() {
        assert!(!RuntimeValue::List(vec![]).is_truthy());
        assert!(RuntimeValue::List(vec![RuntimeValue::Int(1)]).is_truthy());
    }

    #[test]
    fn test_runtime_value_struct_truthy() {
        let mut map = HashMap::new();
        map.insert("x".to_string(), RuntimeValue::Int(1));
        assert!(RuntimeValue::Struct(map).is_truthy());
    }

    #[test]
    fn test_runtime_value_as_resource_fields() {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), RuntimeValue::StringVal("alpha".into()));
        let val = RuntimeValue::Resource("Machine".into(), fields);
        assert_eq!(val.resource_type(), Some("Machine"));
    }

    #[test]
    fn test_machine_registry_create_set() {
        let mut registry = MachineRegistry::new();
        registry.create_set(
            "prod",
            HashSet::from(["alpha".to_string(), "beta".to_string()]),
        );
        let members = registry.get_set_members("prod").unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"alpha".to_string()));
        assert!(members.contains(&"beta".to_string()));
    }

    #[test]
    fn test_machine_registry_get_set_nonexistent() {
        let registry = MachineRegistry::new();
        assert!(registry.get_set("nonexistent").is_none());
        assert!(registry.get_set_members("nonexistent").is_none());
    }

    #[test]
    fn test_machine_device_rates() {
        let mut extents = HashMap::new();
        extents.insert(
            "CPU".to_string(),
            ExtentPool::new("CPU", 4, ExtentType::Count),
        );
        let mut rates = HashMap::new();
        rates.insert("CPU".to_string(), 1_500_000_000u64);
        let device = Device {
            name: "gpu0".to_string(),
            extents,
            rates,
            inherited_extents: HashMap::new(),
        };
        assert_eq!(device.rates.get("CPU"), Some(&1_500_000_000u64));
    }

    #[test]
    fn test_extent_engine_add_device() {
        let mut engine = ExtentEngine::new();
        let mut extents = HashMap::new();
        extents.insert(
            "GPU".to_string(),
            ExtentPool::new("GPU", 2, ExtentType::Count),
        );
        engine.add_device(Device {
            name: "gpu0".to_string(),
            extents,
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        });
        let device = engine.get_device("gpu0").unwrap();
        assert_eq!(device.name, "gpu0");
        assert_eq!(device.extents.get("GPU").unwrap().remaining(), 2);
    }

    #[test]
    fn test_extent_engine_inheritance_resolve() {
        let mut engine = ExtentEngine::new();
        let mut parent_extents = HashMap::new();
        parent_extents.insert(
            "CPU".to_string(),
            ExtentPool::new("CPU", 8, ExtentType::Count),
        );
        engine.add_device(Device {
            name: "parent".to_string(),
            extents: parent_extents,
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        });
        engine.add_device(Device {
            name: "child".to_string(),
            extents: HashMap::new(),
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        });
        engine.set_device_parent("child", "parent");
        engine.resolve_inheritance();
        let child = engine.get_device("child").unwrap();
        assert_eq!(child.total_capacity("CPU"), 8);
    }

    #[test]
    fn test_extent_pool_remaining_bytes() {
        let mut pool = ExtentPool::new("RAM", 1_000_000_000, ExtentType::Bytes);
        assert_eq!(pool.remaining(), 1_000_000_000);
        pool.alloc(500_000_000).unwrap();
        assert_eq!(pool.remaining(), 500_000_000);
    }

    #[test]
    fn test_extent_engine_device_extent_names() {
        let mut engine = ExtentEngine::new();
        let mut extents = HashMap::new();
        extents.insert(
            "CPU".to_string(),
            ExtentPool::new("CPU", 4, ExtentType::Count),
        );
        extents.insert(
            "RAM".to_string(),
            ExtentPool::new("RAM", 16, ExtentType::Bytes),
        );
        engine.add_device(Device {
            name: "gpu0".to_string(),
            extents,
            rates: HashMap::new(),
            inherited_extents: HashMap::new(),
        });
        let device = engine.get_device("gpu0").unwrap();
        let names = device.extent_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"CPU".to_string()));
        assert!(names.contains(&"RAM".to_string()));
    }

    #[test]
    fn test_cost_tracker_different_machines() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        tracker.track("deploy", "node2", "RAM", 200);
        assert_eq!(tracker.get_cost("deploy", "node1", "RAM"), Some(100));
        assert_eq!(tracker.get_cost("deploy", "node2", "RAM"), Some(200));
    }

    #[test]
    fn test_cost_tracker_different_ops() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        tracker.track("teardown", "node1", "RAM", 50);
        assert_eq!(tracker.get_cost("deploy", "node1", "RAM"), Some(100));
        assert_eq!(tracker.get_cost("teardown", "node1", "RAM"), Some(50));
    }

    #[test]
    fn test_cost_tracker_clear_operation() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        tracker.track("deploy", "node1", "GPU", 50);
        tracker.clear_operation("deploy");
        assert!(tracker.get_cost("deploy", "node1", "RAM").is_none());
        assert!(tracker.get_cost("deploy", "node1", "GPU").is_none());
    }

    #[test]
    fn test_cost_tracker_operations() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        tracker.track("teardown", "node1", "RAM", 50);
        let ops = tracker.operations();
        assert_eq!(ops.len(), 2);
        assert!(ops.contains(&"deploy"));
        assert!(ops.contains(&"teardown"));
    }

    #[test]
    fn test_cost_tracker_has_operation() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        assert!(tracker.has_operation("deploy"));
        assert!(!tracker.has_operation("nonexistent"));
    }

    #[test]
    fn test_cost_tracker_reset() {
        let mut tracker = CostTracker::new();
        tracker.track("deploy", "node1", "RAM", 100);
        tracker.reset();
        assert!(tracker.get_cost("deploy", "node1", "RAM").is_none());
    }
}
