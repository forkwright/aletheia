# Aletheia Capability Roadmap
*Based on research doc: 2026-02-05*

## Deployed Tonight

| Capability | Tool | Primary Nous | Status |
|-----------|------|-------------|--------|
| **Observability** | Langfuse (self-hosted) | All | ✅ Running on :3100 |
| **Browser Automation** | Browser Use | Chiron | ✅ Installed |
| **Document Processing** | Docling | Eiron | ⏳ Installing |

## Next (Software Only, Needs Auth/Config)

| Capability | Tool | Primary Nous | Blocker |
|-----------|------|-------------|---------|
| **Email AI** | Inbox Zero / himalaya | Syn | OAuth token expired |
| **Calendar AI** | gcal (existing) | All | OAuth token expired |
| **Graphiti** | Graphiti | Syn | Needs Neo4j (evaluate FalkorDB compat) |
| **Voice** | Pipecat + ElevenLabs | Syn | API key + mic config |

## Future (Needs Hardware)

| Capability | Tool | Primary Nous | What's Needed |
|-----------|------|-------------|---------------|
| **Smart Home** | Home Assistant MCP | Syl | HA hub + Zigbee coordinator ($100-200) |
| **Vehicle Diagnostics** | OBD2 + Bluetooth | Akron | OBDLink CX ($80-100) |
| **Voice Interface** | HA Voice PE | All | HA Voice PE device ($59) |

## Integration Architecture

```
Langfuse ←── traces ←── all nous sessions
                          ├── Chiron → browse (web automation)
                          ├── Eiron → ingest-doc (papers)
                          ├── Syl → Home Assistant (future)
                          ├── Akron → OBD2 (future)
                          └── Syn → orchestration + binding
```

## Approval Architecture (from research)

| Action Type | Default | Examples |
|-------------|---------|----------|
| Read-only | Auto | Check calendar, read email |
| Low-stakes write | Notify | Add calendar event, draft email |
| Medium-stakes | Confirm | Send email, purchase <$50 |
| High-stakes | Block until approved | Payments >$50, delete data |
| Irreversible | Double confirm | Contract signatures |
