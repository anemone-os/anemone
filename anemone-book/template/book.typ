#let anemone-blue = rgb("#174a7c")
#let anemone-ink = rgb("#1b1f24")
#let anemone-muted = rgb("#5b6470")
#let anemone-line = rgb("#d6dce3")
#let anemone-bg = rgb("#f7f9fb")

#let chapter-numbering = num => {
  "§" + str(num - 1)
}

#let anemone-heading-numbering(..nums) = {
  let parts = nums.pos()
  if parts.len() == 0 {
    return ""
  }
  if parts.len() == 1 {
    return chapter-numbering(parts.first())
  }
  chapter-numbering(parts.first()) + "." + parts.slice(1).map(str).join(".")
}

#let cover(
  title: none,
  subtitle: none,
  logo: none,
  logo-width: 24%,
  version: none,
  build-description: none,
) = {
  set align(center)
  v(1fr)
  if logo != none {
    image(logo, width: logo-width)
    v(1.2cm)
  }
  text(30pt, weight: "bold", fill: anemone-ink, title)
  if subtitle != none {
    v(0.45cm)
    text(12pt, fill: anemone-muted, subtitle)
  }
  v(0.8cm)
  line(length: 34%, stroke: 0.6pt + anemone-line)
  v(0.7cm)
  if version != none {
    text(9pt, fill: anemone-muted, version)
  }
  if build-description != none {
    v(0.25cm)
    text(8.5pt, fill: anemone-muted, build-description)
  }
  v(1fr)
  text(8pt, fill: anemone-muted)[Anemone OS]
  pagebreak()
}

#let anemone-book(
  title: [The Anemone Book],
  subtitle: none,
  logo: none,
  logo-width: 24%,
  version: none,
  build-description: none,
  body,
) = {
  set document(title: "The Anemone Book")
  set page(
    paper: "a4",
    margin: (top: 2.4cm, bottom: 2.2cm, x: 2.25cm),
    numbering: none,
    header: none,
  )
  set text(
    font: ("Noto Serif CJK SC", "Libertinus Serif"),
    size: 10.5pt,
    lang: "zh",
    fill: anemone-ink,
  )
  set par(justify: true, leading: 0.72em, first-line-indent: 1em)
  set heading(numbering: anemone-heading-numbering)
  set list(indent: 1.2em)
  set enum(indent: 1.2em, numbering: "1.")
  set raw(block: true)
  show raw: set text(font: ("Noto Sans Mono CJK SC", "Noto Sans CJK SC"), size: 8.8pt)
  show link: set text(fill: anemone-blue)
  show heading.where(level: 1): it => {
    pagebreak(weak: true)
    block(below: 1.2em)[
      #set par(first-line-indent: 0em, justify: false)
      #text(11pt, fill: anemone-blue, weight: "bold", counter(heading).display())
      #v(0.25em)
      #text(22pt, weight: "bold", it.body)
      #v(0.35em)
      #line(length: 100%, stroke: 0.7pt + anemone-line)
    ]
  }
  show heading.where(level: 2): it => {
    block(above: 1.1em, below: 0.7em)[
      #set par(first-line-indent: 0em, justify: false)
      #text(14pt, weight: "bold", fill: anemone-ink, it.body)
    ]
  }
  show heading.where(level: 3): it => {
    block(above: 0.9em, below: 0.45em)[
      #set par(first-line-indent: 0em, justify: false)
      #text(11.5pt, weight: "bold", fill: anemone-ink, it.body)
    ]
  }

  cover(
    title: title,
    subtitle: subtitle,
    logo: logo,
    logo-width: logo-width,
    version: version,
    build-description: build-description,
  )
  text(18pt, weight: "bold")[Contents]
  v(0.6em)
  outline(title: none, depth: 3, indent: auto)
  pagebreak()
  counter(page).update(1)
  set page(
    paper: "a4",
    margin: (top: 2.4cm, bottom: 2.2cm, x: 2.25cm),
    numbering: "1",
    header: context {
      text(8pt, fill: anemone-muted)[The Anemone Book]
      h(1fr)
      text(8pt, fill: anemone-muted, counter(page).display())
    },
  )
  body
}

#let appendices(body) = {
  pagebreak(weak: true)
  counter(heading).update(0)
  set heading(numbering: "A")
  body
}
