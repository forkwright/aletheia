# NAS Configuration and Storage

*Last Updated: 2024-02-03*

## Hardware Overview

**Synology DS923+ (HiFi-NAS)**
- **IP Address:** 192.168.0.120
- **Hostname:** HiFi-NAS
- **Storage Capacity:** ~32TB total
- **OS:** Synology DSM 7.x
- **Tailscale IP:** 100.104.41.59

## Storage Volumes

### Volume Structure
```
/volume1/
├── docker/              # Container configurations and data
├── homes/              # User home directories
│   └── Cody.Kickertz/  # Primary user directory
│       ├── Photos/     # Photo library
│       ├── Documents/  # Personal documents
│       └── ...         # Other user files
├── Media/              # Media library (movies, TV, music)
├── fireflyiii/         # Financial management data
├── homeassistant/      # Home automation data
├── utilities/          # Utility applications data
└── [other shared folders]
```

## NFS Exports and Mounts

### Active NFS Mounts (worker-node → HiFi-NAS)

| Local Mount Point | NFS Export | Mount Options |
|-------------------|------------|---------------|
| `/mnt/nas/Media` | `192.168.0.120:/volume1/Media` | NFSv4.1, soft, rw |
| `/mnt/nas/docker` | `192.168.0.120:/volume1/docker` | NFSv4.1, soft, rw |
| `/mnt/nas/home` | `192.168.0.120:/volume1/homes/Cody.Kickertz` | NFSv4.1, soft, rw |
| `/mnt/nas/photos` | `192.168.0.120:/volume1/homes/Cody.Kickertz/Photos` | NFSv4.1, soft, rw |
| `/mnt/nas/vpn_media` | `192.168.0.120:/volume1/docker/vpn_media` | NFSv4.1, soft, rw |

### Mount Configuration Details

**Mount Command Examples:**
```bash
# Verify mounts
mount | grep nas

# Manual remount if needed
sudo mount -t nfs4 192.168.0.120:/volume1/Media /mnt/nas/Media
```

**fstab Entries:**
```
# NAS Mounts (likely configured via autofs)
192.168.0.120:/volume1/Media /mnt/nas/Media nfs4 rw,soft,timeo=150,retrans=3 0 0
192.168.0.120:/volume1/docker /mnt/nas/docker nfs4 rw,soft,timeo=150,retrans=3 0 0
```

## Docker Configuration Storage

### Container Data Paths

| Service Category | NAS Path | Purpose |
|------------------|----------|---------|
| **Infrastructure** | `/volume1/docker/infra/` | Core infrastructure containers |
| **VPN Media** | `/volume1/docker/vpn_media/` | Download and processing data |
| **Media Apps** | `/volume1/docker/media/` | Media server configurations |

### Specific Service Configurations

**Infrastructure Services (on NAS):**
```
/volume1/docker/infra/
├── portainer/
│   └── data/           # Portainer management data
├── gitea/
│   └── data/           # Git repositories and metadata
├── tdarr_node/
│   ├── configs/        # Node configuration
│   ├── logs/           # Processing logs
│   └── cache/          # Transcoding cache
└── watchtower/         # Watchtower configuration
```

**Media Processing (worker-node accessible via NFS):**
```
/volume1/docker/vpn_media/ (mounted as /mnt/nas/vpn_media)
├── downloads/          # qBittorrent downloads
├── sonarr/            # TV show management configs
├── radarr/            # Movie management configs
├── lidarr/            # Music management configs
├── bazarr/            # Subtitle management configs
├── prowlarr/          # Indexer management configs
├── qbittorrent/       # Torrent client configs
└── gluetun/           # VPN client configs
```

## Media Library Structure

### Media Organization
```
/volume1/Media/ (mounted as /mnt/nas/Media)
├── Movies/
│   ├── [Movie Title] (Year)/
│   │   ├── [Movie Title] (Year).mkv
│   │   └── [Movie Title] (Year).nfo
│   └── ...
├── TV Shows/
│   ├── [Show Name]/
│   │   ├── Season 01/
│   │   │   ├── S01E01 - Episode Title.mkv
│   │   │   └── ...
│   │   └── Season XX/
│   └── ...
├── Music/
│   ├── [Artist]/
│   │   ├── [Album]/
│   │   │   ├── 01 - Song Title.flac
│   │   │   └── ...
│   │   └── ...
│   └── ...
├── Audiobooks/
│   ├── [Author]/
│   │   ├── [Book Title]/
│   │   │   ├── Chapter 01.m4a
│   │   │   └── ...
│   │   └── ...
│   └── ...
└── Podcasts/
    ├── [Podcast Name]/
    │   ├── Episode Title.mp3
    │   └── ...
    └── ...
```

