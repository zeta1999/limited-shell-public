//! Core types, AST, parser, and expression handling for the Limited Shell language.
//!
//! # Modules
//!
//! - [`ast`]     — Abstract syntax tree nodes
//! - [`parser`]  — Recursive descent parser (converts LS source to AST)
//! - [`pretty`]  — Pretty printer (round-trips AST back to source)
//! - [`ty`]      — Type system, environment, and type checking
//! - [`resource`] — Runtime resource management, extent engine, machine registry, cost tracking
//! - [`scheduler`] — Cost-aware operation planning and machine assignment
//! - [`pipeline`]  — Full pipeline: parse → type-check → plan → execute

pub mod ast;
pub mod execute;
pub mod parser;
pub mod pipeline;
pub mod pretty;
pub mod remote;
pub mod resource;
pub mod scheduler;
pub mod ty;
