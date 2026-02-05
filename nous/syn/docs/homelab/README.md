# Homelab Infrastructure Documentation

*Last Updated: 2024-02-03*

This directory contains comprehensive documentation of the homelab infrastructure setup.

## Overview

The homelab consists of three main systems working together:

- **worker-node (192.168.0.29)** - Ubuntu 24.04 server running Clawdbot agents and coordination services
- **HiFi-NAS (192.168.0.120)** - Synology DS923+ with 32TB storage running Docker infrastructure
- **Metis (192.168.0.17)** - Fedora laptop serving as primary development workstation

## Documentation Structure

- [`network-topology.md`](./network-topology.md) - Network diagram and IP assignments
- [`container-inventory.md`](./container-inventory.md) - Complete service inventory with ports
- [`nas-configuration.md`](./nas-configuration.md) - NAS shares and mount points
- [`access-urls.md`](./access-urls.md) - Web interfaces and service endpoints
- [`docker-configurations.md`](./docker-configurations.md) - Docker compose configurations

## Quick Reference

### Key Services
| Service | URL | Purpose |
|---------|-----|---------|
| Portainer | http://192.168.0.29:9001 | Container management |
| Overseerr | http://192.168.0.29:5055 | Media requests |
| Plex | http://192.168.0.29:32400 | Media server |
| Dashy | http://192.168.0.29:7575 | Service dashboard |
| Tautulli | http://192.168.0.29:8181 | Plex analytics |

### NAS Mounts
| Mount Point | NAS Path | Purpose |
|-------------|----------|---------|
| `/mnt/nas/Media` | `/volume1/Media` | Media library |
| `/mnt/nas/docker` | `/volume1/docker` | Container configs |
| `/mnt/nas/home` | `/volume1/homes/Cody.Kickertz` | User files |
| `/mnt/nas/photos` | `/volume1/homes/Cody.Kickertz/Photos` | Photo library |

### Tailscale Network
| Device | Tailscale IP | Status | Purpose |
|--------|--------------|--------|---------|
| worker-node | 100.87.6.45 | Active | Main server |
| hifi-nas | 100.104.41.59 | Active | Storage |
| metis | 100.117.8.41 | Active | Development |

## Key Features

- **VPN-Secured Media Stack** - All *arr services route through Gluetun VPN
- **Automated Management** - Portainer for container orchestration
- **Centralized Storage** - NFS shares from Synology NAS
- **Remote Access** - Tailscale mesh network for secure remote access
- **Service Discovery** - Dashy dashboard for centralized access