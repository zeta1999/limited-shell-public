use std::sync::{Arc, Mutex};
use crate::resource::RuntimeValue;
use crate::ast::SecretSource;

/// Errors that can occur during remote operations.
#[derive(Debug, Clone)]
pub enum RemoteError {
    HostUnknown(String),
    Transport(String),
    CommandFailed {
        command: String,
        exit_code: i32,
        stderr: String,
    },
    PathNotFound(String),
    ParseError(String),
    SecretUnavailable(String),
    Other(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::HostUnknown(host) => write!(f, "host unknown: {host}"),
            RemoteError::Transport(err) => write!(f, "transport failure: {err}"),
            RemoteError::CommandFailed { command, exit_code, stderr } => {
                write!(f, "command '{command}' failed with exit code {exit_code}: {stderr}")
            }
            RemoteError::PathNotFound(path) => write!(f, "path not found: {path}"),
            RemoteError::ParseError(err) => write!(f, "parse error: {err}"),
            RemoteError::SecretUnavailable(err) => write!(f, "secret unavailable: {err}"),
            RemoteError::Other(err) => write!(f, "other error: {err}"),
        }
    }
}

impl std::error::Error for RemoteError {}

pub trait RemoteTransport: Send + Sync {
    fn exec(
        &self,
        machine: &str,
        cmd: &str,
        args: &[String],
    ) -> Result<String, RemoteError>;

    /// Run `cmd` in an interactive PTY session (SSH-style TUI passthrough).
    ///
    /// Default: not supported. Transports that can attach a local TTY should override.
    fn exec_interactive(
        &self,
        _machine: &str,
        cmd: &str,
        _args: &[String],
    ) -> Result<(), RemoteError> {
        Err(RemoteError::Other(format!(
            "interactive exec not supported for command '{cmd}'"
        )))
    }

    fn transfer(
        &self,
        machine: &str,
        src: &str,
        dst: &str,
    ) -> Result<(), RemoteError>;

    fn pull(
        &self,
        machine: &str,
        src: &str,
        dst: &str,
    ) -> Result<(), RemoteError>;

    fn shell(
        &self,
        machine: &str,
        script: &str,
    ) -> Result<String, RemoteError>;

    fn remote_write(
        &self,
        machine: &str,
        value: &RuntimeValue,
        path: &str,
    ) -> Result<(), RemoteError>;

    fn remote_read(
        &self,
        machine: &str,
        path: &str,
    ) -> Result<RuntimeValue, RemoteError>;

    fn set_env(
        &self,
        machine: &str,
        name: &str,
        source: &SecretSource,
    ) -> Result<(), RemoteError>;
}

pub struct NoopTransport;

impl RemoteTransport for NoopTransport {
    fn exec(&self, _machine: &str, _cmd: &str, _args: &[String]) -> Result<String, RemoteError> {
        Ok(String::new())
    }

    fn exec_interactive(
        &self,
        _machine: &str,
        _cmd: &str,
        _args: &[String],
    ) -> Result<(), RemoteError> {
        Ok(())
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
        Ok(RuntimeValue::Struct(std::collections::HashMap::new()))
    }

    fn set_env(&self, _machine: &str, _name: &str, _source: &SecretSource) -> Result<(), RemoteError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RecordedCall {
    Exec { machine: String, cmd: String, args: Vec<String> },
    ExecInteractive { machine: String, cmd: String, args: Vec<String> },
    Transfer { machine: String, src: String, dst: String },
    Pull { machine: String, src: String, dst: String },
    Shell { machine: String, script: String },
    RemoteWrite { machine: String, value: RuntimeValue, path: String },
    RemoteRead { machine: String, path: String },
    SetEnv { machine: String, name: String, source: SecretSource },
}

#[derive(Default, Clone)]
pub struct TestTransport {
    pub calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl TestTransport {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl RemoteTransport for TestTransport {
    fn exec(&self, machine: &str, cmd: &str, args: &[String]) -> Result<String, RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::Exec {
            machine: machine.to_string(),
            cmd: cmd.to_string(),
            args: args.to_vec(),
        });
        Ok(String::new())
    }

    fn exec_interactive(
        &self,
        machine: &str,
        cmd: &str,
        args: &[String],
    ) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::ExecInteractive {
            machine: machine.to_string(),
            cmd: cmd.to_string(),
            args: args.to_vec(),
        });
        Ok(())
    }

    fn transfer(&self, machine: &str, src: &str, dst: &str) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::Transfer {
            machine: machine.to_string(),
            src: src.to_string(),
            dst: dst.to_string(),
        });
        Ok(())
    }

    fn pull(&self, machine: &str, src: &str, dst: &str) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::Pull {
            machine: machine.to_string(),
            src: src.to_string(),
            dst: dst.to_string(),
        });
        Ok(())
    }

    fn shell(&self, machine: &str, script: &str) -> Result<String, RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::Shell {
            machine: machine.to_string(),
            script: script.to_string(),
        });
        Ok(String::new())
    }

    fn remote_write(&self, machine: &str, value: &RuntimeValue, path: &str) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::RemoteWrite {
            machine: machine.to_string(),
            value: value.clone(),
            path: path.to_string(),
        });
        Ok(())
    }

    fn remote_read(&self, machine: &str, path: &str) -> Result<RuntimeValue, RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::RemoteRead {
            machine: machine.to_string(),
            path: path.to_string(),
        });
        Ok(RuntimeValue::Null)
    }

    fn set_env(&self, machine: &str, name: &str, source: &SecretSource) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(RecordedCall::SetEnv {
            machine: machine.to_string(),
            name: name.to_string(),
            source: source.clone(),
        });
        Ok(())
    }
}
