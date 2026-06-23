#import "book.typ": anemone-muted

#let book-figure(path, caption, width: 92%) = {
  figure(
    image(path, width: width),
    caption: caption,
  )
}

#let listing(caption, body) = {
  figure(
    kind: raw,
    supplement: [Listing],
    body,
    caption: caption,
  )
}

#let figure-note(body) = {
  block(above: -0.4em, below: 0.8em)[
    #set par(first-line-indent: 0em, justify: true)
    #text(size: 8.7pt, fill: anemone-muted, body)
  ]
}
