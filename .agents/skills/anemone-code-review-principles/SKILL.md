---
name: anemone-code-review-principles
description: Use when reviewing Anemone kernel changes or writing review guidance for agents and developers. Focus on architecture, subsystem boundaries, concurrency, clean code, directness, observability, ABI containment, resource lifetime, safety boundaries, and failure paths rather than syntax or basic compilation.
---

# Anemone Code Review Principles

## Review Assumption

Assume the submitted code already builds and basic syntax is not the main review target. Review for whether the change preserves kernel behavior, architecture, maintainability, and debuggability under realistic failure and concurrency conditions.

Keep the review scoped to the requested files, subsystem, or patch unless evidence shows the risk crosses a boundary. Prefer concrete findings tied to code paths, invariants, logs, tests, or externally visible behavior.

Also check the repository-level coding rules in `AGENTS.md`, especially the kernel code-shape constraints. Treat violations of single-source-of-truth, diagnostic-field boundaries, narrow interfaces, assertion policy, or temporary-bridge exit conditions as review issues, not as cosmetic style comments.

For an RFC-driven or cross-subsystem change, read `docs/src/contracts.md`, the relevant effective contract IDs, the RFC `Contract Impact` / target invariants, and the transaction cutover record before treating a rule as current. Existing RFC text may be historical or proposal-local; it must not override an extracted current contract. If no contract has been extracted yet and the change is the first cross-RFC reuse or replacement, require the minimum contract closure rather than a repository-wide invariant inventory.

## Issue Levels

Use explicit issue levels to keep reviews bounded by real risk. Always use the current level names below. Older notes may map these to P0/P1/P2/P3 in the same order, but review output should not use the old names.

- **Apollyon**: Causes wrong results, data corruption, security problems, crashes, or severe unrecoverable state. Must fix.
- **Keter**: Does not immediately explode today, but clearly blocks later development, such as misplaced module boundaries, confused state ownership, or a wrong direction for a core abstraction. Must fix.
- **Euclid**: Usually worth fixing, but not mainline-blocking: inelegant design, local coupling, mediocre naming, awkward tests, or refactorable code whose current shape does not break the main path.
- **Safe**: Record only unless it is cheap and local to fix: pure style preference, theoretical purity, abstraction for abstraction's sake, or something that may only matter in a speculative future. Default to not fixing.

For file/module size findings, classify by responsibility rather than line count. A long table or ABI definition file can be Safe; a file that mixes syscall ABI, core state ownership, backend operations, compatibility bridges, and lifecycle rules is at least Euclid, and becomes Keter when the mixed shape is already causing owner-boundary mistakes, duplicated truth, ABI leakage, or blocked follow-up work.

When reviewing, stop issue hunting once remaining observations are Safe unless the user explicitly asks for polish or cleanup. Do not promote Safe or Euclid items into blockers to keep a review going.

## Review Priorities

1. **System architecture**
   - Does the change fit the current subsystem boundaries, ownership model, and layering?
   - Does it preserve the relevant effective contract IDs, or is every intentional delta named in the RFC and activated only at its approved cutover gate?
   - Are accepted targets still pending clearly separated from current behavior and review evidence?
   - Are policy decisions kept at the right layer instead of leaking into low-level helpers?
   - Does it preserve shared contracts such as syscall semantics, VFS rules, memory management invariants, scheduler expectations, and platform abstractions?
   - Do subsystems communicate through explicit interfaces instead of reaching into another subsystem's private types, locks, storage, or internal state?
   - Does a cross-subsystem dependency preserve direction and ownership, or does it make one subsystem implement another subsystem's policy?
   - For a true cross-domain handoff, is there one protocol owner, one owner for each mutable state, explicit participant-local obligations, a handoff/linearization point, and a final cleanup owner?
   - Is the abstraction justified by real reuse or complexity reduction, or is a direct local implementation clearer?
   - If a file keeps growing, is the growth still one owner doing one job, or is it accumulating multiple roles that should be split by boundary?
   - Would a behavior-preserving split inside the same owner make future changes safer without creating a new abstraction layer?

2. **Concurrency and races**
   - Identify shared mutable state and check which lock, atomic rule, interrupt rule, or ownership transfer protects it.
   - Review lock ordering, lifetime across unlock points, wakeup and cancellation paths, and time-of-check to time-of-use windows.
   - Check whether error paths, early returns, and partial initialization leave waiters, references, mappings, or flags in inconsistent states.
   - Treat "works in the common path" as insufficient for kernel code that can be preempted, interrupted, re-entered, or observed by another thread.

3. **ABI containment and compatibility semantics**
   - For syscalls and user-visible APIs, verify Linux-compatible flag validation, errno mapping, signal behavior, struct layout, alignment, and edge cases.
   - Keep Linux ABI representation at syscall or compatibility boundaries. Translate Linux structs, flags, magic constants, and layouts into internal domain types before entering core subsystems.
   - Do not embed ABI structs or Linux-shaped state in kernel objects unless the object is explicitly a boundary representation. Internal APIs should express Anemone concepts and invariants, not mirror Linux ABI layout.
   - Distinguish unsupported features from invalid input and permission failures; returning the wrong error can break tests and userspace even when the internal state is safe.
   - Check boundary cases: zero sizes, maximum sizes, overflow, unaligned addresses, null pointers, duplicate flags, unknown flags, and mixed valid or invalid modes.

