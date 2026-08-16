#import "conf.typ": appendices, cover, doc, frontmatter, mainmatter
#import "components/outline.typ": outline-page

#show: doc

#cover(
  project-name: [Anemone],
  team-name: [ヰ世界电子开发部],
  teammates: ([张正翰], [陈函申]),
  teachers: ([夏文], [仇洁婷]),
  date: (2026, 8, 16),
  logo-path: "assets/school.jpg",
)[
  #frontmatter(title: [Anemone])[
    #include "content/00-abstract.typ"
    #outline-page()
  ]

  #mainmatter(title: [Anemone])[
    #include "content/01-overview.typ"
    #include "content/02-nemophila.typ"
    #include "content/03-process-management.typ"
    #include "content/04-scheduling.typ"
    #include "content/05-memory.typ"
    #include "content/06-ipc.typ"
    #include "content/07-filesystem.typ"
    #include "content/08-device-driver-model.typ"
    #include "content/09-network-stack.typ"
    #include "content/10-time.typ"
    #include "content/11-arch-hal.typ"
    #include "content/12-build-system.typ"
    #include "content/13-summary.typ"

    #appendices[
      #include "content/a-engineering-ai.typ"
      #include "content/b-references.typ"
    ]
  ]
]
