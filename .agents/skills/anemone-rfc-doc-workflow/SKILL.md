---
name: anemone-rfc-doc-workflow
description: >-
  Use when creating, reviewing, aligning, promoting, or closing Anemone
  documentation workflow artifacts: small change records, biweekly devlog
  entries, private draft plans, public RFC directories, tracking issues, RFC
  background docs, transaction devlogs, or register/current-limitations links.
  Enforces artifact boundaries, doc-layer closure before implementation, RFC
  template alignment, tracking issue semantics, and navigation/devlog linkage.
---

# Anemone RFC and Devlog Workflow

## Canonical Inputs

Read the relevant workflow docs before editing public documentation:

- Small changes / devlog: `docs/src/development-log.md`, `docs/src/devlog/changes/index.md`, `docs/src/templates.md`
- RFC / transaction work: `docs/src/rfc-workflow.md`, `docs/src/rfc-template.md`, `docs/src/rfcs.md`

If the task involves review findings or issue severity, also use `anemone-code-review-principles`.

## Core Boundaries

- Private drafts are working material, not public canonical sources. Use a user-provided private draft path if one is given, but do not publish that path as a stable link in docs, devlog, or register.
- Small change records live under `docs/src/devlog/changes/`. They are self-contained local iteration records for bugfixes, small features, cleanup, and investigations. They may include `Problem`, `Solution`, and an inline `Tracking Issues` section for local review concerns, validation gaps, and closure notes.
- Small change records default to one file: `YYYY-MM-DD-short-slug.md`. Use a same-name directory with `index.md` plus optional `backgrounds/` only when evidence, Linux/LTP references, history, or run logs would make the single file hard to scan. `backgrounds/` remains factual evidence, not a plan layer.
- Small change tracking issues are local to the record. Do not create standalone `tracking-issues.md`, `invariants.md`, or `implementation.md` under `changes/`; if the work needs repository-level accepted contracts, non-trivial invariants, staged gates, multi-agent/checkpoint orchestration, or repeated document-layer review, upgrade to the RFC workflow.
- Biweekly devlogs are timeline summaries and query entry points. Keep enough summary, area, validation status, and links for scanning; put longer small-change facts in `changes/` and long-running execution facts in `transactions/`.
- Public RFCs live under `docs/src/rfcs/<short-slug>/` and become canonical immediately after promotion.
- Large feature work should close the document protocol before code implementation unless the user explicitly narrows the task differently. Closing the protocol means the accepted contract, validation floor, stop conditions, and feedback routes are explicit; it does not mean every uncertainty has been eliminated before code starts.
- RFCs record accepted contract, scope, invariants, and planned gates. Transaction devlogs record execution facts, checkpoints, review results, validation evidence, corrections, and handoff.
- Probe / vertical-slice gates are allowed for high-risk design points when the RFC states the hypothesis, minimum write set, validation floor, failure signal, and RFC write-back path. Probe code must not become a permanent abstraction unless the evidence is recorded and the RFC accepts the resulting contract.
- Do not create generic `feedback.md`, `probe.md`, or `experiments.md` files by default. Probe plans belong in RFC `implementation.md`; execution feedback belongs in transaction devlog entries. Use `backgrounds/<topic>-probe-YYYYMMDD.md` only for large evidence packets, and keep it factual rather than a plan or status layer.
- Feedback can optimize the route, not rewrite the destination. If implementation shows a goal, invariant, ABI boundary, or acceptance condition is wrong, stop the current gate and return to RFC review. Do not weaken invariants, shrink goals, lower validation floors, hide failures, rename blockers as limitations, or land hacks just to pass a gate before canonical RFC text is updated.
- When a feature spans multiple RFCs, use a lightweight navigation entry in an umbrella RFC or `docs/src/rfcs.md`; do not create a parallel feature-progress ledger or copy transaction-devlog facts into another status file.
- Structural module splitting is allowed when it preserves behavior inside the same owner boundary and prevents more responsibility from accumulating in an already-mixed file. If the split changes owner surfaces, public APIs, shared contracts, or write sets, require the normal expansion report and record it before implementation continues.
- If a small change starts needing repository-level accepted contract text, non-trivial invariants, staged implementation gates, standalone `tracking-issues.md`, or multi-agent/checkpoint orchestration, upgrade to the RFC workflow instead of expanding `changes/` into a small RFC.
- Write sets are coordination contracts, not architecture constraints. A worker must not silently edit outside its assigned write set, but if the better design needs a different owning surface, it should stop and report the proposed expansion instead of forcing compatibility inside the old write set.
- `tracking-issues.md` remains for design issues, not progress logs. A design issue may come from document-layer review or from implementation feedback that exposes a wrong invariant, owner boundary, ABI choice, stage order, or acceptance condition.

