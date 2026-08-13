---
name: anemone-development-workflow
description: >-
  Use when classifying, planning, documenting, reviewing, promoting, or closing
  Anemone development work across patches, small change records, RFCs, current
  contracts, optional transaction devlogs, register entries, and workflow
  templates. Enforces minimal artifact selection, Implementation Boundary,
  cutover honesty, feedback routing, and architecture-friction closeout.
---

# Anemone Development Workflow

Use this skill to route development work to the smallest sufficient artifact.
Treat `docs/src/development-workflow.md` as the canonical policy; use this body
as a router and never let it override that policy.

## Canonical Inputs

Read the pages relevant to the task before editing:

- Development classification and lifecycle: `docs/src/development-workflow.md`
- Small changes and optional timeline logs: `docs/src/development-log.md`,
  `docs/src/devlog/changes/index.md`, `docs/src/templates.md`
- RFC shape and navigation: `docs/src/rfc-template.md`, `docs/src/rfcs.md`
- Effective shared rules: `docs/src/contracts.md`,
  `docs/src/contract-template.md`
- Current defects and accepted gaps: `docs/src/register.md`,
  `docs/src/register/open-issues.md`,
  `docs/src/register/current-limitations.md`

Also read `LOCAL.md` when present and the relevant live source, current
contract IDs, active RFC/change record, and existing execution evidence. Use
`anemone-code-review-principles` when judging findings or architecture friction.

Treat `docs/src/rfc-workflow.md` as a compatibility path, not the canonical
input.

## Route the Work

1. Classify the change before choosing documents.
   - Patch: behavior is already determined; default to code, tests, validation,
     and Git/PR evidence only.
   - Small iteration: a local decision, non-obvious root cause, compatibility
     choice, reusable investigation, or one final local cutover deserves one
     self-contained change record. It may use at most two execution checkpoints
     inside one fully resolved Implementation Boundary.
   - RFC: owner, handoff, failure, cleanup, ABI, shared contract, non-trivial
     correctness proof, probe, multiple cutovers, or target renegotiation is not
     locally closed.
2. Create only the required artifacts.
   - RFCs default to `index.md`; add `invariants.md`, `implementation.md`,
     `tracking-issues.md`, `backgrounds/`, or a transaction only for a named
     need in the canonical workflow.
   - Biweekly devlogs and transactions are optional evidence/navigation layers,
     not activation gates.
   - A checkpointed small iteration keeps both checkpoints inline in the same
     change record; it does not create an RFC `implementation.md` or transaction.
   - Update register/current limitations only for a genuinely current defect or
     accepted gap.
3. State the Implementation Boundary at the semantic level. File and directory
   hints are non-exhaustive unless the user explicitly imposes a strict list.
4. Preserve effective-vs-target separation. `Contract Impact` lists only actual
   semantic changes; unchanged effective rules belong under Dependencies.
5. Close only the authority surfaces that really changed. Do not duplicate the
   same target, status, validation matrix, or execution history across pages.

## Implementation and Feedback

Within the Implementation Boundary, allow imports/re-exports, module
registration, same-owner files, targeted tests, and behavior-preserving
same-owner splits to follow the natural implementation. Stop before completion
or cutover when target, owner, handoff, failure, cleanup, public API, ABI,
visibility/shared contract, acceptance, or validation claims must change.

A small iteration defaults to one closure checkpoint. Use at most two execution
checkpoints only for a real review, commit, or authorization stop. CKPT 1 must
be independently safe and neutral to protected visible semantics and the
current contract; CKPT 2 closes the same target and owns at most one final
semantic or contract cutover. Both checkpoints share one target, owner and
handoff model, failure/cleanup rules, ABI/contract delta, acceptance, validation
claim, and change record. Ordinary commits do not count toward this limit.
Escalate to an RFC for a probe, transitional contract, boundary re-resolution,
multiple independent cutovers, target renegotiation, or more than two formal
execution checkpoints.

If a user authorizes only one checkpoint or stage, stop after it. Do not infer
authorization for the next gate from an existing plan.

Before RFC closure, keep route corrections that preserve the accepted target in
the implementation plan when one exists. Send target/owner/ABI/contract/
acceptance changes through RFC review or Target Renegotiation. An agent may
propose a reduced target but cannot approve it. After closure, never reopen or
revise the RFC; classify related work from live source, current contracts, and
the register under a new Implementation Boundary.

Probe code needs a hypothesis, protected boundary, failure signal, validation,
write-back, and exit condition. It does not become a permanent abstraction just
because it runs.

## Architecture Friction Closeout

Before closing any implementation unit, apply the Architecture Friction Scan in
the canonical workflow:

- no concrete friction or only Safe: emit no placeholder report;
- residual Euclid: report evidence, model mismatch, impact, and the smallest
  repair direction;
- Keter/Apollyon: stop before completion/cutover and report code disposition and
  the required owner/RFC/target decision.

Do not create `friction.md` or a global friction ledger.

## Documentation Maintenance

- Private drafts are not public canonical sources.
- Positioning or backgrounds are optional evidence, not prerequisites for an
  RFC; start directly with `index.md` when the target is already resolved.
- Git owns RFC text history; do not create per-RFC repositories, versioned
  canonical copies, or default amendment files.
- Treat Closed as an irreversible terminal state. Freeze the RFC as historical
  target/provenance; do not add revisions, gates, or continuation transactions.
  Future-work routing written in a Closed RFC is historical, not authority for
  classifying or authorizing a new task.
- Keep historical RFCs, completed transactions, manifests, and old terminology
  as history. Apply the current workflow to new tasks and the next unstarted
  gate of active work.
- Update `docs/src/SUMMARY.md` only when public navigation changes.
- For gitignored private material, read paths directly and use
  `git check-ignore -v`; do not rely on `git status`.

## Validation

For documentation-only changes, run at least:

```sh
git diff --check
```

When mdBook navigation or pages change and `mdbook` is available, also run:

```sh
mdbook build docs
```

Do not run kernel, QEMU, or LTP gates solely for workflow documentation changes.
