# Getting Started

This is the starter domain pack — a minimal example you can copy and adapt.

## What domain packs do

A domain pack injects context files and tools into an agent at startup. Use them to
provide agent-specific knowledge (schemas, runbooks, policies) without touching the
core runtime.

## How to customise this pack

1. Replace this file with your own documentation.
2. Add more context files under `context/` and reference them in `pack.toml`.
3. Add executable scripts under `tools/` to expose them as LLM-callable tools.
4. Use `agents = ["my-agent"]` on a context entry to restrict it to one agent.
5. Use `[overlays.my-agent]` with `domains = ["my-domain"]` to tag agents by role.

## Full documentation

See `docs/PACKS.md` for the complete pack format reference.
