--[[
apx-theme.lua — map Note{kind} Divs to per-target admonition rendering.

STUB: passes through unchanged. Full implementation requires:
- B-001: Note{kind} Div blocks in the Document AST
- B-002: doc-vars YAML for theme-driven admonition styles
- Per-target rendering rules:
  - html/docx: styled Div with class "admonition-{kind}"
  - latex: \begin{tcolorbox}[title={kind}]
  - typst: callout fn from theme template

See B-006 § 5 (Lua filters) for the planned behaviour.
]]

-- Identity filter: return blocks unchanged until wired.
return {}
