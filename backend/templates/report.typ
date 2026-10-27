// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Sorobo – generic report template (EÜR, GuV, Bilanz, UStVA, Kassenbuch, Journal …)

#let data = json("data.json")
#let accent = rgb("#1d4ed8")
#let ink = rgb("#1f2937")
#let muted = ink.lighten(45%)
#let hair = ink.lighten(82%)

#let parse-width(w) = {
  if w == "auto" or w == "" { auto }
  else if w.ends-with("fr") { float(w.trim("fr", at: end)) * 1fr }
  else if w.ends-with("mm") { float(w.trim("mm", at: end)) * 1mm }
  else if w.ends-with("%") { float(w.trim("%", at: end)) * 1% }
  else { auto }
}

#set document(title: data.title, author: data.company)
#set text(font: ("Inter", "Libertinus Serif"), size: 8.4pt, fill: ink, lang: "de", region: "de")
#set par(leading: 0.55em)
#set page(
  paper: "a4",
  flipped: data.landscape,
  margin: (x: 16mm, top: 22mm, bottom: 18mm),
  header: context {
    set text(size: 7pt, fill: muted)
    grid(columns: (1fr, auto), data.company_line, data.title)
    v(-1.6mm)
    line(length: 100%, stroke: 0.4pt + hair)
  },
  footer: context {
    set text(size: 7pt, fill: muted)
    line(length: 100%, stroke: 0.4pt + hair)
    v(-1mm)
    grid(columns: (1fr, auto), data.generated, [Seite #counter(page).display() von #counter(page).final().first()])
  },
)

#text(size: 17pt, weight: "bold", data.title)
#v(-1mm)
#text(size: 9.5pt, fill: muted, data.subtitle)
#if data.tax_line != "" {
  v(-1.5mm)
  text(size: 8pt, fill: muted, data.company + " · " + data.tax_line)
}
#v(4mm)

#if data.summary.len() > 0 {
  block(
    fill: accent.lighten(93%),
    inset: (x: 5mm, y: 4mm),
    radius: 2mm,
    width: 100%,
    grid(
      columns: (1fr,) * calc.min(data.summary.len(), 4),
      row-gutter: 3mm,
      ..data.summary.map(((k, val)) => stack(
        spacing: 1.6mm,
        text(size: 7.5pt, fill: muted, k),
        text(size: 12pt, weight: "bold", val),
      ))
    ),
  )
  v(4mm)
}

#for section in data.sections {
  if section.title != "" {
    block(above: 5mm, below: 2.5mm, text(size: 10.5pt, weight: "bold", fill: accent.darken(20%), section.title))
  }
  let cols = section.columns
  let n = cols.len()
  table(
    columns: cols.map(c => parse-width(c.width)),
    align: (x, y) => (if cols.at(x).align == "right" { right } else if cols.at(x).align == "center" { center } else { left }) + top,
    inset: (x: 4pt, y: 4.2pt),
    stroke: (x, y) => if y == 0 { (bottom: 0.8pt + accent) } else { (bottom: 0.3pt + hair) },
    fill: (x, y) => if y == 0 { accent.lighten(92%) } else { none },
    table.header(..cols.map(c => text(weight: "bold", size: 7.6pt, fill: accent.darken(25%), c.label))),
    ..section.rows.map(r => {
      let st = r.style
      if st == "header" {
        (table.cell(colspan: n, fill: luma(245), text(weight: "bold", r.cells.at(0))),)
      } else {
        r.cells.enumerate().map(((i, c)) => {
          if st == "total" { text(weight: "bold", size: 9pt, c) }
          else if st == "subtotal" or st == "bold" { text(weight: "semibold", c) }
          else if st == "muted" { text(fill: muted, c) }
          else if st == "indent" and i == 0 { pad(left: 4mm, text(fill: muted, c)) }
          else if st == "indent" { text(fill: muted, c) }
          else { c }
        })
      }
    }).flatten(),
  )
}

#if data.notes.len() > 0 {
  v(4mm)
  for n in data.notes {
    block(below: 1.5mm, text(size: 7.5pt, fill: muted, "Hinweis: " + n))
  }
}
