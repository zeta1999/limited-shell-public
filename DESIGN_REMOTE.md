# Remote Execution Layer — Design Document

## Overview

The Limited Shell DSL supports four remote operation types that execute against machines in the registry:

1. **`exec`** — run a command on a remote machine, return stdout
2. **`transfer`** — copy files between local and remote machines (push/pull)
3. **`shell`** — run a command through a remote shell (supports pipes, redirects, multiline)
4. **`remote_write` / `remote_read`** — serialize/deserialize structured data over the network

All remote operations are **abstracted behind traits** so the core execution engine has zero dependency on networking libraries, auth mechanisms, or UX interfaces.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Pipeline                       │
│   parse → type-check → schedule → execute        │
└──────────────────────┬──────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────┐
│         ExecutionContext                        │
│                                                   │
│  scopes ──► variable bindings                     │
│  current_machine ──► "alpha", "beta", ...         │
│  registry ──► ResourceRegistry                    │
│  extent_engine ──► ExtentEngine                   │
│  machine_registry ──► MachineRegistry             │
│  remote ──► &'a dyn RemoteTransport               │
└──────────────────────┬──────────────────────────┘
                       │ calls
┌──────────────────────▼──────────────────────────┐
│       RemoteTransport trait (abstract)           │
│                                                   │
│  exec(), transfer(), pull(), shell()             │
│  remote_write(), remote_read(), set_env()        │
└──────────────────────┬──────────────────────────┘
                       │ implemented by
┌──────────┬───────────┴───────────┬──────────────┐
│ Noop     │  TestTransport        │  RealTransport│
│(default) │  (records calls)      │  (your libs)  │
└──────────┴───────────────────────┴──────────────┘
```

## Trait Definition

**File:** `limited-core/src/remote.rs`

```rust
/// Errors that can occur during remote operations.
///
/// These are converted into `ExecutionError::Remote` by the execution engine.
#[derive(Debug, Clone)]
pub enum RemoteError {
    /// The target machine is not registered or not found.
    HostUnknown(String),
    /// A transport-level failure (connection refused, timeout, auth, etc.).
    Transport(String),
    /// A command exited with a non-zero status.
    CommandFailed {
        command: String,
        exit_code: i32,
        stderr: String,
    },
    /// A file or path does not exist on the remote host.
    PathNotFound(String),
    /// Remote output could not be parsed into the expected type.
    ParseError(String),
    /// A secret source could not be resolved (env var missing, command failed, etc.).
    SecretUnavailable(String),
    /// An unexpected error.
    Other(String),
}

impl std::fmt::Display for RemoteError {
    // ...
}

impl std::error::Error for RemoteError {}
```

```rust
/// Core trait for remote operations.
///
/// This trait abstracts all network and shell interactions.
/// Implementations can use SSH, gRPC, custom TCP, WebSocket, or any other transport.
///
/// The default implementation is `NoopTransport` which does nothing and returns Ok.
///
/// # Security Note
/// Implementations are responsible for all security: authentication, encryption,
/// authorization, and sandboxing. This trait makes no assumptions about any of these.
pub trait RemoteTransport: Send + Sync {
    /// Execute a command on a remote machine and return stdout.
    ///
    /// # Arguments
    /// * `machine` — the machine name (from `MachineRegistry`)
    /// * `cmd` — the command to execute (binary path or command name)
    /// * `args` — command-line arguments
    ///
    /// # Returns
    /// The stdout output as a string, or an error.
    ///
    /// # Implementation note
    /// Implementations may choose to capture stderr separately, log it,
    /// or merge it into stdout. This is left to the implementor.
    fn exec(
        &self,
        machine: &str,
        cmd: &str,
        args: &[String],
    ) -> Result<String, RemoteError>;

