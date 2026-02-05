# Service Access URLs

*Last Updated: 2024-02-03*

## Quick Access Dashboard

### Primary Services
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Dashy** | [http://192.168.0.29:7575](http://192.168.0.29:7575) | [http://100.87.6.45:7575](http://100.87.6.45:7575) | 🏠 Main service dashboard |
| **Portainer** | [http://192.168.0.29:9001](http://192.168.0.29:9001) | [http://100.87.6.45:9001](http://100.87.6.45:9001) | 🐳 Container management |
| **Plex** | [http://192.168.0.29:32400](http://192.168.0.29:32400) | [http://100.87.6.45:32400](http://100.87.6.45:32400) | 🎬 Media streaming |
| **Overseerr** | [http://192.168.0.29:5055](http://192.168.0.29:5055) | [http://100.87.6.45:5055](http://100.87.6.45:5055) | 📝 Media requests |

## Media Management Stack

### Content Management (*arr Services)
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Sonarr** | [http://192.168.0.29:8989](http://192.168.0.29:8989) | [http://100.87.6.45:8989](http://100.87.6.45:8989) | 📺 TV show automation |
| **Radarr** | [http://192.168.0.29:7878](http://192.168.0.29:7878) | [http://100.87.6.45:7878](http://100.87.6.45:7878) | 🎬 Movie automation |
| **Lidarr** | [http://192.168.0.29:8686](http://192.168.0.29:8686) | [http://100.87.6.45:8686](http://100.87.6.45:8686) | 🎵 Music automation |
| **Bazarr** | [http://192.168.0.29:8789](http://192.168.0.29:8789) | [http://100.87.6.45:8789](http://100.87.6.45:8789) | 💬 Subtitle management |
| **Prowlarr** | [http://192.168.0.29:9696](http://192.168.0.29:9696) | [http://100.87.6.45:9696](http://100.87.6.45:9696) | 🔍 Indexer management |

### Download Management
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **qBittorrent** | [http://192.168.0.29:6767](http://192.168.0.29:6767) | [http://100.87.6.45:6767](http://100.87.6.45:6767) | ⬇️ Torrent client |
| **qBittorrent Alt** | [http://192.168.0.29:8000](http://192.168.0.29:8000) | [http://100.87.6.45:8000](http://100.87.6.45:8000) | ⬇️ Alternative WebUI |

### Media Processing
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Tdarr** | [http://192.168.0.29:8265](http://192.168.0.29:8265) | [http://100.87.6.45:8265](http://100.87.6.45:8265) | 🔄 Media transcoding |
| **Tautulli** | [http://192.168.0.29:8181](http://192.168.0.29:8181) | [http://100.87.6.45:8181](http://100.87.6.45:8181) | 📊 Plex analytics |

### Additional Media Services
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Audiobookshelf** | [http://192.168.0.29:13378](http://192.168.0.29:13378) | [http://100.87.6.45:13378](http://100.87.6.45:13378) | 📚 Audiobook server |
| **Pulsarr** | [http://192.168.0.29:3003](http://192.168.0.29:3003) | [http://100.87.6.45:3003](http://100.87.6.45:3003) | 🔗 *arr coordination |
| **Checkrr** | [http://192.168.0.29:8585](http://192.168.0.29:8585) | [http://100.87.6.45:8585](http://100.87.6.45:8585) | ✅ Health monitoring |

## NAS Services (HiFi-NAS)

### Infrastructure Management
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Synology DSM** | [https://192.168.0.120:5001](https://192.168.0.120:5001) | [https://100.104.41.59:5001](https://100.104.41.59:5001) | 🖥️ NAS management |
| **Portainer (NAS)** | [http://192.168.0.120:9000](http://192.168.0.120:9000) | [http://100.104.41.59:9000](http://100.104.41.59:9000) | 🐳 NAS containers |
| **Gitea** | [http://192.168.0.120:3000](http://192.168.0.120:3000) | [http://100.104.41.59:3000](http://100.104.41.59:3000) | 📝 Git repositories |

## AI and System Services

### Agent Infrastructure
| Service | Local URL | Tailscale URL | Purpose |
|---------|-----------|---------------|---------|
| **Clawdbot Chat** | [https://192.168.0.29:8443](https://192.168.0.29:8443) | [https://100.87.6.45:8443](https://100.87.6.45:8443) | 🤖 AI agent interface |
| **FalkorDB** | `localhost:6379` | `100.87.6.45:6379` | 📈 Knowledge graph DB |
| **CrewAI Bridge** | `localhost:8100` | `100.87.6.45:8100` | 🎯 Agent routing |

## Service Categories and Authentication

### 🔓 Public Access (No Auth Required)
- Dashy (service dashboard)
- Plex (if configured for local access)
- Clawdbot webchat

### 🔐 Authenticated Services
- **Overseerr** - Plex account or local user
- **Portainer** - Admin account required
- **Synology DSM** - NAS account required
- **Gitea** - Repository account required

### 🔒 Application-Specific Auth
- **Sonarr/Radarr/Lidarr** - API key in URL or configured user
- **qBittorrent** - Default admin/adminadmin (change recommended)
- **Tautulli** - Configured during setup
- **Tdarr** - Optional authentication

## Quick Setup URLs

### First-Time Configuration
| Service | Setup URL | Default Credentials |
|---------|-----------|-------------------|
| **qBittorrent** | [http://192.168.0.29:6767](http://192.168.0.29:6767) | admin / adminadmin |
| **Overseerr** | [http://192.168.0.29:5055/setup](http://192.168.0.29:5055/setup) | Connect to Plex |
| **Portainer** | [http://192.168.0.29:9001](http://192.168.0.29:9001) | Create admin on first visit |
| **Tautulli** | [http://192.168.0.29:8181/setup](http://192.168.0.29:8181/setup) | Connect to Plex |

## API Endpoints

### Service APIs
| Service | API Base URL | Authentication |
|---------|--------------|----------------|
| **Sonarr** | `http://192.168.0.29:8989/api/v3/` | API Key required |
| **Radarr** | `http://192.168.0.29:7878/api/v3/` | API Key required |
| **Lidarr** | `http://192.168.0.29:8686/api/v1/` | API Key required |
| **Prowlarr** | `http://192.168.0.29:9696/api/v1/` | API Key required |
| **qBittorrent** | `http://192.168.0.29:6767/api/v2/` | Cookie auth |
| **Overseerr** | `http://192.168.0.29:5055/api/v1/` | API Key required |
| **Tautulli** | `http://192.168.0.29:8181/api/v2/` | API Key required |
| **Plex** | `http://192.168.0.29:32400/` | X-Plex-Token |

## Mobile Access

### Recommended Mobile Apps
| Service | iOS App | Android App | Web Access |
|---------|---------|-------------|------------|
| **Plex** | Plex for iOS | Plex for Android | ✅ Web player |
| **Overseerr** | Web browser | Web browser | ✅ Mobile responsive |
| **Tautulli** | Tautulli Remote | Tautulli Remote | ✅ Mobile friendly |
| **qBittorrent** | qBittorrent Controller | qBittorrent Controller | ✅ Mobile WebUI |

### Tailscale Mobile Setup
1. Install Tailscale app on mobile device
2. Login with your account
3. Access services via `http://100.87.6.45:PORT`
4. Works from anywhere with internet

## Bookmarklet for Quick Access

```javascript
// Homelab Quick Links Bookmarklet
javascript:(function(){
var services=[
['Dashy','http://192.168.0.29:7575'],
['Overseerr','http://192.168.0.29:5055'],
['Portainer','http://192.168.0.29:9001'],
['Plex','http://192.168.0.29:32400'],
['Sonarr','http://192.168.0.29:8989'],
['Radarr','http://192.168.0.29:7878'],
['qBittorrent','http://192.168.0.29:6767']
];
var html='<div style="position:fixed;top:10px;right:10px;background:white;border:1px solid #ccc;padding:10px;z-index:9999;font-family:Arial;"><h3>Homelab Services</h3>';
services.forEach(function(s){html+='<div><a href="'+s[1]+'" target="_blank">'+s[0]+'</a></div>';});
html+='<div style="margin-top:10px;"><a href="#" onclick="this.parentElement.parentElement.style.display=\'none\'">Close</a></div></div>';
document.body.insertAdjacentHTML('afterbegin',html);
})();
```

## Network Access Notes

### Local Network (192.168.0.x)
- Fastest access method when on home network
- Direct connection, no encryption overhead
- All ports accessible

### Tailscale Network (100.x.x.x)
- Secure remote access from anywhere
- End-to-end encrypted tunnel
- Same functionality as local network
- Slightly higher latency due to routing

### Internet Access
- **Not recommended** for most services
- Only Plex configured for internet access
- Use Tailscale for secure remote access instead
- Clawdbot webchat available via HTTPS with self-signed cert

## Troubleshooting Access Issues

### Service Not Accessible
1. Check container status: `docker ps`
2. Check port availability: `ss -tuln | grep :PORT`
3. Check firewall: `sudo ufw status`
4. Check network connectivity: `ping 192.168.0.29`

### Tailscale Access Issues
1. Verify Tailscale status: `tailscale status`
2. Check connectivity: `ping 100.87.6.45`
3. Restart Tailscale: `sudo systemctl restart tailscaled`

### Authentication Problems
1. Check service logs: `docker logs container_name`
2. Clear browser cache/cookies
3. Check service-specific authentication settings
4. Reset passwords if needed

For additional help, check the service dashboard at Dashy or container logs via Portainer.