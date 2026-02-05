# TOOLS.md - Arbor's Local Notes

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/moltbot/shared/TOOLS-INFRASTRUCTURE.md) for common commands.

---

## GitHub

| Item | Value |
|------|-------|
| Repo | https://github.com/forkwright/a2z-tree-site |
| Account | forkwright (Cody's) |
| Branch | main |
| CI/CD | GitHub Actions → Cloudflare Pages |

## Cloudflare

| Item | Value |
|------|-------|
| Domain | a2z409.com |
| Pages URL | a2z-tree-site.pages.dev |
| Zone ID | 8d58578f2a540b5a214d097a70d409d8 |
| Project ID | d9daf78d-72a7-424c-98be-448a9059e3f8 |
| Credentials | `.env` in workspace root |

**Status:** Zone configured, waiting for nameserver cutover from Google Domains.

## Site Development

```bash
# Local dev
cd a2z-tree-site && npm start

# Build
npm run build

# Deploy (automatic on push to main)
git push origin main
```

## Key Files

| File | Purpose |
|------|---------|
| `src/_data/business.json` | All business info (single source of truth) |
| `src/_includes/base.njk` | Base template |
| `src/css/style.css` | All styles |
| `.env` | Cloudflare credentials (gitignored) |

## Coordination

| Agent | Role |
|-------|------|
| Syn | Orchestrator, reviews, cross-agent coord |
| Demiurge | Technical reference (Ardent patterns) |

## Research Tools

| Tool | Purpose |
|------|---------|
| `web_search` | Search the web (Brave API) |
| `web_fetch` | Fetch and extract content from URLs |
| `browser` | Full browser control for complex sites |
| `memory_search` | Semantic search across memory files |
| `sessions_spawn` | Spawn sub-agents for parallel research |

**Rule:** Research before claiming. "I don't know" beats wrong. See SOUL.md for full verification protocol.

## Available Tooling

Full access to ecosystem tools:
- File operations (read, write, edit)
- Shell execution (exec)
- Web research (web_search, web_fetch, browser)
- Memory (memory_search, memory_get)
- Sub-agents (sessions_spawn, sessions_send)
- Messaging (message)
- Image analysis (image)

**Denied:** gateway, cron (orchestration reserved for Syn)

## Future Tools

- Contact form: Formspree (recommended) or Cloudflare Workers
- Invoicing: Wave (free, recommended)
- Analytics: Cloudflare Web Analytics (free)

---

*Updated: 2026-01-31*
