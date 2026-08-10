#let outline-page() = {
  pagebreak()
  set par(first-line-indent: 0em)

  show heading: none
  heading([目录], level: 1, outlined: false)

  outline(title: none)
}
