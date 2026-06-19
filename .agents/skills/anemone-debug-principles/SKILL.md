---
name: anemone-debug-principles
description: Use when debugging Anemone kernel, LTP, QEMU, runner, rootfs, or architecture-specific failures, especially large or mixed failure sets where premature root-cause grouping, whole-log analysis, case hacking, or raw-log documentation would mislead the work. Guides progressive failure indexing, small-batch root-cause iteration, failure-set diffing, reclustering, user-private debug archives, and correct write-back into devlog, transaction logs, register, or current limitations.
---

# Anemone Debug Principles

## Overview

Use a progressive debug loop: build a shallow failure index first, debug a small representative batch, validate by failure-set diff, then re-cluster. Treat early groupings as tentative signatures, not root-cause truth.

This skill governs debug process and evidence handling. Use `anemone-rfc-doc-workflow` when a stable conclusion needs public documentation, and use `anemone-build-system` before running build, rootfs, QEMU, or user-test commands.

## Scope Scale

- **Narrow failure:** one testcase, one panic, or one obvious errno path. Do not create a full failure index unless the evidence starts spreading.
- **Batch failure:** several cases or one shared signature. Create a lightweight index and debug one to three representative cases.
- **Campaign failure:** broad LTP/profile fallout, long logs, mixed architectures, or repeated inconclusive fixes. Create a user-private debug archive and run the full progressive loop.

## Evidence Boundaries

- Keep raw logs, temporary failure indexes, tentative hypotheses, and exploratory notes in a user-private debug directory when they need to survive the chat.
- Do not add a public docs chapter for debug logs or process records. Public docs should receive stable conclusions, validation results, accepted limitations, or still-active defects.
- Do not publish user-private debug paths as canonical public-doc dependencies. If public docs need the evidence, summarize the relevant facts or place a factual evidence packet in the owning RFC or small-change `backgrounds/`.
- Prefer no archive for tiny fixes unless the user asks for one or the evidence would otherwise be lost.

Private archive shape, only when useful:

```text
<user-private-debug-dir>/<YYYY-MM-DD-short-slug>/
  README.md
  failure-index.md      # optional for batch/campaign failures
  logs/                 # raw or trimmed logs
```

Keep `README.md` short: status, input logs, current signatures, leading hypothesis, falsification condition, next validation, and planned docs write-back.

## Progressive Workflow

1. Collect input evidence.
   - Prefer paths named by the user.
   - Check user-test logs, debug logs, panic/assert sites, timeout messages, return codes, and testcase output.
   - For LTP questions, read the locally available testcase source and helper code before inferring from the testcase name.
   - When logs contain multiple runs or repeated case IDs, slice by case/run boundaries before drawing conclusions.

2. Build a shallow failure index.
   - Classify by observable signature: panic/assert location, errno, failing syscall, module, timeout, log keyword, architecture, call path, or runner/rootfs symptom.
   - Mark each group as a tentative failure signature, not a proved root-cause family.
   - Avoid full root-cause analysis for every failure in the first pass.

3. Pick a small representative batch.
   - Choose one to three cases from the same or adjacent signature.
   - Prefer cases with clear logs, high scoring impact, minimal environment noise, or strong subsystem overlap.
   - State why these cases are representative and what result would merge, split, or invalidate the current grouping.

4. Debug toward an invariant, not a testcase.
   - Establish the expected Linux/POSIX/LTP behavior from source or reliable local references.
   - Form a root-cause hypothesis with a falsification condition.
   - Identify the violated invariant, ABI rule, state ownership rule, lock/lifetime rule, or runner/rootfs contract.
   - Propose the smallest fix that repairs that rule. Do not aim merely to make one case pass.
   - If evidence is still indirect, ask for the smallest extra log, trace, or targeted rerun instead of overstating certainty.

5. Validate and record the failure-set diff.
   - Rerun the representative cases and the smallest relevant regression set.
   - Record what disappeared, what remained, and what newly failed.
   - If several different-looking failures disappear together, merge the failure family backward.
   - If only one case passes and the fix does not explain adjacent cases, treat it as possible case hacking and re-check the invariant.

6. Re-cluster periodically.
   - After several fixes or a material failure-count drop, rescan remaining failures and update the grouping.
   - When local fixes stop paying off, do a broader face analysis over the smaller remaining set.
   - Keep old classifications as history only if they explain a correction; do not let stale groups steer new work.

## Public Write-Back Rules

- Simple obvious fix: a short biweekly devlog entry, or no formal docs if it is self-evident and not useful later.
- Small semantic fix, compatibility triage, or reusable investigation: `docs/src/devlog/changes/YYYY-MM-DD-short-slug.md` plus a biweekly devlog summary.
- RFC implementation debug: append execution facts, checkpoints, validation evidence, corrections, and handoff to the transaction devlog.
- Still-active broken expected behavior: update `docs/src/register/open-issues.md`.
- Accepted stage limitation or intentionally deferred capability: update `docs/src/register/current-limitations.md`.
- Large evidence packets for an RFC or small-change record: use a specifically named `backgrounds/` evidence file, factual only, when the main record would become hard to scan.

Do not copy raw logs into public docs by default. Summarize the decisive lines, date the evidence, and preserve whether validation was agent-run, user-run, or not run.

## Stop Conditions

- Stop and report when the next correct step would change an accepted RFC invariant, ABI boundary, owner boundary, validation floor, or write set.
- Stop and ask for user-run evidence when reproduction requires privileged images, long QEMU runs, or unavailable logs and the current evidence is insufficient.
- Stop before landing a compatibility bridge that silently weakens external behavior unless the ABI tradeoff, observability, and removal condition are documented in the owning code/doc layer.

## Output Discipline

- Distinguish symptom, failure signature, hypothesis, confirmed root cause, fix, and validation.
- Keep hypotheses tentative until falsification evidence closes them.
- State residual risk and the next smallest validation step.
- Preserve unrelated worktree changes and unrelated failures.