    /// Run a command in an interactive PTY session (SSH-style TUI passthrough).
    ///
    /// Invoked by `exec interactive Cmd` in the limited-shell DSL.
    /// Default trait impl returns an error; real transports (e.g. simple-remote)
    /// override to attach a local TTY and forward keys/resize.
    fn exec_interactive(
        &self,
        machine: &str,
        cmd: &str,
        args: &[String],
    ) -> Result<(), RemoteError>;

    /// Push a file from a local path to a remote machine.
    ///
    /// # Arguments
    /// * `machine` — the target machine name
    /// * `src` — the local file path
    /// * `dst` — the remote file path (typically `user@host:path` or just `path` if machine is known)
    fn transfer(
        &self,
        machine: &str,
        src: &str,
        dst: &str,
    ) -> Result<(), RemoteError>;

    /// Pull a file from a remote machine to a local path.
    ///
    /// # Arguments
    /// * `machine` — the source machine name
    /// * `src` — the remote file path
    /// * `dst` — the local file path
    fn pull(
        &self,
        machine: &str,
        src: &str,
        dst: &str,
    ) -> Result<(), RemoteError>;

    /// Execute a command through a remote shell, supporting pipes and redirects.
    ///
    /// Unlike `exec` which calls a binary directly, `shell` passes the full
    /// command string to the remote shell interpreter.
    ///
    /// # Arguments
    /// * `machine` — the target machine name
    /// * `script` — the full shell command or script
    ///
    /// # Returns
    /// The stdout output as a string, or an error.
    fn shell(
        &self,
        machine: &str,
        script: &str,
    ) -> Result<String, RemoteError>;

    /// Write a serialized RuntimeValue to a remote machine path.
    ///
    /// The value is serialized as JSON (or another format decided by the implementor)
    /// and written to the specified path on the remote machine.
    ///
    /// # Arguments
    /// * `machine` — the target machine name
    /// * `value` — the data to write
    /// * `path` — the remote file path
    ///
    /// # Returns
    /// Ok(()) on success.
    fn remote_write(
        &self,
        machine: &str,
        value: &RuntimeValue,
        path: &str,
    ) -> Result<(), RemoteError>;

    /// Read a value from a remote machine path and deserialize it.
    ///
    /// # Arguments
    /// * `machine` — the source machine name
    /// * `path` — the remote file path
    ///
    /// # Returns
    /// A RuntimeValue parsed from the remote file, or an error.
    fn remote_read(
        &self,
        machine: &str,
        path: &str,
    ) -> Result<RuntimeValue, RemoteError>;

    /// Set an environment variable on a remote machine via a secret source.
    ///
    /// # Arguments
    /// * `machine` — the target machine name
    /// * `name` — the environment variable name
    /// * `source` — how to obtain the secret value:
    ///   - `SecretSource::Env(ident)` — read from a local env var whose name is `ident`
    ///   - `SecretSource::CmdSecret { machine }` — run a command on `machine` to extract the secret
    fn set_env(
        &self,
        machine: &str,
        name: &str,
        source: &crate::ast::SecretSource,
    ) -> Result<(), RemoteError>;
}
```

## `NoopTransport` (Default Implementation)

A no-op implementation that does nothing and returns `Ok` for every call.
This ensures all existing tests continue to pass when no real transport is injected.

```rust
/// Default transport: does nothing.
///
/// Used when no networking layer is configured. All remote operations
/// are essentially no-ops, making this suitable for testing, validation,
/// and "plan-only" mode.
pub struct NoopTransport;

impl RemoteTransport for NoopTransport {
    fn exec(&self, _machine: &str, _cmd: &str, _args: &[String]) -> Result<String, RemoteError> {
        Ok(String::new())
    }

    fn transfer(&self, _machine: &str, _src: &str, _dst: &str) -> Result<(), RemoteError> {
        Ok(())
    }

    fn pull(&self, _machine: &str, _src: &str, _dst: &str) -> Result<(), RemoteError> {
        Ok(())
    }

