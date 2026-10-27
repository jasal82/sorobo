// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Sorobo – Geschäftsdokumente (Rechnung, Angebot, Auftragsbestätigung,
// Lieferschein, Gutschrift, Mahnung). All data is provided pre-formatted in
// data.json; positions are given in millimetres from the top left page corner.

#let data = json("data.json")
#let L = data.layout
#let C = data.company
#let D = data.doc

#let accent = rgb(L.accent_color)
#let ink = rgb(L.text_color)
#let muted = ink.lighten(42%)
#let hair = ink.lighten(80%)
#let u(x) = x * 1mm

// Multi-line text: blank lines become paragraphs, single newlines line breaks.
#let txt(s) = {
  if s == none or s == "" { return none }
  let paras = s.split("\n\n")
  for (i, p) in paras.enumerate() {
    if i > 0 { parbreak() }
    p.split("\n").join(linebreak())
  }
}

#set document(title: D.heading, author: C.name)
#set text(font: (L.font, "Inter", "Libertinus Serif"), size: L.font_size * 1pt, fill: ink, lang: "de", region: "de")
#set par(leading: 0.62em, spacing: 1.05em, justify: false)

// ---------------------------------------------------------------- page parts
#let info-grid = grid(
  columns: (auto, 1fr),
  column-gutter: 4mm,
  row-gutter: 1.7mm,
  ..D.info.map(((k, val)) => (
    text(fill: muted, size: 0.92em, k),
    align(right, text(size: 0.92em, val)),
  )).flatten()
)

#let company-block = {
  let al = if L.company_align == "right" { right } else { left }
  block(width: u(L.company_width), align(al, text(size: 0.84em, fill: muted)[
    #text(weight: "bold", fill: ink, size: 1.08em, C.name) \
    #C.address.slice(1).join(linebreak())
    #if C.contact.len() > 0 [ \ #C.contact.join(linebreak())]
  ]))
}

#let fold-marks = {
  let s = 0.35pt + ink.lighten(60%)
  place(top + left, dx: 4mm, dy: 105mm, line(length: 5mm, stroke: s))
  place(top + left, dx: 4mm, dy: 148.5mm, line(length: 7mm, stroke: s))
  place(top + left, dx: 4mm, dy: 210mm, line(length: 5mm, stroke: s))
}

#let first-page = {
  if L.accent_bar {
    place(top + left, rect(width: 210mm, height: 3.5mm, fill: accent))
  }
  if L.logo != none {
    place(top + left, dx: u(L.logo_x), dy: u(L.logo_y), image(L.logo, width: u(L.logo_width)))
  }
  if L.show_company_block {
    place(top + left, dx: u(L.company_x), dy: u(L.company_y), company-block)
  }
  place(top + left, dx: u(L.address_x), dy: u(L.address_y), block(
    width: u(L.address_width),
    height: u(L.address_height),
    clip: true,
  )[
    #if L.show_sender_line {
      block(below: 2.5mm, stack(
        spacing: 1.2mm,
        text(size: 6.5pt, fill: muted, C.sender_line),
        line(length: 100%, stroke: 0.3pt + hair),
      ))
    }
    #set par(leading: 0.5em)
    #D.recipient.join(linebreak())
  ])
  place(top + left, dx: u(L.info_x), dy: u(L.info_y), block(width: u(L.info_width), info-grid))
}

// Footer columns use their natural width, separated by flexible space. If they do not fit
// into the type area, the font is scaled down (instead of wrapping lines like "IBAN: …").
#let footer-block = {
  set text(size: L.footer_size * 1pt, fill: muted)
  set par(leading: 0.42em)
  if L.show_footer and C.footer.len() > 0 {
    line(length: 100%, stroke: 0.4pt + hair)
    v(1.4mm)
    let gutter = 3mm
    layout(size => {
      let contents = C.footer.map(col => col.join(linebreak()))
      let natural = contents.map(c => measure(c).width).sum() + gutter * (contents.len() - 1) * 2
      let scale = if natural > size.width { size.width / natural * 0.99 } else { 1 }
      set text(size: L.footer_size * 1pt * scale)
      let cols = ()
      let cells = ()
      for (i, c) in contents.enumerate() {
        if i > 0 {
          cols.push(1fr)
          cells.push([])
        }
        cols.push(auto)
        cells.push(c)
      }
      grid(columns: cols, column-gutter: gutter, ..cells)
    })
  }
}

