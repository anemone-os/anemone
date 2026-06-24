#import "book.typ": anemone-muted, anemone-line, anemone-code-bg

#let listing-numbering(..nums) = context {
  let parts = nums.pos()
  let chapter = counter(heading.where(level: 1)).get().first() - 1
  str(chapter) + "." + parts.map(str).join(".")
}

#let book-figure(path, caption, width: 92%) = {
  figure(
    supplement: [Fig.],
    numbering: listing-numbering,
    image(path, width: width),
    caption: caption,
  )
}

#let listing(caption, body) = {
  figure(
    kind: raw,
    supplement: [Listing],
    numbering: listing-numbering,
    block(
      width: 100%,
      fill: anemone-code-bg,
      stroke: 0.5pt + anemone-line,
      radius: 3pt,
      inset: 0.75em,
    )[
      #set align(left)
      #set par(first-line-indent: 0em, justify: false)
      #body
    ],
    caption: caption,
  )
}

#let figure-note(body) = {
  block(above: -0.4em, below: 0.8em)[
    #set par(first-line-indent: 0em, justify: true)
    #text(size: 8.7pt, fill: anemone-muted, body)
  ]
}
