# External Source References

This directory stores metadata for the repository's curated external source
references. Source checkouts are Git-ignored local materializations under
`xref/<id>`; they are not Anemone source code, build inputs, current contracts,
or RFC targets.

`sources.toml` is the canonical registry. Each entry contains:

- an immutable `id`, which is also the checkout directory name;
- an English `scope` describing what the source is useful for and what
  authority it does not carry;
- the canonical read-only HTTPS Git `url`;
- an optional upstream release `tag`;
- the full `commit` identifying the selected content.

Once an ID is referenced by public documentation, it must not be redirected to
different content; add a new entry for each distinct source snapshot. A tag is
only a retrieval and provenance hint, while the commit is the content identity.
xref verifies that the peeled tag resolves to the registered commit.

Use the repository entry points to inspect or materialize source references:

```text
just xref list
just xref fetch linux-6.6.32
just xref fetch --all
just xref check linux-6.6.32
just xref check --all
```

`fetch` clones the source into `xref/<id>` and checks it out at a detached HEAD
without initializing upstream submodules. An existing matching clean checkout
succeeds idempotently. An existing non-Git directory, a mismatched origin or
commit, or a dirty checkout causes an error and is left unmodified. Normal
build, test, and documentation workflows neither fetch nor depend on these
sources.

Public evidence must use the canonical form defined by the
[External Source Reference Policy](../docs/src/external-source-references.md):
`xref:<source-id>:<repo-relative-path>#<locator>`. Do not cite private checkout
paths. Access to a reference checkout does not grant permission to copy upstream
code into Anemone; review the upstream license and this repository's licensing
boundary before copying any code.
