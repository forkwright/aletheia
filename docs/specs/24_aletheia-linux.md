# Spec 24: Aletheia Linux — OS and Network Integration

**Status:** Skeleton  
**Author:** Cody  
**Date:** 2026-02-21  
**Spec:** 24  

## Problem

Aletheia operates entirely in userspace. It can't see network traffic, respond to security events, manage firewall rules, observe process behavior, or interact with the kernel. The system is blind to the machine it runs on. For a distributed cognition system that manages infrastructure, this is a fundamental gap — it knows what's in its conversations but not what's happening on its host.

## Vision

Bridge Aletheia to the OS and network layers. Not a custom distro — a capability layer that gives agents awareness of and (controlled) authority over system-level concerns:

- **Network visibility:** Traffic patterns, connection monitoring, anomaly detection
- **Security posture:** Intrusion detection, firewall management, fail2ban-style response
- **OS awareness:** Process monitoring, resource utilization, disk/service health
- **Kernel-level signals:** eBPF or audit subsystem integration for real-time event streams
- **Automated response:** Agent-driven remediation within defined policy boundaries

## Open Questions

- How deep? eBPF programs vs. parsing existing tools (ss, iptables, journald)?
- What's the authority model? Agents observe everything but act within policy cages?
- Does this compose with Spec 20 (security hardening) or supersede parts of it?
- Tailscale integration — can Aletheia manage the mesh directly?
- Multi-host: worker-node + metis + NAS as a unified security domain?
- Container awareness — should agents see inside Docker too?

## Possible Phases

1. **Observation layer** — structured feeds from journald, ss, netstat, proc, systemd
2. **Anomaly detection** — baseline normal, flag deviations, surface to agents
3. **Network security** — connection monitoring, port scanning detection, DNS anomaly flagging
4. **Controlled response** — firewall rule injection, service restart, IP blocking within policy
5. **eBPF integration** — kernel-level event streams for real-time visibility
6. **Multi-host mesh** — unified view across all Aletheia-managed machines

## Dependencies

- Spec 20 (Security Hardening) — complementary, not duplicative
- Root/sudo access model needs careful design
- Agent authority boundaries (what can an agent do without human approval?)

## Notes

The instinct to build a full "Aletheia Linux" distro is probably overkill. The right move is likely a capability layer — daemons, eBPF programs, structured feeds — that plugs into the existing runtime. The agents don't need to BE the OS. They need to see it and act on it within policy.
