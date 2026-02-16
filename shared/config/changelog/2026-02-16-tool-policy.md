# Tool Policy Simplification — 2026-02-16

## Change
Replaced per-agent `allow` lists with uniform `deny` lists.

## Before
- Syn: `deny: ['gateway']`
- Akron: `allow: ['read', 'write', 'edit', 'ls', 'find', 'grep', 'exec', 'web_search', 'web_fetch', 'mem0_search', 'message']`
- Eiron: `allow: ['read', 'write', 'edit', 'ls', 'find', 'grep', 'web_search', 'web_fetch', 'browser', 'mem0_search', 'message']`
- Demiurge: `allow: ['read', 'write', 'edit', 'ls', 'find', 'grep', 'web_search', 'web_fetch', 'browser', 'mem0_search', 'message']`
- Syl: `allow: ['read', 'ls', 'web_search', 'web_fetch', 'mem0_search', 'message']`
- Arbor: `allow: ['read', 'ls', 'web_search', 'web_fetch', 'mem0_search', 'message']`

## After
- Syn: `{}` (no restrictions — full access including gateway)
- All others: `deny: ['gateway']`

## Rationale
- `allow` lists required manual updates whenever new tools were added to the runtime
- Agents were missing critical capabilities (inter-agent comms, exec, file writes)
- Syl and Arbor couldn't even write files — too restrictive for functional agents
- Syn as the Nous should have full runtime access with soft protections (validate before restart, doctor before changes)
- Only `gateway` genuinely needs restriction from specialist agents

## Validated
- `aletheia doctor` passed
- SIGUSR1 config reload sent
