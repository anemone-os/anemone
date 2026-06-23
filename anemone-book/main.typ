#import "template/book.typ": anemone-book, appendices
#import "template/components.typ": *
#import "template/figures.typ": *

#show: anemone-book.with(
  title: [The Anemone Book],
  subtitle: [A Design Narrative Snapshot],
  logo: "../assets/images/anemone.png",
  logo-width: 50%,
  version: [Preliminary Seed],
  build-description: [Built From Commit 54e7ec1dc44c],
)

#include "chapters/00-preface.typ"
#include "chapters/01-design-map.typ"

#appendices[
  #include "appendices/glossary.typ"
  #include "appendices/references.typ"
  #include "appendices/ai-usage.typ"
  #include "appendices/version-note.typ"
]