#set page(
  paper: "a4",
  margin: (top: u(L.margin_top), bottom: u(L.margin_bottom), left: u(L.margin_left), right: u(L.margin_right)),
  background: context {
    let p = here().page()
    if p == 1 and L.letterhead_first != none {
      image(L.letterhead_first, width: 100%, height: 100%)
    } else if p > 1 and L.letterhead_other != none {
      image(L.letterhead_other, width: 100%, height: 100%)
    }
    if L.fold_marks and p == 1 { fold-marks }
  },
  foreground: context {
    let p = here().page()
    if p == 1 { first-page } else if L.logo_all_pages and L.logo != none {
      place(top + left, dx: u(L.logo_x), dy: u(L.logo_y), image(L.logo, width: u(L.logo_width) * 0.7))
    }
    if D.is_draft {
      place(center + horizon, rotate(-35deg, text(size: 96pt, weight: "bold", fill: ink.transparentize(91%), "ENTWURF")))
    }
  },
  header: context {
    if here().page() > 1 {
      set text(size: 0.8em, fill: muted)
      grid(columns: (1fr, auto), [#D.label #D.number], [Seite #counter(page).display() von #counter(page).final().first()])
      v(-1.5mm)
      line(length: 100%, stroke: 0.4pt + hair)
    }
  },
  footer-descent: 22%,
  footer: context {
    let total = counter(page).final().first()
    if L.show_page_numbers and total > 1 {
      place(top + right, dy: -4.2mm, text(size: 6.8pt, fill: muted)[Seite #counter(page).display() von #total])
    }
    footer-block
  },
)

// ---------------------------------------------------------------- body
#if D.at("attachment", default: none) != none {
  pdf.attach(
    D.attachment,
    relationship: "alternative",
    mime-type: "text/xml",
    description: "Factur-X / ZUGFeRD Rechnungsdaten (EN 16931)",
  )
}

#v(u(L.content_top - L.margin_top))

#text(size: L.title_size * 1pt, weight: "bold", fill: ink, D.heading)
#v(4mm)

#if D.intro != "" {
  block(below: 5mm, txt(D.intro))
}

// Positions table
#let cols = D.columns
#let spec = {
  let s = ()
  if cols.pos { s.push((key: "pos", label: "Pos.", width: 9mm, al: left)) }
  if cols.number { s.push((key: "number", label: "Art.-Nr.", width: auto, al: left)) }
  s.push((key: "name", label: "Bezeichnung", width: 1fr, al: left))
  s.push((key: "qty", label: "Menge", width: auto, al: right))
  if cols.unit { s.push((key: "unit", label: "Einheit", width: auto, al: left)) }
  if cols.prices {
    s.push((key: "price", label: D.price_label, width: auto, al: right))
    if cols.discount { s.push((key: "discount", label: "Rabatt", width: auto, al: right)) }
    if cols.tax { s.push((key: "tax", label: "USt", width: auto, al: right)) }
    s.push((key: "total", label: D.total_label, width: auto, al: right))
  }
  s
}
#let ncols = spec.len()

#let header-fill = if L.table_header_fill { accent.lighten(90%) } else { none }
#let header-color = if L.table_header_fill { accent.darken(25%) } else { ink }

#let table-stroke = if L.table_style == "grid" {
  0.4pt + hair
} else if L.table_style == "minimal" {
  (x, y) => if y == 0 { (bottom: 0.8pt + ink) } else { none }
} else if L.table_style == "striped" {
  (x, y) => if y == 0 { (bottom: 0.8pt + accent) } else { none }
} else {
  (x, y) => if y == 0 { (bottom: 0.8pt + accent) } else { (bottom: 0.4pt + hair) }
}

#let table-fill = (x, y) => {
  if y == 0 { header-fill } else if L.table_style == "striped" and calc.even(y) { luma(246) } else { none }
}

#let name-cell(r) = stack(
  spacing: 1.6mm,
  text(weight: "semibold", r.name),
  ..if r.description != "" { (text(size: 0.88em, fill: muted, txt(r.description)),) } else { () },
)

