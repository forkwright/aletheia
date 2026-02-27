# Spec 24: Aletheia Linux — OS and Network Integration

**Status:** Skeleton
**Origin:** Issue #332
**Author:** Cody
**Date:** 2026-02-21
**Spec:** 24

---

## Problem

Aletheia operates entirely in userspace. It can't see network traffic, respond to security events, manage firewall rules, observe process behavior, or interact with the kernel. The system is blind to the machine it runs on. For a distributed cognition system that manages infrastructure, this is a fundamental gap — it knows what's in its conversations but not what's happening on its host.

## Vision

Bridge Aletheia to the OS and network layers. Not a custom distro — a capability layer that gives agents awareness of and (controlled) authority over system-level concerns:

- **Network visibility:** Traffic patterns, connection monitoring, anomaly detection
- **Security posture:** Intrusion detection, firewall management, fail2ban-style response
- **OS awareness:** Process monitoring, resource utilization, disk/service health
- **Kernel-level signals:** eBPF or audit subsystem integration for real-time event streams
- **DBus sensing:** Desktop environment events, session state, hardware changes
- **Automated response:** Agent-driven remediation within defined policy boundaries
- **NixOS module:** Declarative deployment as a system service with proper dependency management

## Open Questions

- How deep? eBPF programs vs. parsing existing tools (ss, iptables, journald)?
- What's the authority model? Agents observe everything but act within policy cages?
- Does this compose with Spec 20 (security hardening) or supersede parts of it?
- Tailscale integration — can Aletheia manage the mesh directly?
- Multi-host: worker-node + metis + NAS as a unified security domain?
- Container awareness — should agents see inside Docker too?
- DBus vs. eBPF vs. both? DBus for desktop events, eBPF for network/kernel?
- NixOS packaging: flake, module, or both?

## Phases

### Phase 1 — Observation Layer
Structured feeds from journald, ss, netstat, proc, systemd. DBus session monitoring for desktop events (window focus, hardware changes, power state). This is the foundation — read-only visibility.

### Phase 2 — Anomaly Detection
Baseline normal system behavior, flag deviations, surface to agents. CPU/memory/disk spikes, unusual network connections, failed auth attempts, service crashes.

### Phase 3 — Network Security
Connection monitoring, port scanning detection, DNS anomaly flagging. Integration with existing fail2ban or firewall tooling.

### Phase 4 — Controlled Response
Firewall rule injection, service restart, IP blocking within policy. Requires explicit authority model — what can an agent do without human approval?

### Phase 5 — eBPF Integration
Kernel-level event streams for real-time visibility. Network packet inspection, syscall tracing, file access monitoring. High-fidelity but complex.

### Phase 6 — Multi-Host Mesh
Unified view across all Aletheia-managed machines. Aggregated health, cross-host anomaly correlation, coordinated response.

### Phase 7 — NixOS Module
Declarative system service configuration. Proper dependency management, rollback support, reproducible deployment.

## Dependencies

- Spec 20 (Security Hardening) — complementary, not duplicative
- Root/sudo access model needs careful design
- Agent authority boundaries (what can an agent do without human approval?)

## Notes

The instinct to build a full "Aletheia Linux" distro is probably overkill. The right move is likely a capability layer — daemons, eBPF programs, structured feeds — that plugs into the existing runtime. The agents don't need to BE the OS. They need to see it and act on it within policy.
