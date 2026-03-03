//! aletheia-koina — core types, errors, and tracing for Aletheia
//!
//! Koina (κοινά) — "shared things." The common foundation that every crate depends on.
//! Imports nothing from other Aletheia crates. Contains only types, error definitions,
//! and tracing initialization.

pub mod error;
pub mod id;
pub mod tracing_init;

// --- Static assertions: key types are Send + Sync ---
#[cfg(test)]
mod assertions {
    use super::id::*;
    use static_assertions::assert_impl_all;

    assert_impl_all!(NousId: Send, Sync);
    assert_impl_all!(SessionId: Send, Sync);
    assert_impl_all!(TurnId: Send, Sync);
    assert_impl_all!(ToolName: Send, Sync);
}
