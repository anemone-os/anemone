#let size = (
  small-five: 9pt,
  five: 10.5pt,
  small-four: 12pt,
  four: 14pt,
  small-three: 15pt,
  three: 16pt,
  small-two: 18pt,
  two: 22pt,
  small-one: 24pt,
  one: 26pt,
  small-zero: 36pt,
)


#let font = (
  serif: ("Noto Serif CJK SC", "Libertinus Serif"),
  sans: ("Noto Sans CJK SC", "Inter"),
  mono: ("JetBrains Mono", "Noto Sans Mono CJK SC"),
  title: ("Noto Sans CJK SC", "Inter"),
)

#let chapter-numbering(..nums) = {
  let ns = nums.pos()
  if ns.len() == 1 {
    numbering("1", ..ns)
  } else {
    numbering("1.1", ..ns)
  }
}

#let appendix-numbering(..nums) = {
  let ns = nums.pos()
  if ns.len() == 1 {
    numbering("A", ns.first())
  } else [
    #numbering("A", ns.first()).#numbering("1.1", ..ns.slice(1))
  ]
}

#let display-heading(it, font-family, font-size) = {
  set text(font: font-family, size: font-size)
  set par(first-line-indent: 0em)
  if it.numbering != none [
    #counter(heading).display() #h(0.75em)#it.body
  ] else [
    #it.body
  ]
}
