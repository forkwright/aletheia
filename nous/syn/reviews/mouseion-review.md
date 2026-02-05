# Mouseion Project Review

**Repository:** [forkwright/mouseion](https://github.com/forkwright/mouseion)  
**Review Date:** 2026-01-28  
**Language:** C# / .NET 10.0

---

## 1. What the Project Does

Mouseion is a **unified self-hosted media automation server** that aims to replace the entire *arr ecosystem (Radarr, Sonarr, Lidarr, Readarr, Bazarr, Prowlarr) with a single application.

### Core Functionality
- **Media Library Management** for 10 media types:
  - Movies, TV Shows, Music (Albums/Artists/Tracks)
  - Books, Audiobooks, Podcasts
  - Manga, Webcomics, Comics, News/RSS feeds
- **Automated Downloading**: Monitors for new releases, integrates with download clients (qBittorrent, Transmission, NZBGet)
- **Metadata Integration**: TMDb, TVDB, MusicBrainz, OpenLibrary, Audnexus, MangaDex, AniList, ComicVine
- **Quality Management**: 103+ quality definitions across media types (lossy → hi-res, DSD, etc.)
- **File Organization**: Configurable naming patterns, automatic folder creation, hardlink/copy/symlink strategies
- **Streaming API**: HTTP 206 range requests, chapter markers for audiobooks (M4B, MP3)
- **Audiophile Features**: AcoustID fingerprinting, spectral analysis (fake hi-res detection), ReplayGain

### Key Technical Features
- REST API (v3) with OpenAPI/Swagger documentation
- SignalR for real-time updates
- SQLite (default) or PostgreSQL database
- Rate limiting, RFC 7807 ProblemDetails error handling
- Subtitle integration (OpenSubtitles API)
- Auto-tagging rules engine

### Companion Project
The backend is API-only. Frontend client: [Akroasis](https://github.com/forkwright/akroasis)

---

## 2. Current State / Issues

### Development Status
**Phase 9D In Progress** (Integration & Polish)

| Phase | Status | Focus |
|-------|--------|-------|
| 0-8 | ✅ Complete | Foundation through Polish (24 weeks) |
| 9A | ✅ Complete | Manga/Webcomics |
| 9B | ✅ Complete | News/RSS feeds |
| 9C | ✅ Complete | Comics |
| 9D | 🚧 In Progress | Integration & polish |

### Open Issues (as of review date)
| # | Title | Type |
|---|-------|------|
| #151 | LoggerMessage source generator migration | refactor |
| #150 | Bulk operations API for batch updates | feat |
| #149 | TVDB v4 API integration | feat |

### Recent PRs (last 10)
- `#148` Health checks + unified library statistics
- `#147` Comics foundation (Phase 9C)
- `#146` Manga/Webcomics foundation (Phase 9A)
- `#145` News/RSS foundation (Phase 9B)
- `#144` Validation middleware with ProblemDetails
- `#143` Repository-backed notification CRUD
- `#142` CI optimization
- `#140` Controller split per SonarCloud S6960
- `#139` Auto-tagging rule engine
- `#136` MediaItems CRUD, delta sync, quality upgrades

### Test Coverage
- 930+ tests (unit + integration)
- Target: >60% on new code
- Uses xUnit with Moq, TestWebApplicationFactory for integration tests

### Technical Debt Tracked
- LoggerMessage pattern migration deferred to Phase 9 (Issue #121)
- All sync-over-async anti-patterns removed (PR #132)
- Exception logging added to all catch blocks (PR #127)

---

## 3. Coding Standards Used

### Commit Conventions
**Conventional Commits** format required:
```
type(scope): description
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`

Examples from git log:
- `feat(comic): implement Comics foundation for Phase 9C`
- `refactor: split controllers per SonarCloud S6960`
- `chore: streamline CI for faster builds`

### Branch Naming
- `feature/your-feature`
- `fix/your-bugfix`

### Code Standards (from CONTRIBUTING.md)
- Match existing codebase patterns
- Self-documenting code (minimal comments)
- **No placeholder code or TODOs in PRs**
- Zero compiler warnings for new code

### File Headers
All source files include:
```csharp
// Copyright (c) 2025 Mouseion Project
// SPDX-License-Identifier: GPL-3.0-or-later

// Mouseion - Unified media manager
// Copyright (C) 2024-2025 Mouseion Contributors
// Based on Radarr (https://github.com/Radarr/Radarr)
// Copyright (C) 2010-2025 Radarr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

### Quality Gates (SonarCloud)
From `.sonarcloud.properties`:
- Quality gate enforced (`sonar.qualitygate.wait=true`)
- Critical issues enforced: S3776 (cognitive complexity), S1751 (loop bugs), S1244 (float equality)
- Suppressed: S101 (class naming in migrations), S3260 (sealed class suggestions)

### Architecture Patterns
- Repository pattern with Dapper (type-safe)
- DryIoc for dependency injection
- FluentValidation for declarative rules
- Full async/await with CancellationToken support
- Generic base services (e.g., `AddMediaItemService<T>`)
- Pagination default: 50/page

### Test Patterns
- xUnit framework
- Moq for mocking
- `RepositoryTestBase` for database tests
- `TestWebApplicationFactory` for integration tests
- Naming: `MethodName_Condition_ExpectedResult`

### API Standards
- REST v3 with `/api/v3/` base path
- OpenAPI/Swagger at `/swagger`
- RFC 7807 ProblemDetails for errors
- Rate limiting (100 req/min IP-based)
- API key auth via `X-Api-Key` header

### Project Structure
```
mouseion/
├── src/
│   ├── Mouseion.Common/     # Shared utilities, DI, HTTP client
│   ├── Mouseion.Core/       # Business logic, entities, services
│   ├── Mouseion.Api/        # REST API, controllers, middleware
│   ├── Mouseion.SignalR/    # Real-time messaging
│   └── Mouseion.Host/       # Application entry point
├── tests/
│   ├── Mouseion.Api.Tests/
│   ├── Mouseion.Common.Tests/
│   └── Mouseion.Core.Tests/
└── Mouseion.sln
```

---

## Summary

**Mouseion is a mature, well-documented .NET media server project in active development.** It's a derivative of Radarr (GPL-3.0) with significant expansion to support 10 media types. The codebase follows strict quality standards enforced via SonarCloud, conventional commits, and comprehensive testing. Phase 8 is complete with all technical debt tracked via GitHub issues. Currently in Phase 9 adding manga, comics, and news/RSS support.

**For contributions:**
1. Fork → feature branch → conventional commit → PR to `main`
2. Match existing patterns, zero warnings
3. No TODOs in PRs
4. Tests required for new code

---

*Review generated by Clawdbot*
