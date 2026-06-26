#import "components/typography.typ": appendix-numbering, chapter-numbering, display-heading, font, size

#let doc(content) = {
  set document(title: [Anemone 开发报告])
  set page(
    paper: "a4",
    margin: (top: 2.4cm, left: 2.4cm, right: 2.4cm, bottom: 2.2cm),
  )
  set text(lang: "zh", region: "cn", font: font.serif, size: size.small-four)
  set par(first-line-indent: 2em, leading: 1em, justify: true)

  show raw.where(block: false): box.with(
    fill: rgb("#f7f7f7"),
    inset: (x: 3pt, y: 0pt),
    outset: (y: 2pt),
    radius: 2pt,
  )
  show raw.where(block: false): set text(font: font.mono, size: size.five)
  show raw.where(block: true): block.with(
    fill: rgb("#fafafa"),
    stroke: rgb("#dddddd"),
    inset: 8pt,
    radius: 3pt,
    width: 100%,
  )
  show raw.where(block: true): set text(font: font.mono, size: size.five)

  content
}

#let page-header(title) = {
  [
    #set align(center)
    #set par(leading: 0em)
    #text(font: font.serif, size: size.small-five)[操作系统内核设计 - #title]
    #v(2pt, weak: true)
    #line(length: 100%, stroke: 0.6pt)
  ]
}

#let page-footer(numbering) = context {
  align(center)[
    #grid(
      columns: (1fr, 34pt, 1fr),
      gutter: 10pt,
      align: (horizon, center),
      line(length: 100%, stroke: 0.6pt),
      align(center)[#counter(page).display(numbering)],
      line(length: 100%, stroke: 0.6pt),
    )
  ]
}

#let cover(
  project-name: [Anemone],
  team-name: [Anemone],
  teammates: (),
  teachers: (),
  date: (2026, 6),
  logo-path: none,
  body,
) = {
  align(center)[
    #v(20pt)
    #if logo-path != none {
      image(logo-path, width: 100%)
      v(46pt)
    } else {
      v(90pt)
    }

    #text(font: font.serif, size: size.small-zero)[#project-name]
    #v(36pt)
    #text(font: font.serif, size: size.two)[设计文档]
    #v(96pt)

    #let info-key(text-body) = {
      rect(width: 100%, inset: 2pt, stroke: none)[
        #text(font: font.serif, size: size.three)[#text-body]
      ]
    }
    #let info-value(text-body) = {
      rect(
        width: 100%,
        inset: 2pt,
        stroke: (bottom: 1pt + black),
      )[
        #text(font: font.serif, size: size.three)[#text-body]
      ]
    }

    #grid(
      columns: (76pt, 220pt),
      rows: (42pt, 42pt, 42pt),
      gutter: 4pt,
      info-key([参赛队名]), info-value(team-name),
      info-key([队伍成员]), info-value(teammates.join("、")),
      info-key([指导老师]), info-value(teachers.join("、")),
    )

    #v(46pt)
    #text(font: font.serif, size: size.three)[
      #date.at(0) 年 #date.at(1) 月#if date.len() >= 3 [ #date.at(2) 日]
    ]
  ]

  pagebreak()
  body
}

#let frontmatter(title: [Anemone], body) = {
  set page(
    numbering: "I",
    header: page-header(title),
    header-ascent: 15%,
    footer: page-footer("I"),
    footer-descent: 15%,
  )
  counter(page).update(1)

  set heading(numbering: none)
  show heading: it => {
    set par(first-line-indent: 0em)
    if it.level == 1 {
      align(center)[
        #v(1em)
        #display-heading(it, font.serif, size.small-two)
        #v(0.3em)
      ]
    } else {
      display-heading(it, font.serif, size.small-three)
    }
  }

  body
}

#let mainmatter(title: [Anemone], body) = {
  pagebreak()
  set page(
    numbering: "1",
    header: page-header(title),
    header-ascent: 15%,
    footer: page-footer("1"),
    footer-descent: 15%,
  )
  counter(page).update(1)

  set heading(numbering: chapter-numbering)
  show heading: it => {
    set par(first-line-indent: 0em)
    if it.level == 1 {
      pagebreak(weak: true)
      align(center)[
        #v(1em)
        #display-heading(it, font.serif, size.small-two)
        #v(0.3em)
      ]
    } else if it.level == 2 {
      display-heading(it, font.serif, size.small-three)
    } else {
      display-heading(it, font.serif, size.small-four)
    }
  }

  show figure: set figure.caption(position: bottom)
  show figure.where(kind: table): set figure.caption(position: top)

  body
}

#let appendices(body) = {
  pagebreak(weak: true)
  counter(heading).update(0)
  set heading(numbering: appendix-numbering)
  body
}