#let row-cells(r) = {
  if r.type == "text" {
    (table.cell(colspan: ncols, txt(r.name + if r.description != "" { "\n" + r.description } else { "" })),)
  } else if r.type == "subtotal" {
    let n = if cols.prices { ncols - 1 } else { ncols }
    let cells = (table.cell(colspan: n, align(right, text(weight: "semibold", r.name))),)
    if cols.prices { cells.push(text(weight: "semibold", r.total)) }
    cells
  } else if r.type == "item" {
    spec.map(c => if c.key == "name" { name-cell(r) } else { r.at(c.key) })
  } else {
    ()
  }
}

#if D.rows.len() > 0 {
  table(
    columns: spec.map(c => c.width),
    align: (x, y) => spec.at(x).al + top,
    inset: (x: 4.5pt, y: 5.5pt),
    stroke: table-stroke,
    fill: table-fill,
    table.header(..spec.map(c => text(weight: "bold", size: 0.9em, fill: header-color, c.label))),
    ..D.rows.map(row-cells).flatten(),
  )
}

// Custom table (e.g. dunning letters)
#if D.custom_table != none {
  let ct = D.custom_table
  // Without column titles (e.g. the fields of a mandate form) there is no header row.
  let headed = ct.columns.any(c => c != "")
  table(
    columns: ct.columns.enumerate().map(((i, _)) => if i == 0 { 1fr } else { auto }),
    align: (x, y) => (if ct.align.at(x) == "right" { right } else { left }) + top,
    inset: (x: 4.5pt, y: 5.5pt),
    stroke: (x, y) => if headed and y == 0 { (bottom: 0.8pt + accent) } else { (bottom: 0.4pt + hair) },
    fill: (x, y) => if headed and y == 0 { header-fill } else { none },
    ..if headed { (table.header(..ct.columns.map(c => text(weight: "bold", size: 0.9em, fill: header-color, c))),) } else { () },
    ..ct.rows.flatten(),
  )
}

// Totals (right) and GiroCode (left)
#let giro-block = if D.girocode != none {
  block(breakable: false, width: 68mm, grid(
    columns: (20mm, 1fr),
    column-gutter: 3mm,
    align: (left + top, left + top),
    image(D.girocode.file, width: 20mm),
    text(size: 0.78em, fill: muted)[
      #text(weight: "semibold", fill: ink)[Bequem bezahlen per GiroCode] \
      Mit der Banking-App scannen – Empfänger, IBAN, Betrag (#D.girocode.amount) und Verwendungszweck #D.girocode.reference werden übernommen.
    ],
  ))
} else { none }

#let totals-block = if D.totals.len() > 0 {
  let rows = ()
  for t in D.totals {
    if t.style == "total" {
      rows.push(grid.cell(colspan: 2, line(length: 100%, stroke: 0.9pt + accent)))
      rows.push(text(weight: "bold", size: 1.08em, t.label))
      rows.push(text(weight: "bold", size: 1.08em, t.value))
    } else if t.style == "subtotal" {
      rows.push(grid.cell(colspan: 2, line(length: 100%, stroke: 0.4pt + hair)))
      rows.push(text(weight: "semibold", t.label))
      rows.push(text(weight: "semibold", t.value))
    } else if t.style == "muted" {
      rows.push(text(size: 0.88em, fill: muted, t.label))
      rows.push(text(size: 0.88em, fill: muted, t.value))
    } else {
      rows.push(t.label)
      rows.push(t.value)
    }
  }
  block(breakable: false, width: 92mm, grid(
    columns: (1fr, auto),
    column-gutter: 6mm,
    row-gutter: 2.1mm,
    align: (left, right),
    ..rows,
  ))
} else { none }

#if totals-block != none or giro-block != none {
  v(2mm)
  grid(
    columns: (1fr, auto),
    column-gutter: 6mm,
    align: (left + bottom, right + top),
    if giro-block != none { giro-block } else { [] },
    if totals-block != none { totals-block } else { [] },
  )
}

#if D.notes.len() > 0 {
  v(3mm)
  block(text(size: 0.9em, fill: muted, D.notes.join(linebreak())))
}

#if D.closing != "" {
  v(4mm)
  block(breakable: false, txt(D.closing))
}