## Workflow

1. Identify scope and artifacts.
   - Prefer paths named by the user.
   - Classify the work first:
     - Simple bugfix or tiny cleanup: direct biweekly devlog entry, or no formal doc if the change is self-evident and not worth long-term lookup.
     - Small semantic fix, compatibility triage, small feature, or reusable investigation: small change record plus a short biweekly devlog entry.
     - Medium/large plan, cross-subsystem contract, invariants, staged review, or implementation gates: RFC workflow.
     - RFC implementation: transaction devlog.
     - Still-active defect or accepted limitation: register / current limitations, linked back to devlog, change record, RFC, or transaction.
   - Read current RFC docs, transaction devlog, register/current limitations, and relevant backgrounds before judging correctness.
   - For gitignored private drafts, use direct file reads and `git check-ignore -v`; do not rely on `git status`.

2. Handle small change records.
   - Use the template in `docs/src/templates.md`.
   - Keep the record self-contained: problem, scope, solution, change, validation, local tracking issues, risk/follow-up, and links.
   - Default to `docs/src/devlog/changes/YYYY-MM-DD-short-slug.md`.
   - Use `docs/src/devlog/changes/YYYY-MM-DD-short-slug/index.md` plus `backgrounds/` only for evidence packets or reusable references.
   - Update the current biweekly devlog with a short summary and link to the small change record.
   - Update `docs/src/devlog/changes/index.md` when adding a new record so it remains discoverable.
   - Use an inline `Tracking Issues` section for local review concerns, validation gaps, and closure notes. When closing an item, fold the conclusion back into the record body instead of leaving the tracker as the only repair location.
   - Do not add `invariants.md`, `implementation.md`, or standalone `tracking-issues.md` under `changes/`.

3. Align RFC draft shape.
   - Ensure `index.md` and `implementation.md` exist for directory-level RFCs.
   - Add `invariants.md` only when protocol, identity, lock order, lifecycle, or proof obligations are non-trivial.
   - Add `tracking-issues.md` only when confirmed design-review issues affect implementation order, review gate, stop boundary, or acceptance.
   - Use `backgrounds/` for old plans, rejected alternatives, historic issue lists, and evidence indexes.

4. Review at the document layer.
   - Check system invariants, subsystem ownership, ABI boundaries, concurrency, failure paths, observability, and staged validation.
   - Lead findings with `Apollyon`, `Keter`, `Euclid`, `Safe`, or `Neutralized`.
   - Stop issue hunting once remaining observations are only `Safe` unless the user asks for polish.

5. Repair confirmed issues.
   - Fold fixes into canonical RFC text: `index.md`, `invariants.md`, or `implementation.md`.
   - Keep `tracking-issues.md` as status and evidence only; do not use it as the only repair location.
   - When closing issues, preserve the reason, repair location, and transaction/devlog link when one exists.

6. Promote a draft to public RFC.
   - Create or update `docs/src/rfcs/<short-slug>/`.
   - Rewrite entry fields, headings, links, and acceptance boundary so the public RFC is the authority.
   - Update `docs/src/rfcs.md` and `docs/src/SUMMARY.md`.
   - Create or update `backgrounds/index.md` when background material exists.
   - Remove wording that implies a private draft remains canonical.

7. Start implementation tracking.
   - When implementation begins, create `docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md`.
   - Link RFC `index.md` to the transaction and transaction `Canonical Plan` back to the RFC.
   - Update `docs/src/devlog/transactions/index.md`, the current biweekly devlog, and `docs/src/SUMMARY.md`.
   - Transaction entries should be append-only. Add correction notes instead of silently rewriting completed stages.
   - If the next stage is high risk, add or verify a probe / vertical-slice gate in `implementation.md` before feature code: hypothesis, protected goal/invariant, minimum write set, non-goals, validation floor, failure signal, write-back target, and exit path.
   - Route implementation feedback by impact:
     - execution facts, checkpoints, review results, and validation evidence stay in the transaction devlog;
     - stage order, write set, validation floor, review gate, or stop-condition changes update `implementation.md` plus the transaction devlog;
     - invariant, owner-boundary, ABI, visible-semantics, or acceptance-boundary changes update `index.md` / `invariants.md` and the relevant tracking issue;
     - accepted gaps go to current limitations, while broken expected behavior goes to open issues.
   - For large or fast-growing modules, add a module-boundary preflight or split-only checkpoint before feature code if continuing in the same file would reinforce a wrong owner boundary.
   - If a worker needs a larger write set for architectural reasons, require an upward report with the reason, proposed files/modules, affected contract, and validation gate. After approval, record the updated write set in the transaction devlog or orchestration doc before continuing.

8. Close the workflow.
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
