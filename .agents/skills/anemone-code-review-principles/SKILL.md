---
name: anemone-code-review-principles
description: Use when reviewing Anemone kernel changes or writing review guidance for agents and developers. Focus on architecture, subsystem boundaries, concurrency, clean code, directness, observability, ABI containment, resource lifetime, safety boundaries, and failure paths rather than syntax or basic compilation.
---

# Anemone Code Review Principles

## Review Assumption

Assume the submitted code already builds and basic syntax is not the main review target. Review for whether the change preserves kernel behavior, architecture, maintainability, and debuggability under realistic failure and concurrency conditions.

Keep the review scoped to the requested files, subsystem, or patch unless evidence shows the risk crosses a boundary. Prefer concrete findings tied to code paths, invariants, logs, tests, or externally visible behavior.

## Issue Levels

Use explicit issue levels to keep reviews bounded by real risk. Always use the current level names below. Older notes may map these to P0/P1/P2/P3 in the same order, but review output should not use the old names.

- **Apollyon**: Causes wrong results, data corruption, security problems, crashes, or severe unrecoverable state. Must fix.
- **Keter**: Does not immediately explode today, but clearly blocks later development, such as misplaced module boundaries, confused state ownership, or a wrong direction for a core abstraction. Must fix.
- **Euclid**: Usually worth fixing, but not mainline-blocking: inelegant design, local coupling, mediocre naming, awkward tests, or refactorable code whose current shape does not break the main path.
- **Safe**: Record only unless it is cheap and local to fix: pure style preference, theoretical purity, abstraction for abstraction's sake, or something that may only matter in a speculative future. Default to not fixing.

When reviewing, stop issue hunting once remaining observations are Safe unless the user explicitly asks for polish or cleanup. Do not promote Safe or Euclid items into blockers to keep a review going.

## Review Priorities

1. **System architecture**
   - Does the change fit the current subsystem boundaries, ownership model, and layering?
   - Are policy decisions kept at the right layer instead of leaking into low-level helpers?
   - Does it preserve shared contracts such as syscall semantics, VFS rules, memory management invariants, scheduler expectations, and platform abstractions?
   - Do subsystems communicate through explicit interfaces instead of reaching into another subsystem's private types, locks, storage, or internal state?
   - Does a cross-subsystem dependency preserve direction and ownership, or does it make one subsystem implement another subsystem's policy?
   - Is the abstraction justified by real reuse or complexity reduction, or is a direct local implementation clearer?

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
   - Keep subsystem internals local. If code needs another subsystem's details, prefer adding a narrow API at the owner boundary over importing its private representation.
   - Comments should explain non-obvious invariants, ordering constraints, ABI choices, and missing features; they should not restate obvious code.

8. **Directness**
   - Favor the shortest path that preserves the subsystem contract.
   - Avoid speculative generalization, new framework layers, or repo-wide cleanup inside a narrow fix.
   - A small compatibility fix with precise comments is often better than a broad refactor unless the existing structure is the cause of the bug.

9. **Observability and diagnosis**
   - Ask whether a future failure can be diagnosed from logs, traces, counters, assertions, panic messages, or test artifacts.
   - Add or request observability at subsystem boundaries and rare failure paths, not noisy logs in hot paths.
   - Preserve enough context in errors and debug output to connect symptoms to syscall arguments, object identities, state transitions, and errno or signal results.

10. **Tests and evidence**
    - Match verification to risk: targeted tests for narrow behavior, broader regression runs for shared contracts.
    - Prefer tests that encode externally visible semantics, race-prone paths, and failure cases instead of only the happy path.
    - When a finding is uncertain, state the missing evidence and suggest the smallest log, test, or inspection that would confirm it.

## Review Output

Lead with findings, ordered by severity, and label each finding as Apollyon, Keter, Euclid, or Safe. Each finding should include the affected file or code path, the violated invariant or user-visible behavior, why it matters, and a concrete fix direction when possible.

Separate confirmed issues from questions, assumptions, and optional cleanup. Do not block a review on style preferences unless the style issue hides a real correctness, maintainability, or diagnostic risk.

When no blocking issue is found, say so explicitly and note any residual risk or test gap.