4. **Resource lifetime and cleanup**
   - Track ownership of memory, frames, file descriptors, dentries, inodes, tasks, wait queues, mappings, and device state.
   - Verify every acquire has a matching release on success, failure, cancellation, and teardown paths.
   - Look for leaks, double releases, stale references, use-after-free risks, and state published before initialization is complete.

5. **Safety and trust boundaries**
   - Treat user pointers, device data, filesystem metadata, and external input as untrusted.
   - Check copy-in and copy-out ordering, bounds checks, integer overflow, capability or permission checks, and exposure of kernel-only data.
   - Ensure validation happens before irreversible side effects unless the ABI requires otherwise.

6. **Failure paths and degradation**
   - Review allocation failure, I/O failure, partial copy, interrupted waits, invalid handles, and unsupported platform paths.
   - Confirm failures leave state unchanged or deliberately rolled forward to a documented state.
   - Prefer explicit errors over silent fallback when the caller or tests need to distinguish behavior.

7. **Clean code and local clarity**
   - Prefer small functions with explicit invariants, names that match kernel concepts, and data structures that make illegal states hard to represent.
   - Avoid clever control flow, hidden side effects, broad catch-all helpers, and duplicated compatibility logic.
   - Avoid caching fields that can be derived from the owning object unless the field is a justified performance cache, stable snapshot, or cross-lifetime diagnostic identity with an explicit comment and cheap consistency assertion where possible.
   - Mark diagnostic-only fields such as owner ids, wait ids, token ids, and debug labels at the field declaration. They must not drive behavior unless they are promoted into explicit protocol state.
   - Keep subsystem internals local. If code needs another subsystem's details, prefer adding a narrow API at the owner boundary over importing its private representation.
   - Comments should explain non-obvious invariants, ordering constraints, ABI choices, and missing features; they should not restate obvious code.
   - Treat missing comments on non-obvious invariants, ABI tradeoffs, lock or lifetime ordering, temporary compatibility bridges, accepted limitations, and special cases as maintainability or diagnostic risk, not as pure style. It is Safe only when the behavior is evident from the code and future misuse is unlikely.
   - Do not request narrative filler comments. A useful comment must preserve a decision, invariant, boundary, failure mode, or removal condition that would otherwise be easy to lose during future edits.
   - Do not treat every split as scope creep. When a file already mixes unrelated responsibilities, ask for a split-only checkpoint that preserves public behavior and narrows visibility before more feature logic is added.
   - Prefer directory modules over oversized flat files when a module has stable sub-roles such as ABI conversion, internal state, operations, lifecycle, or tests. Keep re-exports narrow so the split enforces the boundary instead of only moving text around.

8. **Directness**
   - Favor the shortest path that preserves the subsystem contract.
   - Avoid speculative generalization, new framework layers, or repo-wide cleanup inside a narrow fix.
   - A small compatibility fix with precise comments is often better than a broad refactor unless the existing structure is the cause of the bug.
   - If the existing structure is the cause of the risk, make the structural step explicit and bounded: same-owner split, no semantic change, no public API expansion, and validation that call sites still use the intended facade.

9. **Observability and diagnosis**
   - Ask whether a future failure can be diagnosed from logs, traces, counters, assertions, panic messages, or test artifacts.
   - Add or request observability at subsystem boundaries and rare failure paths, not noisy logs in hot paths.
   - Prefer ordinary `assert!` over `debug_assert!` for lightweight invariant checks, so correctness bugs are exposed in normal runs. Reserve `debug_assert!` for checks that are too expensive for release paths, such as broad collection scans.
   - Preserve enough context in errors and debug output to connect symptoms to syscall arguments, object identities, state transitions, and errno or signal results.

10. **Tests and evidence**
    - Match verification to risk: targeted tests for narrow behavior, broader regression runs for shared contracts.
    - For a contract cutover, verify the transaction names every changed ID, records old/new effective semantics and scope, and provides evidence for all participating domains; a passing narrow test cannot prove a broader contract transition.
    - Prefer tests that encode externally visible semantics, race-prone paths, and failure cases instead of only the happy path.
    - When a finding is uncertain, state the missing evidence and suggest the smallest log, test, or inspection that would confirm it.

## Review Output

Lead with findings, ordered by severity, and label each finding as Apollyon, Keter, Euclid, or Safe. Each finding should include the affected file or code path, the violated invariant or user-visible behavior, why it matters, and a concrete fix direction when possible.

Separate confirmed issues from questions, assumptions, and optional cleanup. Do not block a review on style preferences unless the style issue hides a real correctness, maintainability, or diagnostic risk.

When no blocking issue is found, say so explicitly and note any residual risk or test gap.
