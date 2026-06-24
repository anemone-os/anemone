#import "template/book.typ": anemone-book, appendices
#import "template/components.typ": *
#import "template/figures.typ": *

#show: anemone-book.with(
  title: [The Anemone Book],
  subtitle: [A Design Narrative Snapshot],
  logo: "../assets/images/anemone.png",
  logo-width: 50%,
  version: [Preliminary Round Snapshot · 2026-06-24],
  build-description: [Built From Commit 86cfec1cc13c],
)

#include "chapters/00-preface.typ"
#include "chapters/01-design-map.typ"
#include "chapters/02-abi-boundary.typ"
#include "chapters/03-tasks-processes-execution-context.typ"
#include "chapters/04-scheduling-waiting-time.typ"
#include "chapters/05-vfs-namespace-pseudo-fs.typ"
#include "chapters/06-device-driver-model-io-objects.typ"
#include "chapters/07-memory-management.typ"
#include "chapters/08-architecture-traps.typ"
#include "chapters/09-next-road.typ"

#appendices[
  #include "appendices/glossary.typ"
  #include "appendices/references.typ"
  #include "appendices/agentic-coding-workflow.typ"
  #include "appendices/version-note.typ"
]
