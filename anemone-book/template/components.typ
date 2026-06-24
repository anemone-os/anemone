#import "book.typ": anemone-blue, anemone-ink, anemone-muted, anemone-line, anemone-bg, anemone-code-bg

#let epigraph(quote, attribution: none) = {
  block(
    width: 78%,
    inset: (left: 1.1em, right: 0em, y: 0.5em),
    stroke: (left: 2pt + anemone-line),
    below: 1.4em,
  )[
    #set par(first-line-indent: 0em, justify: false)
    #text(size: 9.5pt, style: "italic", fill: anemone-muted, quote)
    #if attribution != none {
      v(0.35em)
      align(right, text(size: 8.5pt, fill: anemone-muted, attribution))
    }
  ]
}

#let callout(kind, body) = {
  block(
    width: 100%,
    fill: anemone-bg,
    stroke: 0.6pt + anemone-line,
    radius: 3pt,
    inset: 0.85em,
    above: 0.7em,
    below: 0.9em,
  )[
    #set par(first-line-indent: 0em)
    #text(size: 8.5pt, weight: "bold", fill: anemone-blue, upper(kind))
    #v(0.35em)
    #body
  ]
}

#let invariant(body) = callout("Invariant", body)
#let rationale(body) = callout("Rationale", body)
#let tradeoff(body) = callout("Trade-off", body)
#let boundary(body) = callout("Boundary", body)
#let non-goal(body) = callout("Non-goal", body)
#let design-note(body) = callout("Design Note", body)
#let historical-note(body) = callout("Historical Note", body)
#let footgun(body) = callout("Footgun", body)

#let thesis(body) = {
  block(
    width: 100%,
    fill: anemone-code-bg,
    radius: 2pt,
    inset: (x: 0.85em, y: 0.65em),
    below: 1.25em,
  )[
    #set par(first-line-indent: 0em, justify: true)
    #grid(
      columns: (auto, 1fr),
      gutter: 0.7em,
      [
        #text(
          font: ("Noto Sans Mono CJK SC", "Noto Sans CJK SC"),
          size: 10.5pt,
          weight: "bold",
          fill: rgb("#0077ff"),
        )[\$]
      ],
      [
        #text(size: 10pt, fill: anemone-ink, body)
      ],
    )
  ]
}
