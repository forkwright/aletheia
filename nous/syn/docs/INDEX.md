# Documentation Index

*Last Updated: 2024-02-03*

This directory contains comprehensive documentation for the Clawdbot ecosystem and supporting infrastructure.

## System Documentation

### Core Architecture
- [`ARCHITECTURE.md`](./ARCHITECTURE.md) - Complete system architecture including agents, memory, coordination, and infrastructure

### Homelab Infrastructure
- [`homelab/`](./homelab/) - Complete homelab documentation
  - [`README.md`](./homelab/README.md) - Infrastructure overview
  - [`network-topology.md`](./homelab/network-topology.md) - Network diagram and IP assignments  
  - [`container-inventory.md`](./homelab/container-inventory.md) - Service inventory with ports
  - [`nas-configuration.md`](./homelab/nas-configuration.md) - NAS shares and mount points
  - [`access-urls.md`](./homelab/access-urls.md) - Web interfaces and service endpoints
  - [`docker-configurations.md`](./homelab/docker-configurations.md) - Docker compose configurations

### Network and VPN
- [`mullvad-tailscale.md`](./mullvad-tailscale.md) - VPN configuration and networking setup

## Research and Development

### Migration Planning
- [`crewai-migration/`](./crewai-migration/) - CrewAI framework migration documentation

### Infrastructure Research  
- [`infra-research/`](./infra-research/) - Infrastructure recommendations and research
  - [`memory-architecture-recommendation.md`](./infra-research/memory-architecture-recommendation.md) - Memory system recommendations

## Quick Reference

### Key Systems
| System | Location | Purpose |
|--------|----------|---------|
| **worker-node** | 192.168.0.29 | Main server running agents and media stack |
| **HiFi-NAS** | 192.168.0.120 | Synology NAS with 32TB storage |
| **Metis** | 192.168.0.17 | Development workstation |

### Primary Services
| Service | URL | Purpose |
|---------|-----|---------|
| **Clawdbot Chat** | https://192.168.0.29:8443 | AI agent web interface |
| **Portainer** | http://192.168.0.29:9001 | Container management |
| **Dashy** | http://192.168.0.29:7575 | Service dashboard |
| **Plex** | http://192.168.0.29:32400 | Media streaming |
| **Overseerr** | http://192.168.0.29:5055 | Media requests |

### Documentation Organization

**System Level:** High-level architecture and design decisions
**Component Level:** Detailed configuration and operation of specific components  
**Operational Level:** Day-to-day procedures and troubleshooting

## Document Status

| Document | Status | Last Updated |
|----------|--------|--------------|
| ARCHITECTURE.md | ✅ Current | 2025-01-30 |
| homelab/ | ✅ Complete | 2024-02-03 |
| mullvad-tailscale.md | ✅ Current | 2024-01-29 |
| crewai-migration/ | 📝 In Progress | 2024-01-29 |
| infra-research/ | 📝 In Progress | 2024-01-29 |

## Getting Started

### New User Onboarding
1. Read [ARCHITECTURE.md](./ARCHITECTURE.md) for system overview
2. Review [homelab/README.md](./homelab/README.md) for infrastructure
3. Check [homelab/access-urls.md](./homelab/access-urls.md) for service access

### Administrator Resources
1. [homelab/container-inventory.md](./homelab/container-inventory.md) - Service management
2. [homelab/docker-configurations.md](./homelab/docker-configurations.md) - Container configs  
3. [homelab/nas-configuration.md](./homelab/nas-configuration.md) - Storage management

### Developer Resources
1. [ARCHITECTURE.md](./ARCHITECTURE.md#coordination-systems) - Agent coordination
2. [crewai-migration/](./crewai-migration/) - Framework evolution
3. [infra-research/](./infra-research/) - Future planning

---

*This documentation is actively maintained and reflects the current state of the system.*