# MCP Ecosystem Research

*Updated: 2026-02-03*

## What We Have (via mcporter)

| Server | Tools | Purpose |
|--------|-------|---------|
| todoist | 86 | Task management |
| github | 26 | Repository operations |
| google-calendar | 12 | Calendar read/write |
| sequential-thinking | 1 | Step-by-step reasoning |
| task-orchestrator | 7 | Task coordination |
| memory | 9 | Knowledge graph |

**Total: 6 servers, 141 tools**

## Gap Analysis

### Already Covered (No MCP Needed)
| Need | Current Solution |
|------|------------------|
| Email | himalaya CLI (pending setup) |
| Search | pplx/research (Perplexity) |
| Web fetch | web_fetch tool (stock) |
| Google Drive | gdrive wrapper |
| Browser | browser tool (stock) |

### Potential Additions

**High Value:**
| Server | Why |
|--------|-----|
| notion | If Cody uses Notion for notes/docs |
| slack | Work context for Chiron |
| linear | If using Linear for issues |

**Medium Value:**
| Server | Why |
|--------|-----|
| obsidian | If migrating notes to Obsidian |
| readwise | If using Readwise for highlights |
| exa | AI-native search alternative |

**Low Priority:**
| Server | Why |
|--------|-----|
| postgres | Direct DB (we have SQL skills) |
| sentry | Error tracking (dev-focused) |

## Ecosystem Stats (2026)
- 1200+ MCP servers on mcp-awesome.com
- Top rated: K2view, Vectara, Zapier, Notion, Supabase
- Official directory: github.com/modelcontextprotocol/servers

## Recommendation

**Current setup is solid.** We have the core integrations:
- Tasks (Todoist)
- Code (GitHub)  
- Calendar (Google)
- Memory (MCP Memory)
- Orchestration (task-orchestrator)

**Only add if needed:**
- Slack MCP → when Chiron needs direct Slack access
- Notion MCP → if Cody adopts Notion
- Don't add just to have more

## Commands

```bash
mcporter list              # Show configured servers
mcporter call SERVER.TOOL  # Call a tool
mcporter tools SERVER      # List server's tools
```
