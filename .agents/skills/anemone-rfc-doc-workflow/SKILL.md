---
name: anemone-rfc-doc-workflow
description: >-
  Use when creating, reviewing, aligning, promoting, or closing Anemone
  documentation workflow artifacts: current contracts, small change records,
  biweekly devlog entries, private draft plans, public RFC directories,
  tracking issues, RFC background docs, transaction devlogs, or
  register/current-limitations links. Enforces artifact boundaries, owner and
  contract-ID discipline, doc-layer closure before implementation, RFC and
  contract template alignment, cutover semantics, and navigation/devlog linkage.
---

# Anemone RFC and Devlog Workflow

## Canonical Inputs

Read the relevant workflow docs before editing public documentation:

- Small changes / devlog: `docs/src/development-log.md`, `docs/src/devlog/changes/index.md`, `docs/src/templates.md`
- RFC / transaction work: `docs/src/rfc-workflow.md`, `docs/src/rfc-template.md`, `docs/src/rfcs.md`
- Current contract work: `docs/src/contracts.md`, `docs/src/contract-template.md`

If the task involves review findings or issue severity, also use `anemone-code-review-principles`.

## Core Boundaries

- Private drafts are working material, not public canonical sources. Use a user-provided private draft path if one is given, but do not publish that path as a stable link in docs, devlog, or register.
- Small change records live under `docs/src/devlog/changes/`. They are self-contained local iteration records for bugfixes, small features, cleanup, and investigations. They may include `Problem`, `Solution`, and an inline `Tracking Issues` section for local review concerns, validation gaps, and closure notes.
- Small change records default to one file: `YYYY-MM-DD-short-slug.md`. Use a same-name directory with `index.md` plus optional `backgrounds/` only when evidence, Linux/LTP references, history, or run logs would make the single file hard to scan. `backgrounds/` remains factual evidence, not a plan layer.
- Small change tracking issues are local to the record. Do not create standalone `tracking-issues.md`, `invariants.md`, or `implementation.md` under `changes/`; if the work needs repository-level accepted targets/contracts, non-trivial invariants, staged gates, multi-agent/checkpoint orchestration, or repeated document-layer review, upgrade to the RFC workflow.
- Biweekly devlogs are timeline summaries and query entry points. Keep enough summary, area, validation status, and links for scanning; put longer small-change facts in `changes/` and long-running execution facts in `transactions/`.
- Public RFCs live under `docs/src/rfcs/<short-slug>/` and become the canonical proposal/target source immediately after promotion; they do not overwrite effective contracts before cutover.
- Large feature work should close the document protocol before code implementation unless the user explicitly narrows the task differently. Closing the protocol means the accepted target, contract delta, correctness boundaries, final proof obligations, feedback routes, and first Ready stage are explicit; later stages may remain Outline and do not need speculative implementation detail.
- Current contracts record effective cross-RFC / cross-module shared rules. RFCs record accepted targets, contract deltas, RFC-local proof obligations, scope, and planned gates. Transaction devlogs record execution facts, contract cutovers, checkpoints, review results, validation evidence, corrections, and handoff.
- The repository Git history owns RFC text versions; do not create per-RFC repositories, versioned canonical file copies, or default amendment documents. RFC `R0`, `R1`, ... identifies accepted target revisions, not every text edit. Bump it only for accepted goal, target invariant, contract delta, owner, ABI / visible-semantics, or acceptance-boundary changes.
- Keep RFC `index.md` and `invariants.md` as the in-place accepted target, `Contract Impact`, and RFC-local proof boundary. They do not own the cross-RFC current contract. Preserve completed-stage and issue history incrementally in `implementation.md` and `tracking-issues.md`, with explicit supersession or neutralization instead of leaving contradictory target rules.
- Do not batch-migrate existing RFC invariants. On the first cross-RFC reuse, extension, or replacement of an existing shared rule, extract only the minimum contract closure: affected rules, unique owner, and required direct dependencies. The old RFC body remains historical and does not require backlink edits.
- Organize contract docs by stable owner and contract surface, not source-file layout. Aggregate small shared rules as stable-ID entries; do not create one file per invariant or generic `misc` / `small-invariants` containers.
- Cross-domain dependencies reference stable IDs. A true cross-domain handoff contract must name one protocol owner, one owner for each state, participant-local obligations, handoff/linearization points, failure, cancellation, and cleanup. Treat shared mutable truth or ownerless cleanup as a blocker.
- Draft and accepted-but-not-effective RFC targets must not overwrite effective contracts. Add an optional pending-successor link after acceptance; update effective IDs only at a documented docs-only or implementation cutover gate with validation evidence in the transaction.
- RFC status describes the current revision: move an accepted revision that still needs implementation to `Accepted for Implementation`, and return it to `Closed` only after that revision closes. When a Closed RFC semantic revision needs code work, create a new transaction linked to that revision; do not reopen or indefinitely append to the previous Completed transaction. Use a follow-up RFC when the core goal, primary owner, overall solution, or most proof boundaries have changed.
- Probe / vertical-slice gates are allowed for high-risk design points when the RFC states the hypothesis, minimum write set, validation floor, failure signal, and RFC/contract write-back path. Probe code must not become a permanent abstraction unless the evidence is recorded, the RFC accepts the target delta, and any long-lived shared rule completes contract cutover.
- Do not create generic `feedback.md`, `probe.md`, or `experiments.md` files by default. Probe plans belong in RFC `implementation.md`; execution feedback belongs in transaction devlog entries. Use `backgrounds/<topic>-probe-YYYYMMDD.md` only for large evidence packets, and keep it factual rather than a plan or status layer.
- Feedback must not unilaterally rewrite the destination, but concrete engineering evidence may trigger a `Target Renegotiation Gate`. Stop before cutover, record cost evidence and the partial-code disposition, then let RFC review choose Route Correction, Accepted Reduced Target, Follow-up RFC, or Not Cut Over. Do not weaken correctness invariants, shrink targets, lower validation conclusions, hide failures, rename in-target defects as limitations, or activate weaker semantics before the new RFC target / `Contract Impact` is accepted and its cutover completes.
- When a feature spans multiple RFCs, use a lightweight navigation entry in an umbrella RFC or `docs/src/rfcs.md`; do not create a parallel feature-progress ledger or copy transaction-devlog facts into another status file.
- Structural module splitting is allowed when it preserves behavior inside the same owner boundary and prevents more responsibility from accumulating in an already-mixed file. If the split changes owner surfaces, public APIs, shared contracts, or the current frozen manifest, require the normal expansion report and record it before implementation continues.
- If a small change starts needing repository-level accepted targets/current contracts, non-trivial invariants, staged implementation gates, standalone `tracking-issues.md`, or multi-agent/checkpoint orchestration, upgrade to the RFC workflow instead of expanding `changes/` into a small RFC.
- Write sets are coordination contracts, not architecture constraints. A worker must not silently edit outside the current frozen manifest, but if the better design needs a different owning surface, it should stop and report the proposed expansion instead of forcing compatibility inside the old manifest.
- Multi-stage RFCs use rolling stage resolution by default. Only the next executable stage must be fully resolved as Ready; later stages stay Outline with purpose, dependencies, protected target/contract boundaries, and a resolution trigger. Missing types, functions, algorithms, exact paths, corner-case matrices, or test commands in a future Outline is intentional, not a finding.
- Close Stage N independently before resolving Stage N+1. Run a separate read-only `N -> N+1 Implementation Resolution Gate` against live source, the completed diff, review findings, module boundaries, validation evidence, RFC target, and current contracts. Resolve the full next stage, including deliverables, route/probe, audit, observability, validation, stop/exit conditions, cutover, and its `Resolved Write Set Manifest`.
- Ready means the whole next stage is resolved but not automatically authorized. Keep the authoritative Ready-stage definition and manifest in `implementation.md`; transaction devlogs record preflight evidence, approval, activation point, and a link, not a second plan authority.
- Only edits outside a Ready / Active stage's frozen manifest are write-set expansions. Changes to future Outlines are normal resolution while accepted semantics stay unchanged; target, protocol/state owner, public API, shared-contract, ABI, visible-semantics, or acceptance-boundary changes require RFC review or target renegotiation.
- Do not batch-rewrite completed stages or historical transaction manifests. Apply rolling stage resolution to the next unresolved stage of an active RFC; retain existing transaction copies as historical execution facts.
- `tracking-issues.md` remains for design issues, not progress logs. A design issue may come from document-layer review or from implementation feedback that exposes a wrong invariant, owner boundary, ABI choice, stage order, or acceptance condition.

