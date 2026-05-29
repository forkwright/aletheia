--[[
apx-figure.lua — select SVG vs PNG per writer for Figure blocks.

STUB: passes through unchanged. Full implementation requires:
- B-005: poiesis-charts SVG emitter API
- B-012 ESCALATION.md Q4: APX_FIGURES sidecar JSON schema
- APX_FIGURES env var: JSON mapping figure_id -> { svg, png_path }

Format rules (planned):
- html / latex / typst-pdf: inline/linked SVG
- docx / odt / epub: PNG bake via resvg at print density

See ESCALATION.md Q4.
]]

-- Identity filter: return blocks unchanged until wired.
return {}
