# Limited Shell — Manual Testing & Verification

## Prerequisites

- Rust toolchain installed
- `cargo test --package limited-core` passes

## Full Test Suite

```bash
cargo test --package limited-core
```

**Expected:** 630 passed, 0 failed, 0 ignored

## Test Breakdown by Module

| Module | Tests | What it verifies |
|--------|-------|-----------------|
| `parser` | 233 | All DSL syntax: statements, control flow, types, expressions, cost rules, operations, services, roles, grants |
| `execute` | 101 | Execution engine: context, expression evaluation, builtins, templates, control flow, condition predicates, op statements, runtime values |
| `ty` | 98 | Type checking: let bindings, expr types, registry, role environment, condition evaluator, policy engine, grant system |
| `ast` | 37 | AST display, types, literals, expressions, binops |
| `pipeline` | 43 | Full pipeline: parse → type-check → schedule → execute |
| `pretty` | 37 | Pretty printing: expressions, conditions, types, role refs, statements |
| `resource` | 51 | Resource subsystem: runtime values, registry, extent engine, machine registry, cost tracker, devices |
| `scheduler` | 30 | Cost-aware scheduling: machine scoring, allocation, constraints, cost metrics |

### Parser (233 tests)

Covers:
- Empty programs, items (alias, role, resource, device, machine, operation, service)
- Statements: let, on, exec, for, if/else, try/catch, while
- Control flow: for loops (list, dict), if/else branches
- Types: primitive, resource, list, map, set, ordered types
- Expressions: literals, variables, field/index access, calls, binops, unops
- Operations: requires, options, cost rules, choose expressions
- Roles: can define, hierarchy
- Grants: role, condition, resource pattern
- Services: replicas, operation definitions
- Error cases: parse failures, syntax errors

### Execute (101 tests)

Sub-systems covered:
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `ExecutionContext` | 3 | bind, lookup, scope push/pop |
| Expression evaluation | 15 | literals, variables, binop, unop, field/index access, calls |
| Builtins | 5 | len, exists, range, to_int, unknown |
| Templates | 2 | literal, variable interpolation |
| Control flow | 6 | if/else, for-list, for-dict, try-succ, try-finally |
| Task items | 2 | bind, expr eval |
| Condition predicates | 5 | exists, not, and, or, startswith, drop_prefix_eq |
| Op statements | 5 | require, let_decl, choose, on_machine |
| Func statements | 3 | return, let_decl, require fail |
| Func body | 3 | return, no-return, early-return |
| RuntimeValue | 3 | truthy, display, eq |
| Index/binop/struct tests | 15 | struct literals, choose error, builtin calls, index access, binops |

### Type Checking (98 tests)

Sub-systems covered:
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `TyEnv` | 2 | bind/resolve, child scope |
| Expression types | 12 | literals, vars, binops (eq, neq, lt, le, gt, ge, and, or, plus, minus, mul, div), unops, comparisons |
| Let bindings | 3 | type inference, missing init, explicit type |
| `TypeRegistry` | 5 | resource registration, alias, resolve type, deep resolve, is_resource |
| Field/index access | 5 | struct field, resource field, list index, map index, set index |
| `RoleEnv` | 7 | add role, parent/child, resolve down, ancestors, cycle detection |
| `CondValue` | 3 | truthy, as_str, is_string |
| Condition evaluator | 8 | starts_with, ends_with, in_set, exists, not, and/or, expr_to_value |
| `EvalContext` | 3 | bind/resolve, child scope, eval expr to value |
| Policy engine | 4 | can check granted, denied, deny overrides, conditions |
| `GrantSystem` | 4 | process grant, no authority, duplicate, transitive permission |
| Role resolution | 3 | empty hierarchy, deep hierarchy |
| Binop edge cases | 10 | numeric mixed types, bool-plus error, lt/neq/or/mul/div/minus |
| Grant evaluation | 4 | self-grant, conditions |
| CondValue extras | 2 | resource/node/string conversion |
| GrantResult | 2 | granted, failed |

### Resource (51 tests)

Sub-systems covered:
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `RuntimeValue` | 6 | truthy (null, bool, int, bytes, string, resource, list, struct), type_name, as_resource |
| `ResourceRegistry` | 5 | register, get, field_value, set_field, list/contains |
| `ExtentPool` | 3 | capacity, allocate, free, remaining, zero alloc, bytes |
| `ExtentEngine` | 5 | device add, inheritance, allocate, constraint check, can_satisfy, device names, inheritance resolve |
| `MachineRegistry` | 4 | register, get, machine sets, create/get sets, set nonexistent |
| `CostTracker` | 6 | track, query, reset, accumulation, all costs, nonexistent query, different machines, different ops, clear, has_operation |
| `Machine`/`Device` | 2 | machine creation, device rates |

### Scheduler (30 tests)

