-- apx-cite.lua — resolve apx-cite spans from the APX_FACTS sidecar.

local facts_cache = nil

local function load_facts()
  if facts_cache ~= nil then
    return facts_cache
  end

  local path = os.getenv("APX_FACTS")
  if path == nil or path == "" then
    facts_cache = {}
    return facts_cache
  end

  local handle = io.open(path, "r")
  if handle == nil then
    facts_cache = {}
    return facts_cache
  end

  local content = handle:read("*a")
  handle:close()

  local ok, decoded = pcall(pandoc.json.decode, content)
  if ok and type(decoded) == "table" then
    facts_cache = decoded
  else
    facts_cache = {}
  end

  return facts_cache
end

local function footnote_block(text)
  return pandoc.Para(pandoc.Inlines(text))
end

local function cite_inlines(display, source_footnote)
  local inlines = { pandoc.Str(display) }
  if source_footnote ~= nil and source_footnote ~= pandoc.json.null then
    inlines[#inlines + 1] = pandoc.Note({ footnote_block(source_footnote) })
  end
  return inlines
end

function Span(span)
  if not span.classes:includes("apx-cite") then
    return nil
  end

  local fact_id = span.attributes["data-factid"]
  if fact_id == nil or fact_id == "" then
    return pandoc.Str("[missing-fact]")
  end

  local fact = load_facts()[fact_id]
  if fact == nil then
    return pandoc.Str("[missing-fact:" .. fact_id .. "]")
  end

  local display = fact.display
  if type(display) ~= "string" or display == "" then
    return pandoc.Str("[missing-fact:" .. fact_id .. "]")
  end

  return cite_inlines(display, fact.source_footnote)
end
