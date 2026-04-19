// poiesis-typst default template.
//
// A minimal one-page report scaffold. The template loads JSON data injected
// via `render_typst` and renders title, author, body paragraphs, and an
// optional table. Keep this intentionally small — it is used for library
// smoke tests and as a baseline for user templates.

#let data = json("data.json")

#set page(paper: "us-letter", margin: 0.75in)
#set text(font: "Liberation Sans", size: 10pt)
#set par(leading: 0.65em, spacing: 0.8em)

// Title block
#align(left)[
  #text(18pt, weight: "bold")[#data.at("title", default: "Untitled Report")]
  #if "author" in data [
    #v(4pt)
    #text(10pt, fill: rgb("#666666"))[By #data.author]
  ]
]

#v(16pt)

// Optional subtitle
#if "subtitle" in data [
  #text(12pt, style: "italic")[#data.subtitle]
  #v(8pt)
]

// Body paragraphs
#if "body" in data {
  for paragraph in data.body [
    #paragraph

    #v(0.4em)
  ]
}

// Optional table: data.table = (columns: n, header: [...], rows: [[...], ...])
#if "table" in data {
  let t = data.table
  let columns = int(t.at("columns", default: t.header.len()))
  v(8pt)
  table(
    columns: columns,
    fill: (_, y) => if y == 0 { rgb("#333333") } else { none },
    stroke: 0.5pt + rgb("#cccccc"),
    inset: 6pt,
    ..t.header.map(h => text(weight: "bold", fill: white, h)),
    ..t.rows.flatten(),
  )
}

// Optional footer note
#if "footer" in data [
  #v(12pt)
  #text(9pt, fill: rgb("#666666"), style: "italic")[#data.footer]
]