    fn shell(&self, _machine: &str, _script: &str) -> Result<String, RemoteError> {
        Ok(String::new())
    }

    fn remote_write(&self, _machine: &str, _value: &RuntimeValue, _path: &str) -> Result<(), RemoteError> {
        Ok(())
    }

    fn remote_read(&self, _machine: &str, _path: &str) -> Result<RuntimeValue, RemoteError> {
        Ok(RuntimeValue::Null)
    }

    fn set_env(&self, _machine: &str, _name: &str, _source: &crate::ast::SecretSource) -> Result<(), RemoteError> {
        Ok(())
    }
}
```

## `TestTransport` (For Testing)

Records all calls and their arguments so tests can verify what would have been sent to the network.

```rust
/// Transport that records all calls for test assertions.
///
/// Usage in tests:
///   let transport = TestTransport::new();
///   ctx = ExecutionContext::with_transport(transport.clone());
///   execute_op_statement(&exec_stmt, &mut ctx);
///   assert_eq!(transport.calls().len(), 1);
///   assert!(matches!(transport.calls()[0], ExecCall { cmd: "deploy", .. }));
#[derive(Default, Clone)]
pub struct TestTransport {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

#[derive(Debug, Clone)]
pub enum RecordedCall {
    Exec { machine: String, cmd: String, args: Vec<String> },
    Transfer { machine: String, src: String, dst: String },
    Pull { machine: String, src: String, dst: String },
    Shell { machine: String, script: String },
    RemoteWrite { machine: String, value: RuntimeValue, path: String },
    RemoteRead { machine: String, path: String },
    SetEnv { machine: String, name: String, source: crate::ast::SecretSource },
}
```

## `ExecutionContext` Changes

Add a `remote` field and a `with_transport` constructor:

```rust
pub struct ExecutionContext<'a> {
    scopes: Vec<HashMap<String, RuntimeValue>>,
    current_machine: Option<String>,
    registry: &'a ResourceRegistry,
    extent_engine: &'a ExtentEngine,
    machine_registry: &'a MachineRegistry,
    /// Remote transport for exec/transfer/shell operations.
    remote: &'a dyn RemoteTransport,
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
            remote: &NoopTransport, // default: no-op
        }
    }

    /// Create a context with a custom remote transport.
    pub fn with_transport(
        registry: &'a ResourceRegistry,
        extent_engine: &'a ExtentEngine,
        machine_registry: &'a MachineRegistry,
        remote: &'a dyn RemoteTransport,
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

    /// Returns true if a real (non-noop) transport is configured.
    pub fn has_real_transport(&self) -> bool {
        // Compare trait object pointer; NoopTransport has a stable address
        std::ptr::eq(
            self.remote as *const dyn RemoteTransport,
            &NoopTransport as *const NoopTransport as *const dyn RemoteTransport,
        )
    }
}
```

## Execution Handler Changes

### `ExecCommand`

```rust
ast::OperationStatement::ExecCommand { cmd, args } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    let args: Vec<String> = args.iter().map(|a| {
        let v = eval_expr(a, ctx)?;
        Ok(runtime_to_string(&v))
    }).collect::<Result<_, _>>()?;

    ctx.remote
        .exec(target, &cmd.name, &args)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(())
}
```

### `Transfer`

```rust
ast::OperationStatement::Transfer { from, machine, location } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    let from_val = eval_expr(from, ctx)?;
    let from_str = match from_val {
        RuntimeValue::StringVal(s) => s,
        RuntimeValue::Resource(_, fields) => fields.get("path")
            .and_then(|v| v.as_string()).unwrap_or_default(),
        _ => return Err(ExecutionError::ExpectedString(from_val.type_name().to_string())),
    };

    let location_val = eval_expr(location, ctx)?;
    let location_str = match location_val {
        RuntimeValue::StringVal(s) => s,
        _ => return Err(ExecutionError::ExpectedString(location_val.type_name().to_string())),
    };

    // Transfer is a push: from local → remote machine
    ctx.remote
        .transfer(target, &from_str, &location_str)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(())
}
```

### `ShellCmd`

```rust
ast::OperationStatement::ShellCmd { cmd, args } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    let full_command = if args.is_empty() {
        cmd.clone()
    } else {
        format!("{} {}", cmd, args.join(" "))
    };

    ctx.remote
        .shell(target, &full_command)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(())
}
```

### `RemoteWrite` (in `TaskItem`)

```rust
ast::TaskItem::RemoteWrite { variable, machine: machine_name, path } => {
    let val = ctx.lookup(&variable.name)
        .cloned()
        .ok_or(ExecutionError::UndefinedVariable(variable.name.clone()))?;

    let machine = resolve_machine_ident(machine_name, ctx)?;
    let path_val = eval_expr(path, ctx)?;
    let path_str = match path_val {
        RuntimeValue::StringVal(s) => s,
        _ => path_val.to_string(),
    };

    ctx.remote
        .remote_write(&machine, &val, &path_str)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(())
}
```

### `SetEnv` (in `FunctionStatement`)

```rust
ast::FunctionStatement::SetEnv { name, secret } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    ctx.remote
        .set_env(target, &name.name, secret)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(None)
}
```

### `ReadJson` / `WriteJson`

```rust
ast::FunctionStatement::ReadJson { var } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    // Read from a default path derived from var name or a config
    let path = format!("/tmp/{}.json", var.name);
    let value = ctx.remote
        .remote_read(target, &path)
        .map_err(|e| ExecutionError::Remote(e))?;
    ctx.bind(var.name.clone(), value);
    Ok(None)
}

