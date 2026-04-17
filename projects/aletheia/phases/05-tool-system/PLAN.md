# Phase 05: Tool system

## Goal
40+ built-in tools covering filesystem, HTTP, web search, memory search, and agent coordination.

## Success criteria
- Tool registry loads all built-ins without dynamic linking
- Each tool has a JSON schema describing inputs and outputs
- Tool call latency (excluding I/O) is under 50ms
- Custom tools can be registered from domain packs at runtime

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Tool registry loads all built-ins without dynamic linking | `ldd` or equivalent shows dynamic library dependency for built-in tools |
| Each tool has a JSON schema describing inputs and outputs | Schema validation fails for any built-in tool's input or output shape |
| Tool call latency (excluding I/O) is under 50ms | Microbenchmark shows mean tool dispatch latency >= 50ms |
| Custom tools can be registered from domain packs at runtime | Pack load test shows tool not available after pack registration |

## Scope

### In scope
- organon crate: tool registry, 49 built-in tools
- JSON schema generation for tool signatures
- Domain pack integration with thesauros

### Out of scope
- Third-party tool marketplaces
- GUI tool builder

## Requirements
- REQ-01: Built-in tools include filesystem, HTTP, git, web search, memory search, planning
- REQ-02: Tool schemas are derivable from Rust types via reflection
- REQ-03: Tool execution is sandboxed to instance directory
- REQ-04: Failed tool calls return structured errors, not panics

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Schema generation | Custom derive over manual JSON | Reduces drift between implementation and documentation |
| Sandboxing | Path validation over chroot | Simpler, sufficient for single-user deployments |

## Open questions
- Should tools support streaming responses? (Resolved: yes, for SSE endpoints)

## Dependencies
- Phase 04 complete
- Instance directory structure defined
