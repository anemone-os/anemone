---
name: anemone-rfc-doc-workflow
description: Use when creating, reviewing, aligning, promoting, or closing Anemone medium/large feature plans, private draft plans, public RFC directories, tracking issues, RFC background docs, or transaction devlogs. Enforces doc-layer closure before implementation, RFC template alignment, tracking issue semantics, and navigation/devlog linkage.
---

# Anemone RFC Doc Workflow

## Canonical Inputs

Read these first for any non-trivial RFC workflow task:

- `docs/src/rfc-workflow.md`
- `docs/src/rfc-template.md`
- `docs/src/rfcs.md`

If the task involves review findings or issue severity, also use `anemone-code-review-principles`.

## Core Boundaries

- Private drafts are working material, not public canonical sources. Use a user-provided private draft path if one is given, but do not publish that path as a stable link in docs, devlog, or register.
- Public RFCs live under `docs/src/rfcs/<short-slug>/` and become canonical immediately after promotion.
- Large feature work should close the document protocol before code implementation unless the user explicitly narrows the task differently.
- RFCs record accepted contract, scope, invariants, and planned gates. Transaction devlogs record execution facts, checkpoints, review results, validation evidence, corrections, and handoff.

## Workflow

1. Identify scope and artifacts.
   - Prefer paths named by the user.
   - Read current RFC docs, transaction devlog, register/current limitations, and relevant backgrounds before judging correctness.
   - For gitignored private drafts, use direct file reads and `git check-ignore -v`; do not rely on `git status`.

2. Align draft shape.
   - Ensure `index.md` and `implementation.md` exist for directory-level RFCs.
   - Add `invariants.md` only when protocol, identity, lock order, lifecycle, or proof obligations are non-trivial.
   - Add `tracking-issues.md` only when confirmed design-review issues affect implementation order, review gate, stop boundary, or acceptance.
   - Use `backgrounds/` for old plans, rejected alternatives, historic issue lists, and evidence indexes.

3. Review at the document layer.
   - Check system invariants, subsystem ownership, ABI boundaries, concurrency, failure paths, observability, and staged validation.
   - Lead findings with `Apollyon`, `Keter`, `Euclid`, `Safe`, or `Neutralized`.
   - Stop issue hunting once remaining observations are only `Safe` unless the user asks for polish.

4. Repair confirmed issues.
   - Fold fixes into canonical RFC text: `index.md`, `invariants.md`, or `implementation.md`.
   - Keep `tracking-issues.md` as status and evidence only; do not use it as the only repair location.
   - When closing issues, preserve the reason, repair location, and transaction/devlog link when one exists.

5. Promote a draft to public RFC.
   - Create or update `docs/src/rfcs/<short-slug>/`.
   - Rewrite entry fields, headings, links, and acceptance boundary so the public RFC is the authority.
   - Update `docs/src/rfcs.md` and `docs/src/SUMMARY.md`.
   - Create or update `backgrounds/index.md` when background material exists.
   - Remove wording that implies a private draft remains canonical.

6. Start implementation tracking.
   - When implementation begins, create `docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md`.
   - Link RFC `index.md` to the transaction and transaction `Canonical Plan` back to the RFC.
   - Update `docs/src/devlog/transactions/index.md`, the current biweekly devlog, and `docs/src/SUMMARY.md`.
   - Transaction entries should be append-only. Add correction notes instead of silently rewriting completed stages.

7. Close the workflow.
   - Update RFC status, transaction status, tracking issues, current limitations/register, and final validation notes.
   - Distinguish agent-run validation, user-run validation, unrun validation, accepted limitations, and follow-up work.

## Validation

For docs-only changes, run at least:

```sh
git diff --check
```

If navigation or mdBook pages changed and `mdbook` is available, also run:

```sh
mdbook build docs
```

Do not run QEMU, LTP, or broad build gates for doc-workflow changes unless the user asks.
