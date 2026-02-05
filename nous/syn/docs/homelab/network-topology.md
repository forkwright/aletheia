# Network Topology

*Last Updated: 2024-02-03*

## Network Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    LAN (192.168.0.0/24)                    │
│                                                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
│  │   worker-node   │  │    HiFi-NAS     │  │    Metis    │ │
│  │  192.168.0.29   │  │  192.168.0.120  │  │192.168.0.17 │ │
│  │ Ubuntu 24.04    │  │ Synology DS923+ │  │ Fedora 41   │ │
│  │                 │  │                 │  │             │ │
│  │ Clawdbot Agents │  │ Docker Services │  │ Development │ │
│  │ AI Coordination │  │ Media Stack     │  │ Workstation │ │
│  │ Main Server     │  │ File Storage    │  │             │ │
│  └─────────────────┘  └─────────────────┘  └─────────────┘ │
│           │                     │                  │       │
└───────────┼─────────────────────┼──────────────────┼───────┘
            │                     │                  │
            └─────────────────────┼──────────────────┘
                                  │
                        ┌─────────┴─────────┐
                        │   Router/Gateway  │
                        │   192.168.0.1     │
                        └─────────┬─────────┘
                                  │
                             ┌────┴────┐
                             │Internet │
                             └─────────┘
```

## Tailscale Overlay Network

```
Tailscale Mesh (100.x.x.x/16)

┌──────────────────────────────────────────────────────────────┐
│                   Tailscale Mesh Network                    │
│                                                              │
│  worker-node          HiFi-NAS             Metis            │
│  100.87.6.45 ←───────→ 100.104.41.59 ←────→ 100.117.8.41   │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │              Secure Remote Access                      ││
│  │           - End-to-end encryption                      ││
│  │           - NAT traversal                              ││
│  │           - Access from anywhere                       ││
│  └─────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────┘
```

## Network Services

### Primary Services (worker-node: 192.168.0.29)
| Port | Service | Protocol | Purpose |
|------|---------|----------|---------|
| 22 | SSH | TCP | System administration |
| 443 | HTTPS/Clawdbot | TCP | AI agent web interface |
| 5055 | Overseerr | TCP | Media request management |
| 6379 | FalkorDB | TCP | Knowledge graph database |
| 7575 | Dashy | TCP | Service dashboard |
| 8100 | CrewAI Bridge | TCP | Agent routing service |
| 8181 | Tautulli | TCP | Plex analytics |
| 8265-8266 | Tdarr | TCP | Media transcoding |
| 9001 | Portainer Agent | TCP | Container management |
| 13378 | Audiobookshelf | TCP | Audiobook server |

### VPN Routed Services (via Gluetun)
| Port | Service | Protocol | Purpose |
|------|---------|----------|---------|
| 6767 | qBittorrent WebUI | TCP | Torrent management |
| 6881 | qBittorrent | TCP/UDP | Torrent traffic |
| 7878 | Prowlarr | TCP | Indexer management |
| 8000 | qBittorrent Alt WebUI | TCP | Alternative torrent UI |
| 8085 | qBittorrent | TCP | Torrent daemon |
| 8686 | Lidarr | TCP | Music management |
| 8787 | Readarr | TCP | Book management |
| 8789 | Bazarr | TCP | Subtitle management |
| 8989 | Sonarr | TCP | TV show management |
| 9696 | Prowlarr | TCP | Indexer proxy |

### NAS Services (HiFi-NAS: 192.168.0.120)
| Port | Service | Protocol | Purpose |
|------|---------|----------|---------|
| 22 | SSH | TCP | NAS administration |
| 80/443 | DSM Web | TCP | Synology management |
| 2049 | NFS | TCP | Network file sharing |
| 3000 | Gitea | TCP | Git repository hosting |
| 9000 | Portainer | TCP | Docker management |

## Docker Networks

### Container Networks (worker-node)
| Network | CIDR | Purpose |
|---------|------|---------|
| docker0 | 172.17.0.1/16 | Default Docker bridge |
| br-56895df84e42 | 172.18.0.1/16 | Media stack network |
| br-4c4b61152023 | 172.19.0.1/16 | Utilities network |
| br-20f35ee8ba9a | 172.20.0.1/16 | Additional services |

### NAS Container Networks
| Network | Purpose |
|---------|---------|
| utilities | Infrastructure services (Portainer, Gitea, Tdarr node) |

## Mount Points and Shares

### NFS Mounts (worker-node → HiFi-NAS)
| Local Mount | Remote NFS Share | Options |
|-------------|------------------|---------|
| `/mnt/nas/Media` | `192.168.0.120:/volume1/Media` | NFSv4.1, soft mount |
| `/mnt/nas/docker` | `192.168.0.120:/volume1/docker` | NFSv4.1, soft mount |
| `/mnt/nas/home` | `192.168.0.120:/volume1/homes/Cody.Kickertz` | NFSv4.1, soft mount |
| `/mnt/nas/photos` | `192.168.0.120:/volume1/homes/Cody.Kickertz/Photos` | NFSv4.1, soft mount |
| `/mnt/nas/vpn_media` | `192.168.0.120:/volume1/docker/vpn_media` | NFSv4.1, soft mount |

### Local Storage (worker-node)
| Mount Point | Device | Size | Purpose |
|-------------|--------|------|---------|
| `/mnt/ssd` | `/dev/sda1` | ~512GB | Fast local storage for AI workspaces |

## Security Considerations

- **VPN Protection**: All *arr services and torrenting route through Gluetun VPN
- **Network Isolation**: Docker networks isolate service groups
- **Tailscale Security**: End-to-end encrypted mesh network
- **NFS Security**: Mounts use NFSv4.1 with Kerberos support
- **SSH Access**: Key-based authentication only

## Access Methods

### Local Network
- Direct IP access: `http://192.168.0.29:PORT`
- Service URLs listed in access-urls.md

### Remote Access
- Tailscale: `http://100.87.6.45:PORT`
- Clawdbot webchat: `https://100.87.6.45:8443`

### Emergency Access
- NAS direct: `https://192.168.0.120:5001`
- SSH via Tailscale: `ssh syn@100.87.6.45`