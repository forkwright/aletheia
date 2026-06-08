-- apx-theme.lua — map apx-note blocks to target-specific admonitions.

local function normalize_kind(kind)
  if kind == nil or kind == "" then
    return "note"
  end

  kind = kind:lower()
  if kind == "warning" or kind == "tip" or kind == "important" or kind == "note" then
    return kind
  end

  return "note"
end

local function title_case(kind)
  return kind:sub(1, 1):upper() .. kind:sub(2)
end

local function append_title(blocks, kind)
  blocks[#blocks + 1] = pandoc.Para({ pandoc.Strong(title_case(kind)) })
end

local function append_content(blocks, content)
  for _, block in ipairs(content) do
    blocks[#blocks + 1] = block
  end
end

function Div(div)
  if not div.classes:includes("apx-note") then
    return nil
  end

  local kind = normalize_kind(div.attributes["data-kind"])
  local title = title_case(kind)
  local attr = pandoc.Attr(
    div.identifier,
    { "apx-note", "admonition", "admonition-" .. kind },
    {
      ["data-kind"] = kind,
    }
  )

  if FORMAT == "latex" then
    local blocks = { pandoc.RawBlock("latex", "\\begin{tcolorbox}[title={" .. title .. "}]") }
    append_content(blocks, div.content)
    blocks[#blocks + 1] = pandoc.RawBlock("latex", "\\end{tcolorbox}")
    return blocks
  end

  local blocks = {}
  append_title(blocks, kind)
  append_content(blocks, div.content)
  return pandoc.Div(blocks, attr)
end
