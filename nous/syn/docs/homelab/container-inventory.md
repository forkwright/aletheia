# Container Service Inventory

*Last Updated: 2024-02-03*

## Container Overview

The homelab runs 21+ Docker containers across two systems, providing a comprehensive media management and automation stack.

## Media Stack (worker-node)

### Core Media Services

| Container | Image | Ports | Status | Purpose |
|-----------|-------|-------|--------|---------|
| **plex** | linuxserver/plex | 32400/tcp | Up 2h | Media streaming server |
| **overseerr** | linuxserver/overseerr | 5055/tcp | Up 7min | Media request management |
| **tautulli** | tautulli/tautulli | 8181/tcp | Up 2h | Plex analytics and monitoring |

### Content Management (*arr Stack)

| Container | Image | Ports | VPN | Purpose |
|-----------|-------|-------|-----|---------|
| **sonarr** | linuxserver/sonarr | 8989/tcp | ✅ | TV show automation |
| **radarr** | linuxserver/radarr | 7878/tcp | ✅ | Movie automation |
| **lidarr** | linuxserver/lidarr | 8686/tcp | ✅ | Music automation |
| **bazarr** | linuxserver/bazarr | 6767/tcp | ✅ | Subtitle automation |
| **prowlarr** | linuxserver/prowlarr | 9696/tcp | ✅ | Indexer management |

### Download and Processing

| Container | Image | Ports | VPN | Purpose |
|-----------|-------|-------|-----|---------|
| **qbittorrent** | linuxserver/qbittorrent | 6881/tcp+udp<br/>8080/tcp | ✅ | BitTorrent client |
| **gluetun** | qmcgaw/gluetun | Multiple* | N/A | VPN gateway for stack |
| **unpackerr** | golift/unpackerr | None | ✅ | Archive extraction |
| **tdarr** | haveagitgat/tdarr | 8265-8266/tcp | No | Media transcoding |

*Gluetun exposes multiple ports for routing other services through VPN

### Automation and Management

| Container | Image | Ports | Purpose |
|-----------|-------|-------|---------|
| **qbit_manage** | bobokun/qbit_manage | 8080/tcp | Torrent management automation |
| **decluttarr** | manimatter/decluttarr | None | Automatic media cleanup |
| **recyclarr** | recyclarr/recyclarr | None | Quality profile synchronization |
| **byparr** | thephaseless/byparr | None | *arr service bypass management |
| **checkrr** | aetaric/checkrr | 8585/tcp | Service health monitoring |
| **pulsarr** | lakker/pulsarr | 3003/tcp | *arr service coordination |

### Additional Media Services

| Container | Image | Ports | Purpose |
|-----------|-------|-------|---------|
| **audiobookshelf** | advplyr/audiobookshelf | 13378/tcp | Audiobook and podcast server |

## Infrastructure Services

### System Management (worker-node)

| Container | Image | Ports | Purpose |
|-----------|-------|-------|---------|
| **portainer-agent** | portainer/agent | 9001/tcp | Container management agent |
| **dashy** | lissy93/dashy | 7575/tcp | Service dashboard |
| **falkordb** | falkordb/falkordb | 6379/tcp | Knowledge graph database |

### NAS Infrastructure (HiFi-NAS)

| Container | Image | Ports | Purpose |
|-----------|-------|-------|---------|
| **portainer** | portainer/portainer-ce | 9000/tcp | Container management UI |
| **gitea** | gitea/gitea | 3000/tcp<br/>222/tcp | Git repository hosting |
| **tdarr-node** | haveagitgat/tdarr_node | None | Distributed transcoding node |

## Service Dependencies

### VPN Routing (Gluetun Network)
All download and indexer services route through the Gluetun VPN container:

```
Internet ← VPN ← Gluetun ← [qBittorrent, Sonarr, Radarr, Lidarr, Bazarr, Prowlarr]
```

**Gluetun Port Mappings:**
- 6767:6767 → qBittorrent WebUI (alt)
- 6881:6881 → qBittorrent peer traffic
- 7878:7878 → Prowlarr
- 8000:8000 → qBittorrent WebUI
- 8085:8085 → qBittorrent daemon
- 8191:8191 → FlareSolverr (if enabled)
- 8686:8686 → Lidarr
- 8787:8787 → Readarr (if enabled)
- 8789:8789 → Bazarr
- 8989:8989 → Sonarr
- 9696:9696 → Prowlarr

### Service Communication Flow

```
1. Overseerr (requests) → Sonarr/Radarr/Lidarr (automation)
2. *arr services → Prowlarr (indexer search)
3. *arr services → qBittorrent (download management)
4. qBittorrent → Unpackerr (extraction)
5. Media files → Plex (streaming)
6. Plex activity → Tautulli (analytics)
7. Tdarr monitors → Media transcoding
```

## Health Monitoring

### Container Status
```bash
# Check all containers
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"

# Check specific services
docker stats --no-stream
```

### Service Health Endpoints
| Service | Health Check URL |
|---------|------------------|
| Overseerr | http://192.168.0.29:5055/api/v1/status |
| Sonarr | http://192.168.0.29:8989/api/v3/system/status |
| Radarr | http://192.168.0.29:7878/api/v3/system/status |
| qBittorrent | http://192.168.0.29:6767/api/v2/app/version |
| Plex | http://192.168.0.29:32400/identity |
| Tautulli | http://192.168.0.29:8181/api/v2?apikey=XXX&cmd=get_server_info |

## Storage Mappings

### Media Paths
| Container Path | Host Path | Purpose |
|----------------|-----------|---------|
| `/movies` | `/mnt/nas/Media/Movies` | Movie library |
| `/tv` | `/mnt/nas/Media/TV Shows` | TV show library |
| `/music` | `/mnt/nas/Media/Music` | Music library |
| `/downloads` | `/mnt/nas/vpn_media/downloads` | Download staging |

### Configuration Paths
| Service | Config Path |
|---------|-------------|
| Sonarr | `/mnt/nas/docker/infra/sonarr` |
| Radarr | `/mnt/nas/docker/infra/radarr` |
| Lidarr | `/mnt/nas/docker/infra/lidarr` |
| qBittorrent | `/mnt/nas/docker/infra/qbittorrent` |
| Plex | `/mnt/nas/docker/infra/plex` |

## Resource Usage

### High Resource Containers
- **Plex** - High CPU during transcoding, 2-8GB RAM
- **Tdarr** - High CPU for transcoding tasks, 1-4GB RAM
- **qBittorrent** - Moderate CPU/RAM, high I/O
- **Gluetun** - Low resource overhead, critical network path

### Low Resource Containers  
- **Dashy** - <100MB RAM, minimal CPU
- **Portainer Agent** - <50MB RAM, minimal CPU
- **Recyclarr** - Periodic execution only
- **Decluttarr** - Periodic execution only

## Network Security

### VPN Protected Services
All *arr services and qBittorrent route through Gluetun VPN for privacy and security.

### Direct Network Access
- **Plex** - Direct access for optimal streaming performance
- **Overseerr** - Direct access for web interface
- **Tautulli** - Direct access for monitoring
- **Dashy** - Direct access for dashboard

### Internal Only
- **FalkorDB** - Only accessible via localhost
- **Portainer Agent** - Only accessible from Portainer server