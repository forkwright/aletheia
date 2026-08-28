//! `reqwest::dns::Resolve` implementation that IS the SSRF/DNS-rebinding
//! checkpoint, not a second opinion alongside it.
//!
//! SECURITY(#5229): installed as a `reqwest::Client`'s DNS resolver, this
//! makes the address a connection actually uses the same address this
//! resolver validated -- there is exactly one DNS lookup on that path, so
//! there is no window between "checked" and "connected" for a rebinding
//! answer to occupy. This is distinct from, and stronger than,
//! [`check_egress`](super::check_egress) run before a request and
//! [`check_egress_remote_addr`](super::check_egress_remote_addr) run after
//! one: those inspect two independent resolutions (or a post-hoc peer
//! report) and can only narrow the gap between them, while a resolver
//! installed here closes it by construction.

use std::net::SocketAddr;
use std::sync::Arc;

use koina::http::{HostResolver, TokioHostResolver, is_private_ip};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::policy::EgressGate;

/// Installs as a `reqwest::ClientBuilder::dns_resolver` so every connection
/// the client makes resolves through -- and is filtered by -- `gate`.
///
/// Applies [`is_private_ip`] unconditionally, independent of `gate`'s
/// policy: [`EgressGate::check_addr`]'s `Allow` arm is an unconditional
/// `Ok(())` by design (no per-address policy under `Allow`), so relying on
/// `check_addr` alone here would validate nothing in the compiled default
/// configuration. This mirrors the unconditional private-peer rejection
/// [`check_egress_remote_addr`](super::check_egress_remote_addr) already
/// documents for every egress mode.
pub struct PolicyDnsResolver {
    gate: Arc<EgressGate>,
    resolver: Arc<dyn HostResolver + Send + Sync>,
}

impl PolicyDnsResolver {
    /// Build a resolver enforcing `gate` over real DNS lookups.
    #[must_use]
    pub fn new(gate: Arc<EgressGate>) -> Self {
        Self::with_resolver(gate, Arc::new(TokioHostResolver))
    }

    /// Build a resolver enforcing `gate` over `resolver`'s lookups.
    ///
    /// Exposed so tests can substitute a resolver that returns a different
    /// answer than an earlier, independent pre-connect check saw -- a
    /// faithful stand-in for DNS rebinding -- without depending on real DNS
    /// or a name a test does not control.
    #[must_use]
    pub fn with_resolver(
        gate: Arc<EgressGate>,
        resolver: Arc<dyn HostResolver + Send + Sync>,
    ) -> Self {
        Self { gate, resolver }
    }
}

fn resolve_error(msg: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other(msg.into()))
}