ast::FunctionStatement::WriteJson { value } => {
    let target = ctx.get_machine()
        .ok_or(ExecutionError::NoMachineContext)?;

    let resolved = eval_expr(value, ctx)?;
    // Write to a default path derived from variable name
    let path = format!("/tmp/output_{}.json", resolved.type_name().to_lowercase());
    ctx.remote
        .remote_write(target, &resolved, &path)
        .map_err(|e| ExecutionError::Remote(e))?;
    Ok(None)
}
```

## `ExecutionError` Extension

Add a variant to `ExecutionError` in `execute.rs`:

```rust
pub enum ExecutionError {
    // ... existing variants ...
    /// A remote operation failed.
    Remote(RemoteError),
}
```

## `Pipeline` Changes

In `pipeline.rs`, the `run` method needs to accept or construct a `RemoteTransport`:

```rust
impl<'a> Pipeline<'a> {
    /// Run the full pipeline with a custom remote transport.
    pub fn run_with_transport(
        &self,
        source: &str,
        remote: &'a dyn RemoteTransport,
    ) -> Result<PipelineResult, PipelineError> {
        // ... parse, type-check, schedule ...
        let execution = self.execute_program_with_transport(&program, remote)?;
        Ok(PipelineResult { items, program, schedules, execution })
    }

