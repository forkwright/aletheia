# Phase 11: Terminal dashboard

## Goal
Rich TUI with markdown rendering, session management, and real-time streaming.

## Success criteria
- TUI renders markdown with code highlighting and tables
- Session list loads 1000 sessions in under 500ms
- Real-time streaming displays tokens as they arrive with < 100ms latency
- Keyboard navigation supports all features without mouse

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| TUI renders markdown with code highlighting and tables | Visual test shows unrendered markdown or broken table layout |
| Session list loads 1000 sessions in under 500ms | Profiler shows list rendering >= 500ms |
| Real-time streaming displays tokens as they arrive with < 100ms latency | Benchmark shows token-to-screen latency >= 100ms |
| Keyboard navigation supports all features without mouse | Accessibility audit shows unreachable interactive element |

## Scope

### In scope
- koilon crate: terminal dashboard
- theatron core: shared API client, SSE infrastructure
- Markdown renderer with syntax highlighting

### Out of scope
- Mouse-driven UI (supported but not required)
- Image rendering in terminal

## Requirements
- REQ-01: TUI uses ratatui for layout and widgets
- REQ-02: Markdown renderer supports CommonMark subset
- REQ-03: Session list supports search and sort
- REQ-04: Streaming uses dedicated async task with bounded channel

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| TUI framework | ratatui over cursive | Larger ecosystem, better async support |
| Markdown parser | pulldown-cmark over comrak | Faster, sufficient for our subset |

## Open questions
- Should we support inline images via kitty graphics protocol? (Deferred)

## Dependencies
- Phase 10 complete
- Terminal with 256-color support
