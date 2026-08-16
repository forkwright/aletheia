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

pub use config::{
    EgressPolicy, SandboxConfig, SandboxConfigExt, SandboxConfigIssue, SandboxEnforcement,
    SandboxPolicy,
};
pub use policy::{
    EgressDenied, EgressGate, apply_sandbox, check_egress, check_egress_remote_addr,
    probe_landlock_abi,
};
