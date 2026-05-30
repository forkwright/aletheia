--[[
apx-cite.lua — resolve apx-cite Span elements to formatted fact numbers.

STUB: passes through unchanged. Full implementation requires:
- B-001: Cite(FactId) inline type in the Document AST
- B-012 ESCALATION.md Q1: APX_FACTS sidecar JSON schema
- APX_FACTS env var: JSON mapping FactId -> { display, source_footnote }

Pipeline order: runs after AST emit, before writer.
See ESCALATION.md Q3 for the sidecar contract.
]]

-- Identity filter: return blocks unchanged until wired.
return {}