## Workflow

1. Identify scope and artifacts.
   - Prefer paths named by the user.
   - Classify the work first:
     - Simple bugfix or tiny cleanup: direct biweekly devlog entry, or no formal doc if the change is self-evident and not worth long-term lookup.
     - Small semantic fix, compatibility triage, small feature, or reusable investigation: small change record plus a short biweekly devlog entry.
     - Medium/large plan, cross-subsystem target, invariants, staged review, or implementation gates: RFC workflow.
     - Effective rule reused or changed across RFCs/modules: current contract plus RFC `Contract Impact` and a cutover gate.
     - RFC implementation: transaction devlog.
     - Still-active defect or accepted limitation: register / current limitations, linked back to devlog, change record, RFC, or transaction.
   - Read current contracts, current RFC docs, transaction devlog, register/current limitations, and relevant backgrounds before judging correctness.
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
   - Add `invariants.md` when the RFC changes a shared contract or when protocol, identity, lock order, lifecycle, or proof obligations are non-trivial.
   - When contracts are affected, require `Contract Impact` with stable IDs, Introduce / Preserve / Refine / Replace / Remove / Scoped Exception classification, current links where an effective rule exists, target summaries, and cutover gates.
   - Use `Introduce` only for a new stable ID with no effective rule. If behavior already exists but has not been extracted into `docs/src/contracts/`, extract the minimum current baseline first and classify the actual semantic delta instead of calling the documentation migration `Introduce`.
   - Keep unchanged effective rules in current contracts and link them; put proposed target rules and RFC-local migration/proof obligations in the RFC.
   - For multi-stage plans, require a complete Ready definition only for the first executable stage. Give later Outline stages a purpose, prerequisites, protected invariants/contract IDs/hard boundaries, a resolution trigger, and optional scope estimates; do not require speculative implementation details or exact validation commands.
   - Add `tracking-issues.md` only when confirmed design-review issues affect implementation order, review gate, stop boundary, or acceptance.
   - Use `backgrounds/` for old plans, rejected alternatives, historic issue lists, and evidence indexes.

