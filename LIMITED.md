# Limited Shell Language — Complete Specification

**Version:** 0.1.0-draft
**Status:** Specification Draft
**Implementation Target:** Rust

---

## Table of Contents

1. [Overview](#1-overview)
2. [Type System](#2-type-system)
3. [Lexical Elements](#3-lexical-elements)
4. [Resource Declarations](#4-resource-declarations)
5. [Device Declarations](#5-device-declarations)
6. [Machine Declarations](#6-machine-declarations)
7. [Role System](#7-role-system)
8. [Policy Engine](#8-policy-engine)
9. [Operations](#9-operations)
10. [Services](#10-services)
11. [Task Blocks](#11-task-blocks)
12. [Functions](#12-functions)
13. [Control Flow](#13-control-flow)
14. [Collections](#14-collections)
15. [Data Formats](#15-data-formats)
16. [Distributed Execution](#16-distributed-execution)
17. [Plugin System](#17-plugin-system)
18. [Secret Management](#18-secret-management)
19. [Governance](#19-governance)
20. [Configuration & Scoping](#20-configuration--scoping)
21. [Process Tree & Lifecycle](#21-process-tree--lifecycle)

---

## 1. Overview

The Limited Shell (LS) is a domain-specific language for expressing **capabilities-based**, **multi-machine** computational workflows with:

- **Role hierarchies** — users map to roles with parent/child relationships
- **Granular access policies** — fine-grained, conditional permissions on resources
- **Resource/cost management** — explicit accounting for GPU VRAM, RAM, disk, and abstract computational extents
- **Distributed execution** — task graphs spanning heterogeneous machines
- **Policy verification** — all operations checked against the active policy before execution
- **Pluggable scheduling** — configurable scheduler and semaphore-based resource control

The system consists of:

| Component | Responsibility |
|-----------|----------------|
| **Parser/Tokenizer** | Lexes and parses LS source into an AST |
| **Type Checker** | Validates types, capabilities, role hierarchies |
| **Policy Engine** | Evaluates `can`/`cannot` rules at runtime and compile time |
| **Resource Manager** | Tracks extents, costs, device allocations |
| **Scheduler** | Schedules task blocks, optimizes for cost functions |
| **Executor** | Executes operations locally or remotely via pairing |
| **Plugin Registry** | Hosts pluggable schedulers, semaphores, backends |

---

## 2. Type System

### 2.1 Primitive Types

| Type | Description | Literals |
|------|-------------|----------|
| `String` | UTF-8 string | `"hello"`, `"$HOME/.config"` |
| `Bytes` | Unsigned 64-bit integer, byte semantics | `128GB`, `1TB`, `512MB` |
| `Duration` | Time span | `5s`, `30min`, `2h` |
| `Int` | Signed integer | `42`, `-7` |
| `Bool` | Boolean | `true`, `false` |
| `FilePath` | Typed path string; validated as POSIX path | `/mnt3/w4/z1/data.bin` |
| `Node` | Machine identifier; validated as hostname/IP | `salamander`, `nas` |
| `Role` | Role identifier | `Emperor`, `Shogun` |
| `Secret` | Opaque secret type; never printed or logged | hidden |
| `JSON` | Parsed JSON value | `{ "key": "val" }` |

### 2.2 Composite Types

```ls
[list Type]                      — immutable list
[mut list Type]                  — mutable list
[map IndexType to ContentType]   — hash map
[ordered map IndexType to ContentType] — sorted by key
[set Type]                       — unique elements, hash set
[ordered set Type]               — unique, sorted
```

### 2.3 Type Inference

Types are inferred from context when `type:` annotation is omitted. The compiler requires inference to be unambiguous.

```ls
let x = { a: 1, b: 2 }          // x inferred as [map String to Int]
let y: [set String] = { "a", "b" }  // explicit annotation
```

### 2.4 Aliases

```ls
alias machine = Node;
alias path = FilePath;
alias role = Role;
```

Aliases are fully interchangeable with their base types.

---

## 3. Lexical Elements

### 3.1 Identifiers

- Alphanumeric and underscore, must start with letter or underscore
- Case-sensitive
- Reserved words: `role`, `resource`, `device`, `machine`, `operation`, `service`, `function`, `can`, `cannot`, `grant`, `requires`, `allow`, `if`, `and`, `or`, `not`, `is`, `in`, `from`, `on`, `exec`, `batch`, `interactive`, `transfer`, `read`, `write`, `let`, `mut`, `for`, `while`, `break`, `continue`, `return`, `optimize`, `costs`, `cost`, `extends`, `default`, `type`, `key`

### 3.2 Comments

```ls
// single-line comment

/* multi-line
   comment */
```

### 3.3 String Literals

- Double-quoted: `"hello world"`
- Variable interpolation: `"$HOME/.x/y"` — variables are expanded at evaluation time
- Raw strings: `r"no-interp $HOME"`
- Escape sequences: `\n`, `\t`, `\\`, `\"`

### 3.4 Numeric Literals

```ls
1024                      // Int
128GB                     // Bytes (KiB, MiB, GiB, TiB supported)
5s                        // Duration
3.14                      // Float (if needed)
```

Byte suffixes: `KB` (1000), `KiB` (1024), `MB`, `MiB`, `GB`, `GiB`, `TB`, `TiB`.

---

## 4. Resource Declarations

Resources represent entities that can be operated upon. They define:
- **Capacities** — granular operation tags
- **Fields** — struct-like members

### 4.1 Syntax

```ls
resource TypeName {
    capacity Read,
    capacity Write,
    capacity Transfer {
        machine Node,
        location FilePath,
    }
    field location FilePath,
    field machine Node,
    field owner Role,
}
```

### 4.2 Semantics

- `capacity OpName` — declares that the resource type supports operation `OpName` as a capability tag
- `capacity OpName { arg1 Type1, arg2 Type2 }` — declares a parameterized operation
- `field name Type` — declares a struct field
- `field name Type, default = expression` — field with default value
- Resources are type-safe: only fields declared in the resource can be accessed

### 4.3 Examples

```ls
// Simple resource
resource ConfigFile {
    capacity Read,
    capacity Write,
    field location FilePath,
    field owner Role,
}

// Resource with transfer capability
resource File {
    capacity Read,
    capacity Write,
    capacity Transfer {
        machine Node,
        location FilePath,
    }
    field location FilePath,
    field machine Node,
    field owner Role,
}
```

---

## 5. Device Declarations

Devices are specialized resources with **extent constraints** and **cost rules** for resource management (GPU, RAM, disk, network).

### 5.1 Base Device

```ls
device DeviceType {
    extent NVRAM bytes,              // named extent
    extent SharedRAM bytes, default = NVRAM,  // with default inference
    rate bandwidth bytes/sec,         // rate-limited extent

    cost rule {
        // Cost rules express constraints
        sum(cost NVRAM) <= NVRAM,
        sum(cost SharedRAM) <= SharedRAM,
        sum(cost NVRAM) + sum(cost RAM) <= SharedRAM,
    }
}
```

### 5.2 Extents

An **extent** is a quantifiable resource pool:
- `extent NVRAM bytes` — named pool of bytes
- `extent SharedRAM bytes, default = NVRAM` — infers default from another extent
- `extent DISK name bytes, mountpoint path` — disk with filesystem mount point
- `rate bandwidth bytes/sec` — rate-limited extent (throughput)

### 5.3 Cost Rules

Cost rules are invariants enforced by the scheduler:
- `sum(cost extent) <= pool` — total consumption fits within pool
- `cost item extent = amount` — per-item consumption (applied when allocating)
- Cost rules are evaluated before every `exec` in a task block

### 5.4 Device Inheritance

```ls
device GPU {
    extent NVRAM bytes,
    cost rule {
        sum(cost NVRAM) <= NVRAM,
    }
}

// Inheritance with ':'
device MACGPU : GPU {
    extent SharedRAM bytes, default = RAM.size,
    extent NVRAM, default = SharedRAM,
    cost rule {
        sum(cost NVRAM) <= NVRAM,
        sum(cost NVRAM) <= SharedRAM,
        sum(cost NVRAM) + sum(cost RAM) <= SharedRAM,
    }
}
```

### 5.5 Built-in Devices

| Device | Extents | Notes |
|--------|---------|-------|
| `CPU` | `RAM` | Default extent inference from context |
| `GPU` (CUDA) | `NVRAM`, `PCIe` | NVIDIA-specific |
| `MACGPU` | `SharedRAM`, `NVRAM` | Apple Silicon unified memory |
| `Disk` | `capacity bytes`, `IOPS` | Block storage |
| `Network` | `bandwidth bytes/sec`, `latency Duration` | Network interface |
| `QRNG` | `rate bytes/sec` | Quantum random number generator |

---

## 6. Machine Declarations

Machines (nodes) are declared with their resources and devices.

### 6.1 Syntax

```ls
Machine MachineName {
    extent RAM 128GB,
    extent DISK DISK1 1TB mountpoint /,
    extent DISK DISK2 1TB mountpoint /nas,
    key "xlqwkjfoeqhrgehg",           // machine credential/key
    device cpu type CPU,              // device with inferred extent
    device gpu type MACGPU,           // MAC GPU, infers SharedRAM from context
}
```

### 6.2 Machine Sets

```ls
// Named set of machines
machine set ClusterA {
    machine a1,
    machine a2,
    machine a3,
}
```

### 6.3 Inference

Devices without explicit extent bindings inherit from context:
- `device cpu type CPU` — uses `RAM` extent by default
- `device gpu type MACGPU { SharedRAM = 64GB }` — explicit override

---

## 7. Role System

Roles represent **capabilities-bound identities** with hierarchical parent relationships.

### 7.1 Syntax

```ls
role RoleName {
    up ParentRole,

    // Read access with conditions
    can Read {x:File} if x.machine is salamander,

    // Write denial
    cannot Write {x:File},

    // Parameterized transfer capability
    can Transfer {x:File} {
        machine y:Node,
        location z:FilePath
    }
    if can Read x
    and y.owner is Emperor or down,
    and z starts with "/mnt2/w1/x1/"
    and x starts with "/mnt3/w4/z1/"
    and drop "/mnt3/w4/z1/" z is drop "/mnt3/w4/z1/" x,

    // Operation definition rights
    can define operation for Musashi, down, Musashi.down,
}
```

### 7.2 Semantics

| Clause | Meaning |
|--------|---------|
| `up ParentRole` | This role is a child of `ParentRole`; inherits all parent permissions |
| `can Op {x:Resource} if condition` | Grant capability with preconditions |
| `cannot Op {x:Resource}` | Explicit deny (overrides inheritance) |
| `can define operation for RoleList` | Allows defining new operations on behalf of listed roles |

### 7.3 Role Hierarchy Resolution

The `down` keyword refers to all children (recursive) of the current role:

| Expression | Expands to |
|------------|------------|
| `is Emperor` | exactly Emperor |
| `is Emperor or down` | Emperor or any descendant role |
| `down` | all direct children |
| `Musashi.down` | all descendants of Musashi |

### 7.4 Granting Rights Separately

```ls
// A role with define-operation rights grants a specific permission
grant Musashi can Write {x:File} if x.machine is muramasa;
```

The granting role must itself have the capability to grant the permission (transitive check).

---

## 8. Policy Engine

The policy engine is the core enforcement mechanism. It evaluates policies **at two levels**:

1. **Compile-time (static analysis)** — verify that all operations in a script are permissible under the declared roles
2. **Runtime** — check dynamic conditions (e.g., "machine x is available", "file exists")

### 8.1 Policy Evaluation Order

1. **Denies first** — `cannot` rules are checked before `can` rules
2. **Most specific match wins** — rules matching the exact resource type and field values take precedence
3. **Inheritance resolves upward** — if no local rule, check parent roles
4. **Default deny** — if no rule permits an action, it is denied

### 8.2 Policy Query Language

The policy engine supports these query patterns:

```
can Op {x:Resource}                              // Can role X perform Op on resource X?
can Op {x:Resource} for Role R                  // Can role R perform Op on resource X?
has-capability Role Op                          // Does Role have the Op capability (abstract)?
resolve-role Role                               // Resolve all effective permissions for Role
check-condition condition-string                // Evaluate arbitrary policy condition
```

### 8.3 Condition Syntax

Conditions in `can`/`cannot` clauses use:

| Operator | Meaning |
|----------|---------|
| `is` | Identity/equality |
| `or` | Logical disjunction |
| `and` | Logical conjunction |
| `not` | Negation |
| `starts with "prefix"` | String prefix check |
| `ends with "suffix"` | String suffix check |
| `drop "prefix" a is drop "prefix" b` | Structural equality after prefix removal |
| `in set S` | Membership check |
| `exists` | Existence check |
| `matches regex` | Regex match |

---

## 9. Operations

Operations are **reusable, parameterized computations** with preconditions and execution options.

### 9.1 Syntax

```ls
operation OpName { param1: Type1, param2: Type2 } {
    requires can Read f,
    requires can ExecuteOn f.machine { user ajax },
    allow if role is Kerai or down,

    options {
        "local" {
            on f.machine
            exec cp {f} ~/.x
        }
        "set" {
            choose { machine: Node }
            let tmp = tempfile { machine } { f }
            requires can Transfer f { machine: machine, location: tmp }
            transfer f { machine: machine, location: tmp }
            on machine
            exec cp {tmp} ~/.x
        }
    }

    cost {
        // Resource costs for this operation
        GPUVRAM model.vramsize
        start 30s
        stop 5s
    }
}
```

### 9.2 Semantics

| Clause | Meaning |
|--------|---------|
| `requires condition` | Precondition; must hold before any option can execute |
| `allow if role is R or down` | Which roles may invoke this operation |
| `options { "name" { ... } }` | Alternative execution strategies; scheduler picks one |
| `cost { ... }` | Resource consumption profile |

### 9.3 Built-in Operations

| Operation | Description |
|-----------|-------------|
| `exec [batch\|interactive] Cmd args...` | Execute a shell command; inherits current role capabilities. Omit mode or use `batch` (default) to capture stdout/stderr with no TTY. Use `interactive` for an opt-in PTY/TUI session (e.g. remote `mc`) when the transport supports it. |
| `transfer src { machine m, location dest }` | Copy file to remote machine |
| `read json input as var` | Read and parse JSON input into variable |
| `write json output value` | Write value as JSON output |
| `transfer src to dest` | Local file copy |
| `tempfile { machine m } { f }` | Allocate a temporary file on machine `m`, same type as `f` |

### 9.4 Operation Resolution

When multiple options exist, the scheduler selects based on:
1. `optimize for time` — minimize wall-clock time
2. `optimize for RAM` — minimize memory usage
3. `optimize for cost` — minimize resource cost
4. Default — first matching option

---

## 10. Services

Services are **long-running computational units** bound to machines, with resource costs.

### 10.1 Syntax

```ls
service ServiceName { param: Type } on machine m {
    costs {
        GPUVRAM model.vramsize
        start 30s
        stop 5s
        RAM 8GB
    }
}
```

### 10.2 Semantics

- Services persist across operations on the same machine
- The scheduler tracks service lifetimes and resource usage
- `costs` declare the resource footprint; the scheduler enforces these as constraints
- Services can be started, stopped, and queried for status

### 10.3 Dependency Declaration

```ls
// Within a task block or operation option:
dependency service llm { Qwen3.6 } on machine xyz as s1
```

Dependencies are resolved by the scheduler before task execution.

---

## 11. Task Blocks

Task blocks express **parallel, distributed computation graphs**.

### 11.1 Syntax

```ls
on machine set { t1, t2, t3 }:
tasks {
    @1 <- File { machine: nas, location: /a/b/c/d },   // bind @1 to file on nas
    coco @1 @2                                          // run operation coco; result -> @2
    coco @2 @3                                          // run operation coco; result -> @3
    $HOME/xxx on machine z1 <- @3                       // write @3 to remote path
    optimize for time
    optimize for RAM
}
```

### 11.2 Semantics

| Syntax | Meaning |
|--------|---------|
| `@var` | Task output / data binding variable |
| `@var <- ResourceSpec` | Bind a variable to a resource (file, data) |
| `op @a @b` | Invoke operation; @b receives output |
| `var on machine m <- @b` | Write variable to machine `m` |
| `optimize for metric` | Add optimization constraint for the scheduler |

### 11.3 Scheduling

The scheduler:
1. **Analyzes dependencies** between task bindings (`@` variables)
2. **Resolves machine assignments** (explicit or chosen via `choose`)
3. **Checks cost constraints** against machine capabilities
4. **Evaluates policy** for every operation
5. **Produces an execution plan** respecting parallelism and resource limits
6. **Dispatches** operations to executors

### 11.4 Choice Expression

```ls
choose { machine: Node }                    // scheduler picks a machine
choose { xyz: Node } from set gpullmm       // pick from a named set
```

---

## 12. Functions

Functions are **reusable, composable scripts** with typed parameters and explicit success/failure criteria.

### 12.1 Syntax

```ls
function FunctionName { param1: Type1, param2: Type2 } {
    requires precondition,
    allow if role is R or down,

    // Execution body can span multiple machines
    on machine m2
    setenv VAR { secret: cmd_secret for machine m2 }
    exec cmd --arg { param1 }
    read json output as local_var

    on machine m1
    write json on input { key: local_var.key }
    exec cmd --accept-pair
    read json output as result

    success if result.status == true;
    failure otherwise;
}
```

### 12.2 Semantics

- `on machine m` — switches execution context to machine `m`
- `setenv VAR { secret: cmd_secret for machine m }` — injects a secret into the environment
- `read json output as var` — captures stdout as parsed JSON
- `write json on input value` — writes value to stdin as JSON
- `success if condition` / `failure otherwise` — defines what constitutes success/failure
- Variables are **lexical-scoped** within the function
- `requires` checks are evaluated before the function body begins

### 12.3 Example: Pairing Function

```ls
function pair { m: Node } { m2: Node } {
    requires m can Pair { machine: m2 }

    // Phase 1: Initiate pairing on m2
    on machine m2
    setenv SECRET1 { secret: cmd_secret for machine m2 }
    exec cmd --pair
    read json output as o1

    // Phase 2: Accept pairing on m1
    on machine m1
    setenv SECRET1 { secret: cmd_secret for machine m1 }
    write json on input { key: o1.key }
    exec cmd --accept-pair
    read json output as o2

    // Phase 3: Verify
    on machine m2
    setenv SECRET1 { secret: cmd_secret for machine m2 }
    exec cmd --check-paired { m1.keyx }
    read json output as o3

    success if o2.status == true and o3.status == true;
    failure otherwise;
}
```

---

## 13. Control Flow

### 13.1 For Loops

```ls
// List iteration
for i in items {
    // body
}

// Dictionary iteration
for k, v in dict {
    // k is key, v is value
}
```

### 13.2 While Loops with Tell-Request Pattern

```ls
while cantellmore result is true {
    tellmemore result a b c

    if thisistheend yyy {
        break   // exit loop
    }
    // continue is implicit at loop end
}
```

### 13.3 Conditionals

```ls
if condition {
    // body
} else if other_condition {
    // body
} else {
    // body
}
```

### 13.4 Return and Break

| Statement | Meaning |
|-----------|---------|
| `return` | Return from function/operation with implicit `true` |
| `return value` | Return with explicit value |
| `break` | Exit innermost loop |
| `continue` | Skip to next iteration |

---

## 14. Collections

### 14.1 Lists

```ls
let items: [String] = { "a", "b", "c" }
let nums: [Int] = new list of Int(5)       // 5-element list, zero-initialized
let mut mitems: [mut String] = { "x" }     // mutable list
```

### 14.2 Maps

```ls
let config: map of String to String = {
    host: "localhost",
    port: 8080,
}

let sorted: ordered map of Int to String = {
    1: "one",
    2: "two",
}
```

### 14.3 Sets

```ls
let keys: set of String = { "a", "b", "c" }
let nums: ordered set of Int = { 3, 1, 2 }  // sorted: { 1, 2, 3 }
```

### 14.4 Access Patterns

```ls
array[index]               // list indexing
map[key]                   // map lookup
map[key] = value           // map assignment
struct.field               // struct field access
```

---

## 15. Data Formats

LS supports direct access to structured data formats with shell-style and jq-like syntax.

### 15.1 Supported Formats

- **JSON** — primary format for operation I/O
- **YAML** — configuration files
- **XML** — legacy system integration
- **CSV** — tabular data

### 15.2 Access Syntax

```ls
// JSON access with jq-like syntax
.read .users[].name            // select names array
.read .config.host             // select single field
.read [0].status               // index into array
```

Data access patterns use `.field`, `[index]`, and `[filter]` notation compatible with jq.

---

## 16. Distributed Execution

### 16.1 Machine Pairing

Two machines establish a secure, authenticated connection via a pairing protocol:

1. **Initiator** on machine `m2`: generates connection string, sends to initiator
2. **Acceptor** on machine `m1`: receives connection string, establishes link
3. **Verification**: both sides confirm the connection via `--check-paired`

### 16.2 Remote Execution Model

```ls
// Syntax variants for remote execution
cmd args                          // runs locally by default
cmd args on machine target        // run on specific machine
cmd args on machine target user role  // run as specific role on target

// File transfer
copy src on src_machine to dst on dst_machine
transfer file { machine m, location dest }
```

### 16.3 Execution Context

```ls
// Switch context
on machine target
// All subsequent exec commands run on target until 'on' changes

// On machine sets (parallel dispatch)
on machine set { m1, m2, m3 }:
broadcast {
    exec some-command --arg
}
```

### 16.4 Secret Propagation

```ls
// Inject secret into execution environment
setenv VAR { secret: cmd_secret for machine m }
// VAR is never logged, never printed, only available as environment variable
```

---

## 17. Plugin System

### 17.1 Plugin Types

| Plugin | Purpose |
|--------|---------|
| **Scheduler** | Custom scheduling strategies (greedy, genetic, constraint-based) |
| **Semaphore** | Resource capacity control and throttling |
| **Backend** | Execution backends (SSH, gRPC, container, native) |
| **Resolver** | Custom machine/role resolution |

### 17.2 Plugin Declaration

```ls
plugin Scheduler custom {
    // plugin config
    strategy "genetic"
    generations 100
    population 50
}

plugin Semaphore gpu_count {
    max_concurrent 4
    queue_timeout 30s
}
```

### 17.3 Plugin API

Plugins expose a well-defined interface:

```rust
trait SchedulerPlugin {
    fn schedule(&self, tasks: &[Task], machines: &[Machine]) -> ExecutionPlan;
    fn optimize(&self, plan: ExecutionPlan, criteria: &[OptimizeFor]) -> ExecutionPlan;
}

trait SemaphorePlugin {
    fn acquire(&self, resource: &Resource) -> Result<Slot>;
    fn release(&self, slot: Slot);
    fn status(&self) -> SemaphoreStatus;
}
```

---

## 18. Secret Management

### 18.1 Secret Types

| Type | Source |
|------|--------|
| `cmd_secret for machine m` | Machine-specific credential from key store |
| `env_secret VAR` | Environment variable |
| `file_secret path` | File-based credential |
| `pkix_cert path` | PKI certificate |
| `shared_secret key` | Pre-shared key |

### 18.2 Security Guarantees

- Secrets are **never logged, printed, or transmitted in plaintext**
- Secrets are scoped to their use (single-operation lifetime by default)
- Secrets are typed as `Secret`; implicit conversion to other types is forbidden
- Secret propagation uses the **cmd_secret** mechanism: derived from a local key store per-machine

### 18.3 Secret Binding

```ls
// Bind a secret to a machine's context
setenv API_KEY { secret: cmd_secret for machine api_server }

// Secrets can be bound to roles
role Admin {
    can access secret for machine db if role is Admin or down,
}
```

---

## 19. Governance

### 19.1 Quota-Based Voting

Governance decisions are made through **quota-based votes** among role holders:

- Each role has a quota weight
- Proposals require a majority of total quota to pass
- Emergency actions require supermajority

### 19.2 Quota Expressions

```ls
// Define quota weights
role Emperor { quota 100 }
role Shogun { quota 50 }
role Kerai { quota 10 }

// Proposal syntax
proposal "Update transfer policy" {
    requires majority of quota
    from Shogun    // proposer
}
```

### 19.3 Policy Updates

Policy updates require:
1. Draft in a sandbox (no immediate effect)
2. Vote confirmation
3. Gradual rollout (canary)
4. Rollback capability

---

## 20. Configuration & Scoping

### 20.1 Global Configuration

```ls
// Root-level settings
root settings {
    default_role Admin,
    default_timeout 30s,
    max_parallelism 8,
    policy_enforcement strict,      // strict | permissive
    secret_store local,             // local | vault | k8s
}
```

### 20.2 Session Context

```ls
role Kerai;          // set current role
on machine titi;     // set current machine
// all subsequent commands run in this context
```

### 20.3 Scope Precedence

1. **Local config** — machine-local settings (highest priority)
2. **Session config** — current role and machine
3. **Remote config** — inherited from parent roles / central policy
4. **Defaults** — built-in system defaults

When used in distributed mode, local config always takes priority over remote configs. Role IDs are managed via PKI.

---

## 21. Process Tree & Lifecycle

### 21.1 Process Tree

Every LS execution creates a process tree:

```
Shell (root process)
├── TaskGroup (parallel unit)
│   ├── Operation (local exec)
│   ├── Operation (remote exec on m1)
│   └── Operation (remote exec on m2)
├── Service (long-running)
│   └── ChildProcess
└── BackgroundTask
```

### 21.2 Lifecycle Events

| Event | Trigger |
|-------|---------|
| `started` | Script/policy loaded and parsed |
| `type-checked` | Static analysis passes |
| `policy-verified` | All operations verified against policy |
| `scheduled` | Execution plan produced |
| `dispatched` | Operations sent to executors |
| `completed` | All tasks finished successfully |
| `failed` | One or more tasks failed |
| `rolled-back` | Failure recovery triggered |

### 21.3 Error Handling

```ls
try {
    // body
} catch Error e {
    // handle specific error
} catch {
    // catch-all
} finally {
    // cleanup (always runs)
}
```

---

## Appendix A: Full Example

```ls
alias machine is Node;

machine set ClusterA {
    machine a1,
    machine a2,
    machine a3,
}

resource File {
    capacity Read,
    capacity Write,
    capacity Transfer { machine Node, location FilePath }
    field location FilePath,
    field machine Node,
    field owner Role,
}

role Shogun {
    up Emperor,
    can Read {x: File} if x.machine is salamander,
    cannot Write {x: File},
    can Transfer {x: File} {
        machine y: Node,
        location z: FilePath
    }
    if can Read x
    and y.owner is Emperor or down,
    and z starts with "/mnt2/w1/x1/"
    and x starts with "/mnt3/w4/z1/"
    and drop "/mnt3/w4/z1/" z is drop "/mnt3/w4/z1/" x,
    can define operation for Musashi, down, Musashi.down,
}

Machine alpha {
    extent RAM 128GB,
    extent DISK DISK1 1TB mountpoint /,
    key "xlqwkjfoeqhrgehg",
    device gpu type MACGPU,
}

operation calamar { f: File } {
    requires can Read f,
    requires can ExecuteOn f.machine { user ajax },
    allow if role is Kerai or down,

    options {
        "local" {
            on f.machine
            exec cp {f} ~/.x
        }
        "set" {
            choose { machine: Node }
            let tmp = tempfile { machine } { f }
            requires can Transfer f { machine: machine, location: tmp }
            transfer f { machine: machine, location: tmp }
            on machine
            exec cp {tmp} ~/.x
        }
    }

    cost {
        // costs declared here
    }
}

// --- Execution ---

role Kerai;
on machine titi;

calamar $HOME/.x/y

on machine set { t1, t2, t3 }:
tasks {
    @1 <- File { machine: nas, location: /a/b/c/d }
    coco @1 @2
    coco @2 @3
    $HOME/xxx on machine z1 <- @3

    optimize for time
    optimize for RAM
}
```

---

## Appendix B: Grammar (EBNF Summary)

```
script        ::= { declaration }
declaration   ::= role_decl | resource_decl | device_decl | machine_decl
                | operation_decl | service_decl | function_decl | alias_decl | grant_decl

role_decl     ::= 'role' IDENT '{' role_body '}'
role_body     ::= { 'up' Role ','? | permission_clause | 'can define operation for' role_list ','? }
permission_clause ::= 'can' | 'cannot' op_name { bind: Resource } ['if' condition] ','?
condition     ::= condition_atom { ('and' | 'or') condition_atom }
condition_atom ::= IDENT 'is' Role { 'or' 'down' }?
                | string 'starts with' string
                | string 'ends with' string
                | 'drop' string IDENT 'is' 'drop' string IDENT
                | IDENT 'in' set_name

resource_decl ::= 'resource' IDENT '{' resource_body '}'
resource_body ::= { capacity_decl | field_decl }
capacity_decl ::= 'capacity' op_name [ '(' param_list ')' ] ','?
field_decl    ::= 'field' IDENT Type [ ',' 'default' '=' expression ] ','?

device_decl   ::= 'device' IDENT [':' ParentDevice] '{' device_body '}'
device_body   ::= { extent_decl | rate_decl | cost_rule }
extent_decl   ::= 'extent' name Type [',' 'default' '=' expression] ','?
cost_rule     ::= 'cost rule' '{' cost_invariant { ',' cost_invariant } '}'

machine_decl  ::= 'Machine' IDENT '{' machine_body '}'
machine_body  ::= { extent_decl | key_decl | device_decl | 'machine' IDENT ','? }

operation_decl::= 'operation' IDENT '(' param_list ')' '{' operation_body '}'
operation_body::= { requires_clause | allow_clause | options_block | cost_block }

function_decl ::= 'function' IDENT '(' param_list ')' '{' function_body '}'
function_body ::= { requires_clause | allow_clause | exec_block | success_clause | failure_clause }

task_block    ::= 'on' machine_ref ':' 'tasks' '{' task_list '}'
task_list     ::= { task_item | optimize_clause }

control_flow  ::= for_loop | while_loop | if_expr | try_block
```

---

## Appendix C: Semantic Requirements Summary

| Requirement | Enforcement Point | Details |
|-------------|-------------------|---------|
| Type safety | Type checker | All variables and expressions must have valid types |
| Capability checks | Policy engine | Every operation checked against active role |
| Cost accounting | Resource manager | Extents and costs tracked per task |
| Role hierarchy | Role resolver | Parent/child resolved transitively |
| Secret hygiene | Executor | Secrets never exposed outside Secret type |
| Denial overrides | Policy engine | `cannot` before `can`, most specific first |
| Default deny | Policy engine | No implicit permissions |
| Local config priority | Config loader | Local settings override remote |
| PKI role management | Auth module | Role IDs validated via PKI in distributed mode |
| Governance votes | Governance module | Quota-based voting for policy changes |
