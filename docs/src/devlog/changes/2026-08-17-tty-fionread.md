# ANE-CHG-20260817-tty-fionread

**Type:** Small iteration / local TTY ABI refinement
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** TTY / PTY / line discipline / Linux ioctl ABI

## Problem / Context

TTY endpoints did not implement the asm-generic `FIONREAD` / `TIOCINQ` alias, so applications could not query the total committed
bytes available to subsequent reads without changing the stream. The missing command crosses three existing read owners: the line
discipline owns serial and PTY-slave input, while the shared Terminal output queue owns bytes exposed to a PTY master after output
processing.

Tracked Linux 6.6.32 defines `TIOCINQ` as the `FIONREAD` alias and routes the N_TTY query through `n_tty_ioctl()` /
`inq_canon()`. The useful compatibility boundary is therefore an `int *` byte-count projection of the actual read-side queue, not
a second counter or a readiness alias.

## Decision / Implementation Boundary

Target is one closure checkpoint that implements `FIONREAD` / `TIOCINQ` for serial TTY, PTY slave and PTY master. Serial and PTY
slave report committed line-discipline bytes: canonical pending edit is excluded, completed records are included, and raw mode
reports the committed byte-stream length. An empty canonical `VEOF` record can still make read/poll ready while the byte count is
zero. PTY master reports bytes already present in the Terminal output queue after `OPOST` transforms, because those are the bytes
its read path will consume.

The discipline and Terminal queues remain the only state owners. The ioctl takes an owner-local snapshot without consuming data;
the PTY master uses the existing pair operation lock for its liveness/query transaction. The common FileOps boundary converts the
`usize` snapshot to Linux `int`, releases owner locks before user copyout, returns `EFBIG` if the value cannot fit, and preserves
ordinary user-copy `EFAULT` behavior.

This iteration does not change read/poll predicates, canonical record boundaries, `VMIN/VTIME`, output processing, `FIONBIO`,
unknown-ioctl behavior, PTY lifecycle, hangup ordering, public kernel traits or architecture-specific code. If implementation
required a cached count, a second queue truth, a widened lifecycle interface or a new generic ioctl framework, the iteration would
stop for boundary review.

## Change

- ABI exports `TIOCINQ` as the existing `FIONREAD` value;
- serial and PTY-slave ioctl dispatch snapshot the line-discipline committed byte count;
- PTY-master dispatch snapshots the transformed Terminal output queue under the pair operation lock;
- inline owner-local KUnit covers canonical pending/committed/partial/empty-EOF/raw input and transformed output accounting;
- public `tty-test` and `pty-test` cover aliasing, non-consuming queries, partial reads, bad pointers, empty EOF, raw input,
  transformed master output and hangup/liveness outcomes;
- `TTY-INPUT-001`, `TTY-OUTPUT-001`, `TTY-ABI-001` and `PTY-ABI-001` refine atomically with this completed checkpoint.

## Validation

- RV64 `pty-test` and `tty-test` app builds and `qemu-virt-rv64-release` kernel build passed; existing adjacent warnings were not
  introduced by this iteration;
- fresh RV64 public PTY rootfs on SMP8 QEMU passed 701/701 KUnit, `PTYTEST:PASS:input-queue-query`,
  `PTYTEST:SUMMARY:PASS:18` and orderly PowerOff;
- RV64 serial wrapper passed 701/701 KUnit, `TTYTEST:PASS:input-queue-query`, `TTYTEST:SUMMARY:PASS:57`, BusyBox vi/ash,
  host byte oracles and orderly PowerOff;
- final independent review found and corrected an over-broad absent-slave zero-count claim and a single-read wording error; the
  public lifecycle oracle now proves retained master output is counted and drained before absent-peer `EIO`;
- `just fmt kernel --check`, `just fmt pty-test --check`, `just fmt tty-test --check`, `git diff --check` and
  `mdbook build docs` passed.

**Not Run:** LA64 build/runtime, hardware, LTP.

## Contract Impact / Cutover

| Contract ID | Change | Previous effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `TTY-INPUT-001` | Refine | read/poll consume and project the discipline-owned input truth | queue query reports committed readable bytes without consuming; canonical pending edit is excluded and empty EOF may remain ready at count zero |
| `TTY-OUTPUT-001` | Refine | PTY master consumes transformed Terminal output | master queue query reports the transformed bytes currently consumable from that same queue |
| `TTY-ABI-001` | Refine | `FIONREAD` / `TIOCINQ` absent from the TTY ioctl envelope | serial and PTY slave expose the Linux alias with checked `int *` copyout and stable snapshot semantics |
| `PTY-ABI-001` | Refine | no read-side queue-count ioctl | slave maps to discipline input; live master maps to Terminal output, including output retained after last-slave close; an empty live-master queue reports zero and post-hangup slave query returns `EIO` |

`TTY-TERM-001`, `TTY-LIFE-001`, `PTY-PAIR-001` and iomux/opened-description contracts remain protected dependencies; no state
owner, readiness predicate or lifecycle transition moved.

## Remaining Risk / Links

- Counts are bounded by existing queues, so the checked `usize -> int` overflow path is source-proved but not runtime-forced.
- Linux reference: `xref:linux-6.6.32:include/uapi/asm-generic/ioctls.h#TIOCINQ`,
  `xref:linux-6.6.32:drivers/tty/n_tty.c#inq_canon` and `#n_tty_ioctl`.
- Current contracts: [TTY data plane](../../contracts/tty/data-plane.md),
  [TTY controlling relation and ABI](../../contracts/tty/job-control.md), and
  [Unix98 PTY and devpts](../../contracts/tty/pty-devpts.md).
