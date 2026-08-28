//! Process-wide rustls crypto provider installation.
//!
//! WHY(#7012): first-party binaries disagreed on what an already-installed
//! provider means. `aletheia::main` and `theatron::proskenion` discarded
//! `install_default`'s `Err` as harmless (a dependency may have installed
//! first); `theatron::koilon` called `.expect(...)` on the same result and
//! documented a second installation as a programming error. This module is
//! the one canonical policy both camps now share.
//!
//! `rustls::crypto::CryptoProvider` (0.23) exposes no equality check: it does
//! not derive `PartialEq`, and the one field-level `PartialEq` it does carry
//! transitively (`SupportedCipherSuite`, via `Tls13CipherSuite`/
//! `Tls12CipherSuite`) compares cipher-suite *identifiers* only, not backend
//! identity -- a `ring` provider and an `aws-lc-rs` provider advertising the
//! same suite IDs compare equal by that path. There is therefore no safe way
//! to distinguish "the same provider another component already installed"
//! from "a different, incompatible provider installed first" through the
//! public API. Given that, an already-installed provider is treated as
//! steady state rather than surfaced as an error: every first-party call
//! site installs the identical ring-backed provider, so in practice an
//! already-installed provider in this codebase's own processes is never a
//! different backend, and `install_default`'s own contract already treats a
//! second call as expected rather than exceptional.

/// Outcome of [`install_default_provider`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoProviderInstall {
    /// This call installed the process-wide default provider.
    Installed,
    /// A provider was already installed prior to this call. Treated as
    /// steady state, never a startup failure -- see the module docs for why
    /// this crate cannot (and does not attempt to) verify the prior
    /// installation matches.
    AlreadyInstalled,
}

/// Install the ring-backed rustls crypto provider as the process-wide
/// default.
///
/// Call once at process startup (binary `main`) or lazily before the first
/// TLS-backed client is constructed. Safe to call from multiple sites or
/// multiple times: every outcome is represented, none of them panics.
pub fn install_default_provider() -> CryptoProviderInstall {
    match rustls::crypto::ring::default_provider().install_default() {
        Ok(()) => CryptoProviderInstall::Installed,
        Err(_already_installed) => CryptoProviderInstall::AlreadyInstalled,
    }
}
