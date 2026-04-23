// {{slug}}

#let data = json("data.json")

#set page(paper: "us-letter", margin: 0.75in)
#set text(font: "Liberation Sans", size: 10pt)
#set par(leading: 0.65em, spacing: 0.8em)

{{confidential_header}}
#align(left)[
  #text(18pt, weight: "bold")[#data.at("title", default: "Untitled Report")]
  #v(4pt)
  #text(10pt, fill: rgb("#666666"))[#data.at("description", default: "")]
]
#v(16pt)

// Body paragraphs
#if "body" in data {
  for paragraph in data.body [
    #paragraph
    #v(0.4em)
  ]
}

{{confidential_footer}}