    /// Run the full pipeline with the default no-op transport.
    pub fn run(&self, source: &str) -> Result<PipelineResult, PipelineError> {
        self.run_with_transport(source, &NoopTransport)
    }
}
```

## Module Structure

```
limited-core/src/
├── lib.rs              # pub mod remote;
├── remote.rs           # RemoteTransport trait, RemoteError, NoopTransport, TestTransport
├── execute.rs          # ExecutionContext.add(remote), handler changes, ExecutionError::Remote
├── pipeline.rs         # run_with_transport() method
├── parser.rs           # (no changes - already parses exec/transfer/shell)
├── ast.rs              # (no changes - enums already defined)
├── ty.rs               # (no changes - type checking already validates these)
└── pretty.rs           # (no changes - pretty printing already works)
```

## Serialization Strategy

### RuntimeValue → JSON (for `remote_write`)

```rust
impl RuntimeValue {
    /// Serialize this value as a JSON string.
    pub fn to_json(&self) -> String {
        // Convert RuntimeValue → serde_json::Value → String
        // Null → null
        // Bool → true/false
        // Int → number
        // Bytes → number (bytes as integer)
        // StringVal → string
        // Resource("Name", fields) → {"type": "Name", ...fields}
        // List → [items...]
        // Struct → {"key": value, ...}
    }
}
```

### JSON → RuntimeValue (for `remote_read`)

```rust
impl RuntimeValue {
    /// Deserialize a JSON string into a RuntimeValue.
    pub fn from_json(json: &str) -> Result<Self, RemoteError> {
        // serde_json::Value → RuntimeValue
        // Handle type tags for Resource values: {"type": "Name", ...}
    }
}
```

## SecretSource Resolution

The `set_env` method receives a `SecretSource` which must be resolved to an actual string value:

```rust
pub enum SecretSource {
    /// Read from a local environment variable.
    Env(Ident),       // e.g., Env("API_KEY") → std::env::var("API_KEY")
    /// Extract secret by running a command on a specific machine.
    CmdSecret { machine: Ident },  // e.g., CmdSecret { machine: "secrets-box" }
}
```

Resolution strategy in `RemoteTransport::set_env`:

```rust
fn set_env(&self, machine: &str, name: &str, source: &SecretSource) -> Result<(), RemoteError> {
    let value = match source {
        SecretSource::Env(ident) => std::env::var(&ident.name)
            .map_err(|_| RemoteError::SecretUnavailable(format!(
                "env var {} not found", ident.name
            ))),
        SecretSource::CmdSecret { machine: src_machine } => {
            // Run a command on the secret machine to extract the value
            // The command and arguments are left to the implementor to define
            // A common pattern: exec on the secret machine, capture the last line
            let output = self.exec(&src_machine.name, "cat", &["/run/secrets/".to_string() + &ident.name])?;
            Ok(output.trim().to_string())
        }
    }?;
    // Now push the secret to the target machine
    // Implementation-specific: SSH key injection, env file write, etc.
    self.push_secret(machine, name, &value)
}
```

## What This Document Does NOT Cover

These are intentionally out of scope and will be addressed when networking libraries are provided:

- **Authentication**: SSH keys, mTLS, JWT, OIDC, PKI — up to `RealTransport` implementor
- **Encryption**: TLS, IPsec, custom encryption — up to `RealTransport` implementor
- **Connection management**: pooling, keep-alive, reconnect — up to `RealTransport` implementor
- **UX layer**: CLI commands, TUI, HTTP API, WebSocket — separate crate/module
- **Distributed mode**: peer-to-peer coordination, leader election — separate concern
- **Sandboxing**: container isolation, seccomp, namespaces — handled separately
- **Secret sharing mechanisms**: Shamir's Secret Sharing, threshold crypto — handled separately

## Testing

| Test | Transport | What it verifies |
|------|-----------|-----------------|
| All existing tests | `NoopTransport` (default) | Nothing breaks when no transport is provided |
| `TestTransport` recording | `TestTransport` | Correct machine, cmd, args are passed |
| `TestTransport` errors | `TestTransport` with error injection | ExecutionError::Remote is raised correctly |
| Integration tests | `RealTransport` (future) | End-to-end with actual networking |

## Summary of Changes

| File | Lines Added | Lines Modified | Purpose |
|------|------------|---------------|---------|
| `remote.rs` (NEW) | ~200 | — | Trait, error, NoopTransport, TestTransport |
| `execute.rs` | ~30 | ~50 | `ExecutionContext.remote` field, handler implementations |
| `pipeline.rs` | ~15 | ~10 | `run_with_transport()` method |
| `lib.rs` | 1 | — | `pub mod remote;` |
| `execute.rs` error enum | 1 | — | `ExecutionError::Remote(RemoteError)` variant |
