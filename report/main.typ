#import "conf.typ": appendices, doc, cover, frontmatter, mainmatter
#import "components/outline.typ": outline-page

#show: doc

#cover(
  project-name: [Anemone],
  team-name: [Anemone],
  teammates: ([待补充],),
  teachers: ([待补充],),
  date: (2026, 6, 24),
  logo-path: "assets/school.jpg",
)[
  #frontmatter(title: [Anemone])[
    #include "content/00-abstract.typ"
    #outline-page()
  ]

  #mainmatter(title: [Anemone])[
    #include "content/01-overview.typ"
    #include "content/02-process-management.typ"
    #include "content/03-scheduling-time.typ"
    #include "content/04-memory.typ"
    #include "content/05-ipc.typ"
    #include "content/06-filesystem.typ"
    #include "content/07-device-driver-model.typ"
    #include "content/08-arch-hal.typ"
    #include "content/09-abi-compat.typ"
    #include "content/10-summary.typ"

    #appendices[
      #include "content/a-engineering-ai.typ"
      #include "content/b-references.typ"
    ]
  ]
]
