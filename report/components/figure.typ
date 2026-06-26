#import "typography.typ": font, size

#let report-figure(body, caption: [], supplement: [图], label-name: none) = {
  let fig = figure(
    body,
    caption: caption,
    supplement: supplement,
  )
  if label-name == none {
    fig
  } else {
    [#fig #label(label-name)]
  }
}

#let code-block(body, caption: [], label-name: none, lang: none) = {
  let fig = figure(
    block(
      fill: rgb("#fafafa"),
      stroke: rgb("#dddddd"),
      inset: 8pt,
      radius: 3pt,
      width: 100%,
    )[
      #set text(font: font.mono, size: size.five)
      #raw(body, block: true, lang: lang)
    ],
    caption: caption,
    kind: raw,
    supplement: [代码],
  )
  if label-name == none {
    fig
  } else {
    [#fig #label(label-name)]
  }
}