4. Review at the document layer.
   - Check system invariants, subsystem ownership, ABI boundaries, concurrency, failure paths, observability, and staged validation.
   - For contract changes, verify minimum-closure coverage, unique owners, effective-vs-target separation, cross-domain obligations, and an explicit cutover gate.
   - Lead findings with `Apollyon`, `Keter`, `Euclid`, `Safe`, or `Neutralized`.
   - Read stage maturity before judging precision. Do not emit a finding merely because a future Outline omits types, functions, algorithms, per-file paths, a complete corner-case matrix, or exact test commands. Require that precision only from the next Ready stage.
   - Treat an Outline as defective only when its missing dependency/protected boundary makes the target path incoherent, or when it defers a possible owner/ABI/contract/acceptance change without a resolution or renegotiation gate.
   - Stop issue hunting once remaining observations are only `Safe` unless the user asks for polish.

5. Repair confirmed issues.
   - Fold target fixes into RFC `index.md`, `invariants.md`, or `implementation.md`; update `Contract Impact` when effective shared rules are affected.
   - Do not update effective contract text until the approved cutover gate; record the changed IDs and evidence in the transaction when cutover occurs.
   - Keep `tracking-issues.md` as status and evidence only; do not use it as the only repair location.
   - When closing issues, preserve the reason, repair location, and transaction/devlog link when one exists.
   - If the accepted repair changes RFC semantics, increment the RFC revision and add a concise revision-record row. Preserve the current revision for wording, evidence, or implementation-plan-only changes.

