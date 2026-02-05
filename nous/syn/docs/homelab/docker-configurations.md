# Docker Configurations

*Last Updated: 2024-02-03*

## Overview

The homelab uses Docker containers deployed across two systems:
- **worker-node (192.168.0.29)** - Media stack and AI services
- **HiFi-NAS (192.168.0.120)** - Infrastructure services

Most containers are managed through Portainer, with configurations stored as compose files.

## NAS Infrastructure Stack

### Location
- **File:** `/mnt/nas/docker/infra/compose.yaml` 
- **Management:** Portainer on NAS (http://192.168.0.120:9000)

### Infrastructure Services Configuration

```yaml
# NAS Infrastructure Stack - Synology DS1621+ (192.168.0.120)
# Deploy via Portainer or: docker compose -f infra-compose.yml up -d

networks:
  utilities:
    driver: bridge

services:
  portainer:
    image: portainer/portainer-ce:latest
    container_name: portainer
    restart: always
    ports:
      - "9000:9000"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - /volume1/docker/infra/portainer/data:/data
    environment:
      - TZ=America/Chicago
    networks:
      - utilities
    dns:
      - 8.8.8.8
      - 8.8.4.4

  gitea:
    image: gitea/gitea:latest
    container_name: gitea
    restart: always
    environment:
      - USER_UID=1028
      - USER_GID=100
      - TZ=America/Chicago
    volumes:
      - /volume1/docker/infra/gitea/data:/data
    ports:
      - "3000:3000"
      - "222:22"
    networks:
      - utilities
    dns:
      - 8.8.8.8
      - 8.8.4.4

  tdarr-node:
    image: ghcr.io/haveagitgat/tdarr_node:latest
    container_name: tdarr-node
    restart: always
    environment:
      - PUID=1028
      - PGID=100
      - TZ=America/Chicago
      - nodeName=NASNode
      - serverIP=192.168.0.29
      - serverPort=8266
    volumes:
      - /volume1/docker/infra/tdarr_node/configs:/app/configs
      - /volume1/docker/infra/tdarr_node/logs:/app/logs
      - /volume1/docker/infra/tdarr_node/cache:/temp
      - /volume1/Media:/media
    networks:
      - utilities
    dns:
      - 8.8.8.8
      - 8.8.4.4
```

## Media Stack (worker-node)

### Portainer-Managed Stacks
The media stack is deployed through Portainer stacks, likely with the following structure:

#### VPN Media Stack
**Estimated compose location:** Portainer stack "media_vpn" (stack ID 28)

**Key services:**
- **Gluetun** - VPN gateway container
- **qBittorrent** - Routed through Gluetun
- ***arr services** - Routed through Gluetun network
- **Download processors** - Unpackerr, etc.

#### Direct Media Services
**Services with direct network access:**
- **Plex** - Media streaming server
- **Overseerr** - Media request management
- **Tautulli** - Plex analytics
- **Tdarr** - Media transcoding server

### Network Configuration

#### Docker Networks
```bash
# Active Docker networks on worker-node
docker network ls

# Network details
docker network inspect bridge
docker network inspect [network_name]
```

**Known networks:**
- `bridge` (172.17.0.1/16) - Default Docker bridge
- `media_vpn_default` - VPN-routed services
- `tdarr_default` - Transcoding services
- Custom bridges for service isolation

#### VPN Network Routing
```
Internet ← VPN Provider ← Gluetun ← [VPN-routed services]
                                  ├── qBittorrent (6767, 6881)
                                  ├── Sonarr (8989)
                                  ├── Radarr (7878)
                                  ├── Lidarr (8686)
                                  ├── Bazarr (8789)
                                  └── Prowlarr (9696)
```

### Volume Mappings

#### Standard Volume Patterns
```yaml
# Media services typically use:
volumes:
  # Media library (read-only for most services)
  - /mnt/nas/Media:/media:ro
  - /mnt/nas/Media/Movies:/movies:ro
  - /mnt/nas/Media/TV:/tv:ro
  - /mnt/nas/Media/Music:/music:ro
  
  # Downloads (read-write for processing)
  - /mnt/nas/vpn_media/downloads:/downloads
  
  # Service configuration
  - /mnt/nas/docker/SERVICE_NAME/config:/config
  - /mnt/nas/docker/SERVICE_NAME/data:/data
  
  # Logs and cache
  - ./logs:/logs
  - ./cache:/cache
```

#### Service-Specific Mappings

**Plex:**
```yaml
volumes:
  - /mnt/nas/Media:/media:ro
  - /mnt/nas/docker/plex/config:/config
  - /mnt/nas/docker/plex/transcode:/transcode
```

**Sonarr/Radarr/Lidarr:**
```yaml
volumes:
  - /mnt/nas/docker/SERVICE/config:/config
  - /mnt/nas/Media:/media
  - /mnt/nas/vpn_media/downloads:/downloads
```

**qBittorrent:**
```yaml
volumes:
  - /mnt/nas/docker/qbittorrent/config:/config
  - /mnt/nas/vpn_media/downloads:/downloads
```

**Tdarr:**
```yaml
volumes:
  - /mnt/nas/docker/tdarr/server:/app/server
  - /mnt/nas/docker/tdarr/configs:/app/configs
  - /mnt/nas/docker/tdarr/logs:/app/logs
  - /mnt/nas/docker/tdarr/transcode_cache:/temp
  - /mnt/nas/Media:/media
```

## Environment Configuration

### Common Environment Variables
```yaml
environment:
  # User/Group IDs (important for NFS permissions)
  - PUID=1026        # Cody's UID
  - PGID=100         # users group
  
  # Timezone
  - TZ=America/Chicago
  
  # Service-specific config
  - UMASK=022        # File permissions
  - DEBUG=false      # Disable debug logging
```

### VPN Configuration (Gluetun)
```yaml
environment:
  # VPN Provider (example - actual config varies)
  - VPN_SERVICE_PROVIDER=mullvad
  - VPN_TYPE=wireguard
  - WIREGUARD_PRIVATE_KEY=xxxxx
  - WIREGUARD_ADDRESSES=xxx.xxx.xxx.xxx/32
  - SERVER_CITIES=Dallas
  
  # DNS
  - DOT=on
  - DNS_SERVER=1.1.1.1
  - BLOCK_MALICIOUS=on
```

## Security Configuration

### Container Security
```yaml
# Security context for containers
security_opt:
  - no-new-privileges:true
  
# Read-only root filesystem where appropriate
read_only: true

# Tmpfs for writable areas
tmpfs:
  - /tmp:noexec,nosuid,size=100m
```

### Network Security
```yaml
# Disable unnecessary capabilities
cap_drop:
  - ALL
cap_add:
  - NET_BIND_SERVICE  # Only if needed

# User namespace mapping
user: "1026:100"  # Run as Cody's UID/GID
```

## Health Checks and Monitoring

### Health Check Examples
```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8989/api/v3/system/status"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 40s
```

### Restart Policies
```yaml
# Standard restart policy
restart: unless-stopped

# For critical services
restart: always

# For one-time tasks
restart: "no"
```

## Resource Management

### Resource Limits
```yaml
# CPU and memory limits for resource-intensive services
deploy:
  resources:
    limits:
      cpus: '2.0'
      memory: 4G
    reservations:
      cpus: '0.5'
      memory: 1G
```

### Logging Configuration
```yaml
logging:
  driver: "json-file"
  options:
    max-size: "50m"
    max-file: "3"
```

## Service Dependencies

### Dependency Management
```yaml
depends_on:
  # Simple dependency
  gluetun:
    condition: service_healthy
    
  # Multiple dependencies
  database:
    condition: service_healthy
  redis:
    condition: service_started
```

### Network Dependencies
```yaml
# Services that must route through VPN
network_mode: "container:gluetun"

# Or using depends_on with custom network
networks:
  - vpn_network
depends_on:
  - gluetun
```

## Configuration Management

### Portainer Stack Management
1. **Access:** http://192.168.0.120:9000 (NAS) or http://192.168.0.29:9001 (worker-node)
2. **Stacks:** View and edit compose configurations
3. **Environment Variables:** Manage secrets and config
4. **Networks:** Monitor network configuration
5. **Volumes:** Manage persistent storage

### Manual Deployment
```bash
# Deploy infrastructure stack (NAS)
docker compose -f /volume1/docker/infra/compose.yaml up -d

# Update specific service
docker compose pull SERVICE_NAME
docker compose up -d SERVICE_NAME

# View logs
docker compose logs -f SERVICE_NAME
```

### Backup and Restore
```bash
# Backup configurations
tar czf docker-configs-backup.tar.gz /mnt/nas/docker/

# Export Portainer configurations
# Via Portainer UI: Stacks → Export

# Container data backup
docker run --rm -v SERVICE_volume:/data -v $(pwd):/backup alpine tar czf /backup/service-backup.tar.gz -C /data .
```

## Troubleshooting

### Common Issues

**Permission Problems:**
```bash
# Check container user
docker exec CONTAINER id

# Fix volume permissions
sudo chown -R 1026:100 /mnt/nas/docker/SERVICE/
sudo chmod -R 755 /mnt/nas/docker/SERVICE/config/
```

**Network Issues:**
```bash
# Check container network
docker inspect CONTAINER | grep -A 10 NetworkMode

# Test connectivity
docker exec CONTAINER ping google.com
docker exec CONTAINER nslookup google.com
```

**VPN Issues:**
```bash
# Check Gluetun status
docker logs gluetun | tail -20

# Test VPN connectivity
docker exec gluetun curl -s ifconfig.me

# Check port forwarding
docker exec gluetun netstat -tulpn
```

**Resource Issues:**
```bash
# Check resource usage
docker stats --no-stream

# Check disk usage
docker system df
docker volume ls | xargs docker volume inspect

# Clean up
docker system prune -a
docker volume prune
```

### Log Locations
```bash
# Container logs
docker logs CONTAINER_NAME

# System logs
journalctl -u docker.service

# Application logs (mounted volumes)
/mnt/nas/docker/SERVICE/logs/
```

## Best Practices

1. **Version Pinning:** Use specific image tags, not `latest`
2. **Health Checks:** Implement for all services
3. **Resource Limits:** Set appropriate CPU/memory limits
4. **Security:** Run containers as non-root users
5. **Networking:** Use custom networks for service isolation
6. **Backups:** Regular backup of configurations and data
7. **Monitoring:** Implement logging and alerting
8. **Updates:** Regular but controlled updates via Portainer

This configuration provides a robust, secure, and maintainable media server infrastructure with proper separation of concerns and security practices.