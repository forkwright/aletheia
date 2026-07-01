-- NOTE: apx-figure.lua swaps chart figures to SVG or PNG assets per writer.

local figures_cache = nil
local svg_counter = 0

local function load_figures()
  if figures_cache ~= nil then
    return figures_cache
  end

  local path = os.getenv("APX_FIGURES")
  if path == nil or path == "" then
    figures_cache = {}
    return figures_cache
  end

  local loader, err = loadfile(path)
  if loader == nil then
    figures_cache = {}
    return figures_cache
  end

  local ok, decoded = pcall(loader)
  if ok and type(decoded) == "table" then
    figures_cache = decoded
  else
    figures_cache = {}
  end

  return figures_cache
end

local function is_raster_format()
  if FORMAT == "docx" or FORMAT == "odt" or FORMAT == "latex" or FORMAT == "pdf" then
    return true
  end

  return FORMAT:match("epub") ~= nil
end

local function figure_id(img)
  if type(img.identifier) == "string" and img.identifier:match("^apx%-figure%-%d+$") then
    return img.identifier
  end

  local fallback = img.attributes["data-figure-id"]
  if type(fallback) == "string" and fallback:match("^apx%-figure%-%d+$") then
    return fallback
  end

  return nil
end

local function ensure_svg_file(figure_id_value, svg)
  svg_counter = svg_counter + 1
  local tmpdir = os.getenv("APX_TMPDIR") or os.getenv("TMPDIR") or "."
  local sep = package.config:sub(1, 1)
  local path = tmpdir .. sep .. figure_id_value .. "-" .. tostring(svg_counter) .. ".svg"
  local handle = assert(io.open(path, "w"))
  handle:write(svg)
  handle:close()
  return path
end

local function replace_image(img)
  local id = figure_id(img)
  if id == nil then
    return nil
  end

  local figure = load_figures()[id]
  if figure == nil then
    return pandoc.Str("[missing-figure:" .. id .. "]")
  end

  if is_raster_format() then
    local png_path = figure.png_path
    if type(png_path) ~= "string" or png_path == "" then
      return pandoc.Str("[missing-figure:" .. id .. "]")
    end
    return pandoc.Image(img.caption, png_path, img.title, img.attr)
  end

  local svg = figure.svg
  if type(svg) ~= "string" or svg == "" then
    return pandoc.Str("[missing-figure:" .. id .. "]")
  end

  if FORMAT:match("html") ~= nil then
    return pandoc.RawInline("html", svg)
  end

  local svg_path = figure.svg_path
  if type(svg_path) ~= "string" or svg_path == "" then
    svg_path = ensure_svg_file(id, svg)
    figure.svg_path = svg_path
  end

  return pandoc.Image(img.caption, svg_path, img.title, img.attr)
end

function Image(img)
  return replace_image(img)
end

function Figure(fig)
  return fig:walk({
    Image = replace_image,
  })
end