6. Promote a draft to public RFC.
   - Create or update `docs/src/rfcs/<short-slug>/`.
   - Rewrite entry fields, headings, links, and acceptance boundary so the public RFC is the proposal/target authority without claiming pre-cutover effective semantics.
   - Update `docs/src/rfcs.md` and `docs/src/SUMMARY.md`.
   - Link affected current contract IDs and, after acceptance, add pending-successor navigation where useful.
   - Create or update `backgrounds/index.md` when background material exists.
   - Remove wording that implies a private draft remains canonical.

7. Start implementation tracking.
   - When implementation begins, create `docs/src/devlog/transactions/YYYY-MM-DD-<short-slug>.md`.
   - Record the target RFC revision in the transaction. For post-close semantic revisions, create a new transaction instead of appending to the completed original transaction.
   - Record affected contract IDs, delta classifications, and planned cutover gates; use `None` when no current contract changes.
   - Link RFC `index.md` to the transaction and transaction `Canonical Plan` back to the RFC.
   - Update `docs/src/devlog/transactions/index.md`, the current biweekly devlog, and `docs/src/SUMMARY.md`.
   - Transaction entries should be append-only. Add correction notes instead of silently rewriting completed stages.
   - Before the first executable stage, resolve its full Ready definition and exact manifest in `implementation.md`. For later stages, close the current stage first, then run a separate read-only Implementation Resolution Gate and resolve only the next stage; do not make next-stage resolution part of the current stage's closure.
   - The transition preflight must inspect live owners, the completed stage diff, review findings, module-boundary pressure, validation-only inputs, documentation write-back surfaces, RFC target, and current contracts. Record evidence and approval in the transaction, but link to the authoritative Ready stage instead of duplicating it.
   - A fully resolved next stage is Ready, not Active. Preserve any explicit user/orchestrator authorization gate and never auto-enter the stage.
   - If the next stage is high risk, add or verify a probe / vertical-slice gate in `implementation.md` before feature code: hypothesis, protected goal/invariant, minimum write set, non-goals, validation floor, failure signal, write-back target, and exit path.
   - Route implementation feedback by impact:
     - execution facts, checkpoints, review results, and validation evidence stay in the transaction devlog;
     - future Outline/Ready resolution, stage order, write set, validation arrangement, review gate, or stop-condition changes that preserve target update `implementation.md` plus the transaction devlog;
     - target invariant, owner-boundary, ABI, visible-semantics, or acceptance-boundary changes stop before cutover and enter RFC review / Target Renegotiation Gate; accepted changes update RFC `index.md` / `invariants.md`, `Contract Impact`, and the relevant tracking issue;
     - effective contract changes occur only at the approved cutover gate and update the contract plus transaction evidence atomically;
     - accepted gaps go to current limitations, while broken expected behavior goes to open issues.
   - When engineering cost triggers target renegotiation, require concrete cost evidence, original-target impact, correctness invariants, completed-slice/code disposition, comparable options, candidate reduced-target semantics, revision/contract impact, validation, authority, and remaining-gap routing. An agent may propose but never approve its own reduced target.
   - For large or fast-growing modules, add a module-boundary preflight or split-only checkpoint before feature code if continuing in the same file would reinforce a wrong owner boundary.
   - If a worker needs to exceed the current frozen manifest for architectural reasons, require an upward report with the reason, proposed files/modules, affected contract, and validation gate. After approval, update the authoritative manifest in `implementation.md`, then record the approval and link in the transaction before continuing. Do not classify changes to a future Outline or its scope estimate as expansion.

8. Close the workflow.
   - Update RFC status, transaction status, each affected contract ID's effective/pending/not-cut-over result, tracking issues, current limitations/register, and final validation notes.
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