Sub-systems covered:
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `MachineInfo` | 3 | from_registry, remaining, unknown extent |
| Scheduling feasibility | 4 | with machines, insufficient resources, no machines, required machine not found |
| `CostMetrics` | 4 | balanced, time/ram/cost optimized, values |
| Cost evaluation | 3 | basic expr, binop add/mul, variable unresolved |
| Cost constraints | 3 | satisfied, failed, no constraints |
| Allocation/deallocation | 3 | track and return resources, insufficient resources, multi-resource |
| Machine preference | 3 | cheapest, biggest, equal machines |
| Error display | 3 | InsufficientExtent, NegativeCost, UnsupportedCostOp |
| Debug/Display | 3 | ScheduleAssignment, SchedulerPlan, error variants |
| Convenience | 1 | schedule function |

### Pipeline (43 tests)

Covers:
- Empty programs, single/multiple statements
- Control flow parsing
- Operation parsing with requires/options/cost
- Schedule infeasibility
- Execution state
- Type checking: alias, resource decl, function, operation with let, service
- Full pipeline: items, statements, schedules, execution
- Statements: tasks, grant, alias, for-loop, if, multiple
- Operation with multiple options
- Function declaration
- Try/catch, while loop
- Error cases: parse error, type check alias, type error unknown var, type error mismatch, resource with fields
- Error display: all PipelineError variants
- Complex programs: roles + aliases, resource declarations, device declarations, machine declarations, multiple operations, service declarations, scheduling with no machines, cost operations, for/if statements

### Pretty Printer (37 tests)

Covers:
- Empty program, let statement, if statement, binops, unops
- Call expr, field access, index access, struct literal
- Choose expr (with/without from), template, string escaping, bytes
- List/map/set types, bytes suffix, machines inline/set
- Conditions: exists, starts_with, matches, in_set, not, and/or, drop_prefix, can, is_role, is_role_down
- Binop strings, role refs, secret source, ordered map, mut list, bytes suffix variants

### AST (37 tests)

Covers:
- Ident display, Program empty
- PrimitiveType, all Type variants
- BytesLit, BytesSuffix
- MachineExtent, CostConstraint
- Expr variants, BinOp variants
- Various AST structures and Display impls

## Execution Sub-system Tests

Phase 4 execution engine test sub-systems:

```bash
cargo test --package limited-core -- execute::tests --nocapture
```

**Expected:** 101 execute tests pass

### Execution sub-systems covered
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `ExecutionContext` | 3 | bind, lookup, scope push/pop |
| Expression evaluation | 15 | literals, variables, binop, unop, field/index access, calls |
| Builtins | 5 | len, exists, range, to_int, unknown |
| Templates | 2 | literal, variable interpolation |
| Control flow | 6 | if/else, for-list, for-dict, try-succ, try-finally |
| Task items | 2 | bind, expr eval |
| Condition predicates | 5 | exists, not, and, or, startswith, drop_prefix_eq |
| Op statements | 5 | require, let_decl, choose, on_machine |
| Func statements | 3 | return, let_decl, require fail |
| Func body | 3 | return, no-return, early-return |
| RuntimeValue | 3 | truthy, display, eq |

## Clippy Check

```bash
cargo clippy --package limited-core 2>&1 | grep -c '^error'
```

**Expected:** 0 errors (only pre-existing warnings)

## Round-trip Verification

Parse a DSL program, then pretty-print it back. The output should be semantically equivalent.

```bash
cargo test --package limited-core -- pretty::tests::test_pretty_print_empty_program --nocapture
cargo test --package limited-core -- pretty::tests::test_pretty_print_let_statement --nocapture
cargo test --package limited-core -- pretty::tests::test_pretty_print_if_statement --nocapture
```

## Scheduler + Planner Verification

```bash
cargo test --package limited-core -- scheduler::tests --nocapture
```

**Expected:** 30 scheduler tests pass

## Type Checking Verification

```bash
cargo test --package limited-core -- ty::tests --nocapture
```

**Expected:** 98 type check tests pass

## Resource Subsystem Verification

```bash
cargo test --package limited-core -- resource::tests --nocapture
```

**Expected:** 51 resource tests pass

### Subsystems covered
| Subsystem | Tests | What it verifies |
|-----------|-------|-----------------|
| `RuntimeValue` | 6 | Null/Int/Bytes/Resource/List/Struct truthy, type_name, field access |
| `ResourceRegistry` | 5 | register, get, field_value, set_field, list/contains |
| `ExtentPool` | 3 | capacity, allocate, free, remaining, zero alloc, bytes |
| `ExtentEngine` | 5 | device add, inheritance, allocate, constraint check, can_satisfy |
| `MachineRegistry` | 4 | register, get, sets, create/get sets |
| `CostTracker` | 6 | track, query, reset, accumulation, all costs, clear, has_operation |

## Implementation Plan

- `limited-core/src/ast.rs` — All AST types and traits
- `limited-core/src/parser.rs` — Full DSL parser (233 tests)
- `limited-core/src/execute.rs` — Execution engine (101 tests)
- `limited-core/src/ty.rs` — Type checker and policy engine (98 tests)
- `limited-core/src/resource.rs` — Resource subsystem (51 tests)
- `limited-core/src/scheduler.rs` — Cost-aware scheduler (30 tests)
- `limited-core/src/pipeline.rs` — Full pipeline (43 tests)
- `limited-core/src/pretty.rs` — Pretty printer (37 tests)
- `limited-core/src/lib.rs` — Module exports