## Backup and Redundancy

### RAID Configuration
- **RAID Type:** SHR (Synology Hybrid RAID) or RAID 5/6
- **Hot Spare:** Configured for automatic failover
- **Disk Health:** Monitored via DSM interface

### Backup Strategy
- **Cloud Sync:** Important data synced to cloud storage
- **Version Control:** Git repositories backed up via Gitea
- **Snapshot Replication:** Btrfs snapshots for file recovery
- **HyperBackup:** System configuration and critical data backup

## Performance Optimizations

### NFS Tuning
```bash
# NFS mount options for performance
rsize=131072        # Read buffer size
wsize=131072        # Write buffer size
timeo=150          # Timeout for RPC calls
retrans=3          # Number of retransmissions
soft               # Soft mount (don't hang on NAS issues)
```

### Network Performance
- **Gigabit Ethernet:** All devices on gigabit network
- **Jumbo Frames:** Enabled for NFS traffic where supported
- **Direct Connection:** No network hops between worker-node and NAS

### Storage Performance
- **SSD Cache:** NAS likely configured with SSD read/write cache
- **File System:** Btrfs with compression enabled
- **RAM Cache:** Synology OS aggressive file caching

## Access Control and Security

### NFS Security
- **Network Restriction:** NFS exports limited to local network
- **No Root Squashing:** Limited to necessary services only
- **Authentication:** Host-based authentication for NFS mounts

### User Management
- **Primary User:** Cody.Kickertz (UID 1026)
- **Service Accounts:** Docker containers run with appropriate UIDs
- **Group Memberships:** Users group (GID 100), docker group (GID 105733)

### Firewall Configuration
- **NFS Ports:** 2049 (NFS), 111 (portmap), 892 (mountd)
- **SSH:** Port 22 for administration
- **DSM Web:** Ports 5000/5001 for HTTPS management
- **Custom Services:** Gitea (3000), Portainer (9000)

## Monitoring and Maintenance

### Health Monitoring
```bash
# Check NFS mount health
df -h | grep nas
mount | grep nas

# Test NFS connectivity
showmount -e 192.168.0.120

# Check mount options
cat /proc/mounts | grep nas
```

### Regular Maintenance
- **Disk SMART Tests:** Weekly automated tests
- **File System Check:** Monthly consistency checks
- **Docker Cleanup:** Automated via Watchtower and cron jobs
- **Log Rotation:** Automated log management
- **Snapshot Cleanup:** Automated old snapshot removal

### Troubleshooting Common Issues

**NFS Mount Issues:**
```bash
# Check NFS service status
systemctl status nfs-client.target

# Remount failed mounts
sudo umount /mnt/nas/Media
sudo mount -t nfs4 192.168.0.120:/volume1/Media /mnt/nas/Media

# Check network connectivity
ping 192.168.0.120
telnet 192.168.0.120 2049
```

**Permission Issues:**
```bash
# Check ownership
ls -la /mnt/nas/docker/

# Fix ownership if needed (be careful!)
sudo chown -R 1026:100 /mnt/nas/docker/service_name/
```

## Service Integration

### Media Services
- **Plex Media Server:** Reads from `/mnt/nas/Media/`
- **Overseerr:** Manages requests, writes to *arr configs
- **Sonarr/Radarr/Lidarr:** Write to download areas, organize to Media
- **qBittorrent:** Downloads to `/mnt/nas/vpn_media/downloads/`

### Configuration Management
- **Portainer:** Manages containers, stores configs on NAS
- **Docker Compose:** Configuration files stored in `/volume1/docker/`
- **Application Configs:** Persistent via NFS mounts

This NAS configuration provides centralized, resilient storage for the entire homelab infrastructure while maintaining good performance through optimized NFS settings and network configuration.