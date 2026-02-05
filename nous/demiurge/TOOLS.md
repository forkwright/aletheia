# TOOLS.md - Demiurge's Tools

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/moltbot/shared/TOOLS-INFRASTRUCTURE.md) for common commands (gcal, gdrive, tw, letta, pplx, facts, mcporter).


## NAS Access

| Path | NAS Location | Contents |
|------|--------------|----------|
| `nas-home/` | `/volume1/homes/Cody.Kickertz` | Home folder |
| `nas-home/Photos/` | (same)/Photos | Photo library |
| `nas-home/Joint/` | (same)/Joint | Shared with Kendall |

```bash
# Browse home folder
ls nas-home/

# Access photos
ls nas-home/Photos/
```


## All Creative Domains (Poiesis on Metis)

```bash
# Access all creative work
ssh ck@192.168.0.17 'cat ~/dianoia/poiesis/CLAUDE.md'

# Specific domains
ssh ck@192.168.0.17 'ls ~/dianoia/poiesis/'
#=> cad  handcraft  imaging  photography

# Vehicle/overland context
ssh ck@192.168.0.17 'ls ~/dianoia/autarkeia/praxis/vehicle/'
```

## Domain Structure

### Handcraft (Ardent Brands)
| Path | Contents |
|------|----------|
| `leatherworks/` | DBA filed, pre-launch |
| `bindery/` | Bookbinding projects |
| `joinery/` | Woodworking patterns |

### Photography (Hybrid Digital + Film)
| Equipment | Specs |
|-----------|-------|
| **Digital:** Nikon D3400 + 35mm f/1.8 | Film-inspired settings, Auto ISO |
| **Film:** Canon P + Voigtlander 35mm f/2.5 | Manual rangefinder, EI 320 metering |
| **Film Stocks:** Tri-X 400, CineStill 400D, UltraMax | B&W home dev, color lab |

| Path | Contents |
|------|----------|
| `raw/YYYY/MM/` | Digital NEF + XMP files |
| `processed/YYYY/MM/` | Digital exported JPGs |
| `film/negatives/` | Scanned film strips |
| `darktable/` | Styles, workflows |

### CAD Design  
| Path | Contents |
|------|----------|
| `projects/wm1am2-truck-mount/` | Radio mount for Akron |
| `projects/pixel10xl-truck-mount/` | Phone mount |
| `projects/leather-wet-molds/` | Leathercraft tools |

### Imaging (AI Art)
| Server | Status |
|---------|--------|
| `135.181.63.179:8188` | Stable Diffusion Forge |
| Models: waiIllustriousSDXL, cyberRealisticPony | A100-80GB |

### Vehicle (Akron + Teardrop)
| System | Specs |
|--------|--------|
| Truck | $35k invested, 100Ah LiFePO4, B&W gooseneck |
| Trailer | Planned 600Ah, REDARC BCDC1250D |

## Research

```bash
/mnt/ssd/moltbot/clawd/bin/pplx "query"
/mnt/ssd/moltbot/clawd/bin/research "query" --sources
```

Also: `web_search` tool (Brave)

## Task Management

**Namespace:** `project:craft`

```bash
# Add craft task
tw add "description" project:craft priority:M

# Subprojects
tw add "..." project:craft.leather    # Ardent Leatherworks
tw add "..." project:craft.bindery    # Bookbinding
tw add "..." project:craft.joinery    # Woodworking

# View craft tasks
tw project:craft
tw project:craft.leather
```

**Tags:** +materials, +order, +wip, +design, +blocked, +review

## Letta Memory

Agent: demiurge-memory (agent-3d459f2b-867a-4ff2-8646-c38820810cb5)

```bash
# Check status (auto-detects agent from workspace)
letta status

# Store a fact
letta remember "important fact here"

# Query memory
letta ask "what do you know about X?"

# Search archival memory
letta recall "topic"

# View memory blocks
letta blocks

# Use explicit agent
letta --agent demiurge status
```