impl Resolve for PolicyDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let gate = Arc::clone(&self.gate);
        let resolver = Arc::clone(&self.resolver);
        // WHY: reqwest's `Resolve::resolve` receives a bare name, never a
        // port (an explicit URL port, or the scheme default, is applied by
        // the caller after this returns) -- port 0 here reaches every
        // `HostResolver` impl in this tree unused, since none of the policy
        // checks below are port-sensitive.
        let host = name.as_str().to_owned();
        Box::pin(async move {
            gate.check_before_connect()
                .map_err(|e| resolve_error(e.to_string()))?;

            let addrs: Vec<SocketAddr> = resolver
                .resolve_host(&host, 0)
                .await
                .map_err(resolve_error)?;

            if addrs.is_empty() {
                return Err(resolve_error(format!(
                    "DNS resolution returned no addresses for {host}"
                )));
            }

            let mut validated = Vec::with_capacity(addrs.len());
            for addr in addrs {
                if is_private_ip(&addr.ip()) {
                    return Err(resolve_error(format!(
                        "{host} resolved to private/internal address {} (possible DNS \
                         rebinding); refusing to connect",
                        addr.ip()
                    )));
                }
                gate.check_addr(addr.ip())
                    .map_err(|e| resolve_error(e.to_string()))?;
                validated.push(addr);
            }

            let addrs: Addrs = Box::new(validated.into_iter());
            Ok(addrs)
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    use koina::http::ResolveHostFuture;

    use super::super::config::EgressPolicy;
    use super::*;

    #[derive(Default)]
    struct MapResolver(HashMap<String, Vec<SocketAddr>>);

    impl HostResolver for MapResolver {
        fn resolve_host<'a>(&'a self, host: &'a str, _port: u16) -> ResolveHostFuture<'a> {
            let addrs = self.0.get(host).cloned().unwrap_or_default();
            Box::pin(async move { Ok(addrs) })
        }
    }

    fn public_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 0)
    }

    fn name(host: &str) -> Name {
        host.parse().expect("valid dns name")
    }

    #[tokio::test]
    async fn resolves_public_address_under_allow() {
        let mut map = HashMap::new();
        map.insert("example.com".to_owned(), vec![public_addr()]);
        let resolver = PolicyDnsResolver::with_resolver(
            Arc::new(EgressGate::new(EgressPolicy::Allow, &[])),
            Arc::new(MapResolver(map)),
        );

        let addrs: Vec<SocketAddr> = resolver
            .resolve(name("example.com"))
            .await
            .expect("public address must resolve")
            .collect();
        assert_eq!(addrs, vec![public_addr()]);
    }

    #[tokio::test]
    async fn rejects_private_address_even_under_allow() {
        // SECURITY(#5229): `EgressGate::check_addr`'s `Allow` arm is an
        // unconditional pass -- this resolver must not rely on it alone.
        let mut map = HashMap::new();
        map.insert(
            "rebind.example".to_owned(),
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
                0,
            )],
        );
        let resolver = PolicyDnsResolver::with_resolver(
            Arc::new(EgressGate::new(EgressPolicy::Allow, &[])),
            Arc::new(MapResolver(map)),
        );

        let err = resolver
            .resolve(name("rebind.example"))
            .await
            .map(|_| ())
            .expect_err("private address must be rejected regardless of egress=allow");
        assert!(err.to_string().contains("private/internal"));
    }

    #[tokio::test]
    async fn rejects_empty_resolution_under_allowlist() {
        let resolver = PolicyDnsResolver::with_resolver(
            Arc::new(EgressGate::new(
                EgressPolicy::Allowlist,
                &["10.0.0.0/8".to_owned()],
            )),
            Arc::new(MapResolver::default()),
        );

        let err = resolver
            .resolve(name("example.com"))
            .await
            .map(|_| ())
            .expect_err("an empty DNS answer must not vacuously pass");
        assert!(err.to_string().contains("no addresses"));
    }

    #[tokio::test]
    async fn rejects_unlisted_address_under_allowlist() {
        let mut map = HashMap::new();
        map.insert("example.com".to_owned(), vec![public_addr()]);
        let resolver = PolicyDnsResolver::with_resolver(
            Arc::new(EgressGate::new(
                EgressPolicy::Allowlist,
                &["10.0.0.0/8".to_owned()],
            )),
            Arc::new(MapResolver(map)),
        );

        let err = resolver
            .resolve(name("example.com"))
            .await
            .map(|_| ())
            .expect_err("address outside the allowlist must be rejected");
        assert!(err.to_string().contains("allowlist"));
    }

    #[tokio::test]
    async fn deny_rejects_before_resolving() {
        struct PanicResolver;
        impl HostResolver for PanicResolver {
            fn resolve_host<'a>(&'a self, _host: &'a str, _port: u16) -> ResolveHostFuture<'a> {
                Box::pin(async { panic!("deny must short-circuit before any DNS resolution") })
            }
        }

        let resolver = PolicyDnsResolver::with_resolver(
            Arc::new(EgressGate::new(EgressPolicy::Deny, &[])),
            Arc::new(PanicResolver),
        );

        let err = resolver
            .resolve(name("example.com"))
            .await
            .map(|_| ())
            .expect_err("deny must reject");
        assert!(err.to_string().contains("deny"));
    }
}
