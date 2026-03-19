//! Landlock + seccomp + network namespace sandbox for tool execution.
//!
//! Restricts filesystem access via Landlock LSM, blocks dangerous
//! syscalls via seccomp BPF filters, and isolates network access via
//! Linux network namespaces. Applied in child processes after fork,
//! before exec.

mod config;
mod policy;
#[cfg(test)]
mod tests;

pub use config::{EgressPolicy, SandboxConfig, SandboxEnforcement, SandboxPolicy};
pub use policy::{apply_sandbox, probe_landlock_abi};
