---
name: anemone-book-workflow
description: >-
  Use when editing, reviewing, planning, or validating The Anemone Book under
  anemone-book, including Typst chapters, templates, figures, refs.bib, meta
  files, source passes, chapter briefs, editor/reviewer/chapter-writer handoffs,
  or book-specific AGENTS rules. Enforces the anemone-book/meta workflow,
  artifact boundaries, write-set discipline, source verification, figure/source
  handling, and Typst or whitespace validation.
---

# Anemone Book Workflow

## Purpose

Use this skill as the entry protocol for `anemone-book`. Keep the book rules in
`anemone-book/meta/`; do not duplicate them here. This skill exists so agents
load the right meta files before writing, reviewing, or validating the book.

## Required Reading

Always read these before changing or reviewing `anemone-book`:

- `anemone-book/README.md`
- `anemone-book/meta/workflow.md`

Then read the task-specific meta files:

- Positioning, audience, non-goals, artifact boundary, or whole-book direction:
  `anemone-book/meta/positioning.md`
- Chapter structure, module coverage, case studies, figures, or listing choices:
  `anemone-book/meta/outline.md`
- Prose, titles, section numbering, Typst paragraph wrapping, callouts, captions,
  terminology, or visual style: `anemone-book/meta/style.md`
- Technical facts, references, quotes, source material, or accepted limitations:
  `anemone-book/meta/sources.md`, then verify against the relevant code, RFC,
  devlog, register, current limitations, or external primary source.
- Multi-agent work, chapter briefs, source passes, review passes, editor duties,
  or write-set questions: `anemone-book/meta/agent-orchestration.md`

If the task touches Typst-specific syntax, templates, package behavior, or PDF
layout, also use the `typst` skill.

If the task involves draw.io source files, exported figures, or architecture
diagrams, also use the `drawio-skill`.

## Core Boundaries

- Treat `anemone-book` as a design narrative snapshot, not Anemone's canonical
  source of truth.
- Keep canonical facts in code, RFCs, devlogs, transaction devlogs, register
  entries, current limitations, and primary external sources.
- Do not cite private working paths such as `etc/` as stable public sources.
- Do not turn the book into a syscall manual, source tour, RFC compression,
  test-score report, LTP matrix, or implementation plan.
- Do not add parallel progress files such as `tracking.md`, `todo.md`,
  `devlog.md`, or `rfc.md` under `anemone-book`.
- Do not silently expand the write set. Report why a meta file, chapter, figure
  asset, or template outside the assigned scope must change before editing it.

## Workflow

1. Classify the task.
   - Meta-only update: align the relevant meta file and keep it as the rule
     source.
   - Chapter or prose work: perform a source pass before drafting facts.
   - Review: lead with blocking factual, boundary, coverage, or style findings.
   - Figure work: keep editable source under `assets/sources/` and exported
     figure output under `assets/figures/`.
   - Template or layout work: keep `main.typ` thin and localize style in
     `template/`.

2. Check the artifact boundary.
   - For facts, identify the source of truth before writing.
   - For open issues and accepted limitations, verify the current register or
     current-limitations entry before describing the boundary.
   - For external quotes or claims, use a stable primary source or rewrite as a
     paraphrase.

3. Keep the prose shape.
   - Write Chinese body prose with necessary English technical terms.
   - Use a thesis paragraph for each main chapter.
   - Prefer design judgments, owner boundaries, invariants, and trade-offs over
     feature lists.
   - In Typst source, keep Chinese natural paragraphs on one line unless the
     break is after punctuation or a clear structural boundary.

4. Validate the changed surface.
   - Meta-only changes: run `git diff --check -- anemone-book`.
   - Typst prose, templates, references, or included figures: run
     `typst compile anemone-book/main.typ anemone-book/build/anemone-book.pdf`.
   - draw.io changes: export the affected stable figure, then run Typst compile.
   - Use `pdftotext`, image export, or PDF inspection only when text structure or
     visual layout needs direct verification.

Do not run QEMU, LTP, or the kernel build for book changes unless the book claim
requires fresh implementation evidence or the user asks for it.
