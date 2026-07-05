# ANE-CHG-20260705-read-write-request-structure

**Type:** Cleanup / VFS read-write structure
**Status:** Completed
**Date:** 2026-07-05
**Authors:** doruche, Codex
**Area:** VFS / syscall read-write / direct-user I/O orchestration

## Problem

After the first VFS direct-user I/O implementation, `fs/api/read_write` owned
the right protocol boundary: syscall-facing argument normalization, direct-user
dispatch, kernel-buffer fallback, vectored progress aggregation, fanotify
notification, and the `pwritev2(flags != 0)` unsupported gate.

The code still expressed that protocol through a flat helper set in
`read_write/mod.rs`. Single-buffer helpers, vectored helpers, fallback helpers,
and `*_without_notify()` paths jointly maintained the same rules: `N > 0`
partial success, `None`-only direct-user fallback, fanotify aggregate
notification, different read transaction / ordinary / positioned dispatch
order, and fallback progress handling. That was not yet a wrong owner boundary,
but it made future `preadv2`, `RWF_*`, or backend capability work too easy to
scatter across unrelated helpers.

## Scope

This iteration is a behavior-preserving structure cleanup inside
`anemone-kernel/src/fs/api/read_write/`.

In scope:

- add an internal `request/` module;
- introduce short-lived `ReadRequest` and `WriteRequest` execution objects;
- keep syscall wrappers responsible for raw ABI gates, fd/uspace lookup, offset
  decoding, and iovec import order;
- move direct-user dispatch, fallback, vectored aggregate partial handling, and
  final notification decisions behind request `execute()`.

Out of scope:

- no `FileDesc`, `File`, `FileOps`, or `fs/uio.rs` contract change;
- no backend hook additions, removals, or scope expansion;
- no `RWF_*` / per-call flag implementation;
- no fanotify transaction semantic change;
- no change to Linux-visible partial success, `EFAULT`, offset advancement, or
  notification counts.

## Solution

`ReadRequest` and `WriteRequest` model the normalized syscall shape inside the
read-write API owner. They borrow the opened file description, current
userspace handle, and either a single user buffer or imported iovec list. The
request position is a restricted internal state: sequential or positioned.

`execute()` is the only public execution entry inside the module. It owns:

- direct-user hook dispatch and `None` fallback handling;
- kernel-buffer fallback for single-buffer and per-segment vectored paths;
- vectored `N > 0` aggregate partial-success finalization;
- final `FAN_ACCESS` / `FAN_MODIFY` notification policy.

Segment helpers return only progress or error. They no longer decide whether
to notify. Final notification still flows through the existing opened-file
notification helper, so opened-description notification suppression remains in
the same place.

This stayed below RFC level because it does not define a new repository-wide
contract. It only makes an already accepted owner boundary explicit within one
module.

## Change

- Added `anemone-kernel/src/fs/api/read_write/request/mod.rs` for shared
  request-local helpers: checked offset decoding, iovec import, count clamping,
  request position, and kernel-buffer allocation.
- Added `request/read.rs` with `ReadRequest`, read-side direct-user dispatch,
  fallback read/copyout helpers, aggregate read partial handling, and read
  notification finalization.
- Added `request/write.rs` with `WriteRequest`, write-side direct-user
  dispatch, fallback copyin/write helpers, aggregate write partial handling,
  and write notification finalization.
- Simplified read/write syscall wrappers so they construct a normalized request
  after preserving their existing raw ABI gate and import order.
- Removed the outer `*_without_notify()` helper split from
  `read_write/mod.rs`; notification policy is now request-internal and applied
  once per aggregate syscall result.

## Validation

- `just fmt kernel` was run by the implementation worker and passed for the
  edited read-write files. The formatter also exposed pre-existing generated
  file formatting drift and a write-set-external comment wrap; the latter was
  reverted so the final code write set stays inside `fs/api/read_write`.
- `just build` was run by the implementation worker after fixing a request
  module visibility issue and passed.
- `git diff --check -- anemone-kernel/src/fs/api/read_write` reported no
  whitespace diagnostics.
- Source audit confirmed no remaining `*_without_notify`, `read_iovecs`, or
  `write_iovecs` outer protocol helpers in `fs/api/read_write`.
- Source audit confirmed request final notification still calls
  `notify_opened_file_event()`, which preserves
  `FileDesc::notifications_suppressed()`.
- QEMU / LTP were not run for this structure-only iteration.

## Tracking Issues

### CHG-001 - Request object must not become a syscall ABI parser

**Status:** Neutralized
**Severity:** Keter

**Issue:** Moving offset decoding, iovec import, or `pwritev2(flags != 0)`
ordering into `execute()` would let the request object reorder user-visible
errors.

**Resolution:** Syscall wrappers still perform raw offset decode, `flags != 0`
gate, fd/uspace lookup, and iovec import before constructing or executing a
request. `pwritev2` keeps the existing offset-decode-before-flags order and
does not let `execute()` parse `RWF_*`.

### CHG-002 - Notification suppression must stay at opened-description boundary

**Status:** Neutralized
**Severity:** Keter

**Issue:** Request-local notification policy could accidentally bypass generic
fanotify suppression or emit once per iovec segment.

**Resolution:** Segment helpers no longer notify. Request finalization applies
one aggregate notification when the final byte count is greater than zero, and
still routes through `notify_opened_file_event(&FileDesc, mask)`. Fanotify
transaction reads keep the extra `notify_read_user_access()` gate.

### CHG-003 - Formatter drift outside the write set

**Status:** Neutralized
**Severity:** Safe

**Issue:** Running kernel formatting can expose unrelated generated-file drift
or old comment wrapping outside this iteration's write set.

**Resolution:** The write-set-external source formatting change was reverted.
Generated formatting drift remains outside this iteration and is not recorded
as a code change here.

## Risk / Follow-up

This cleanup intentionally does not close existing `RWF_*`, complete Linux
`O_DIRECT`, mmap coherency, splice family, or non-regular direct-user backend
limitations. Runtime `read-write` profile coverage remains a useful follow-up
when the user wants a QEMU / LTP gate, but it is not part of this structure-only
validation floor.

## Links

- Biweekly devlog: [2026-06-22 至 2026-07-05](../2026-06-22_to_2026-07-05.md)
- RFC / transaction: [RFC-20260629-vfs-direct-user-io](../../rfcs/vfs-direct-user-io/index.md), [VFS Direct User I/O 事务日志](../transactions/2026-06-29-vfs-direct-user-io.md)
- Register / limitations: [当前限制](../../register/current-limitations.md)
