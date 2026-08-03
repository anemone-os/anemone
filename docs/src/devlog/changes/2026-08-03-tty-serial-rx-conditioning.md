# ANE-CHG-20260803-tty-serial-rx-conditioning

**Type:** Small feature / contract-bearing TTY input cutover
**Status:** Completed
**Date:** 2026-08-03
**Authors:** doruche, Codex
**Area:** serial / TTY data plane / termios input modes / foreground signal

## Problem / Context

Python 3.13 native PyREPL derives a raw terminal snapshot, clears `INPCK | ISTRIP | IXON`, enables
`BRKINT`, and commits it with `TCSADRAIN`. Anemone previously allowed only `ICRNL` to change in
`c_iflag`, so the complete candidate failed with `EINVAL` before interactive setup. Merely accepting
`BRKINT` as an inert compatibility bit would pass the ioctl while leaving serial break unimplemented,
contradicting the existing rule that successful termios state must have real visible semantics.

The old UART handoff also collapsed break, parity/framing error and overrun into one diagnostic
`line_error` fact beside a byte-only queue. It could not preserve which ordered sample carried a fault,
nor represent a no-payload break without inventing a byte. The smallest closed change therefore covers
the common input-conditioning family that consumes one shared condition handoff:
`IGNBRK`、`BRKINT`、`IGNPAR`、`PARMRK`、`INPCK`、`ISTRIP`、`INLCR`、`IGNCR` 与 `ICRNL`。

## Decision

NS16550A remains the sole owner of LSR/RBR reads, hardware-status classification, the bounded raw
ring and physical diagnostics. Each consumed sample becomes one ordered `Byte`、`Break` or
`FaultedByte`; break wins over parity/framing on the same sample, while overrun remains a counter
because its lost payload cannot be reconstructed. Read-clear LSR status is never probed across a batch
boundary, and RBR is read only when DR says the current sample has a payload.

`TtyPort` transfers only these owner-neutral units in FIFO order. There is no byte/status sideband,
condition queue or replay log. The existing worker owns its dequeued batch and cursor until `Terminal`
fully consumes the current unit; backpressure therefore keeps one unit for retry without duplicating it.

The endpoint-shared `Terminal` remains the sole termios and input-policy owner. It applies normal-byte
conditioning in the order `ISTRIP`、CR/NL mapping、valid-`0xff` quoting, then enters the existing
control-character/canonical/raw pipeline. Break and fault markers are literal tokens: they do not
trigger strip、mapping、control characters、delimiters or echo. A 2/3-byte marker is admitted only
after the complete token fits, so no prefix becomes visible on retry.

`BRKINT` is independent of `ISIG`. At the break's ordered stream position, `Terminal` first flushes
already-conditioned input and pending output that has not reached the port, then returns a guards-out
foreground `SIGINT` effect. Relation、topology and Signal owners revalidate and publish outside the
Terminal guard. A missing or stale foreground target does not roll back the local flush and does not
fall back to the current task or another PGID.

The input flags do not reconfigure baud、parity、data bits or stop bits. Startup defaults remain
unchanged (`ICRNL` only), and any changed unsupported flag such as `IXON` still rejects the whole
candidate with `EINVAL` and preserves the previous snapshot.

## Change

- Added asm-generic input-flag values to `anemone-abi` without changing `Termios` layout or ioctl
  numbers; extended projection、validation and generation-checked commit for the nine supported flags.
- Replaced the byte-only NS16550A raw ring and `TtyPort` dequeue element with ordered RX units; added
  distinct break、fault、overrun and whole-unit overflow diagnostics.
- Added Terminal-owned break/fault/normal conditioning, literal marker admission and break-origin
  flush/signal effects while preserving the existing readiness、canonical/raw and echo owners.
- Extended owner-local KUnit and the RV64 TTY guest/host matrix for status classification、flag
  round-trip、unsupported rollback、condition matrix、marker atomicity and genuine QEMU serial break.
- Kept flow control、`TCFLSH`/`TCSBRK`、`NOFLSH`、PTY、runtime line reconfiguration、hangup and
  canonical overflow policy outside this cutover.

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `TTY-PORT-001` | Refine | byte-only raw ring；line condition只作为不可关联的diagnostic | physical owner发布ordered `Byte`/`Break`/`FaultedByte` units；read-clear status与payload保持同一sample，overrun只计数 | [Serial TTY data plane](../../contracts/tty/data-plane.md#tty-port-001--物理端口与-raw-handoff-只有一个-owner)及本记录证据 |
| `TTY-TERM-001` | Refine | 只有`ICRNL`可变；其它input flags原子拒绝 | 九个input flags由共享Terminal真实round-trip并执行，不反向修改UART line | [Serial TTY data plane](../../contracts/tty/data-plane.md#tty-term-001--endpoint共享唯一terminal-semantic-truth)及本记录证据 |
| `TTY-INPUT-001` | Refine | byte直接进入control/canonical/raw pipeline | committed snapshot驱动break/fault/normal matrix；condition marker all-or-nothing admission并保留worker cursor重试 | [Serial TTY data plane](../../contracts/tty/data-plane.md#tty-input-001--input-ownershiprecord-boundary与readiness同源)及本记录证据 |
| `TTY-JOBCTL-001` | Refine | 只有`VINTR/VQUIT/VSUSP`形成input-origin foreground signal | `BRKINT`先完成Terminal-local flush，再通过既有guards-out relation/topology/Signal handoff生成foreground `SIGINT` | [TTY job control](../../contracts/tty/job-control.md#tty-jobctl-001--terminal-policy只产生经重验的guards-out-effect)及本记录证据 |
| `TTY-ABI-001` | Refine | native PyREPL的`TCSADRAIN` candidate被`EINVAL`拒绝 | input-mode family、QEMU serial break与native Python 3.13 PyREPL进入首版真实兼容包络 | [TTY job control](../../contracts/tty/job-control.md#tty-abi-001--首版兼容包络必须真实可观察)及本记录证据 |

以上 ABI constants、driver classification、raw handoff、Terminal semantics、foreground effect、
focused tests与current contract在同一checkpoint生效；不存在只allow-list `BRKINT`、只保存diagnostic tag
或发布部分marker matrix的中间surface。

## Validation

- `just fmt all --check`、`bash -n scripts/run-tty-test-rv64.sh`、`git diff --check`通过。
- repository-owned RV64 TTY wrapper从决赛BusyBox与只读master image建立worktree-local运行副本；release
  kernel build通过，404/404 KUnit通过，TTY `50/50`、BusyBox vi/ash与host byte oracle全部通过。
- QEMU `mon:stdio`的真实serial-break escape在`ISIG=0`的controlling-terminal/foreground关系下证明：
  break前input被清除、break后`keep\n`保留、foreground只收到一次`SIGINT`；键入Ctrl-C未被用作break证据。
- 决赛RV64 image上的Python 3.13.5分别完成：`python3 -c 'print(1 + 1)'`输出`2`；basic REPL
  执行表达式输出`7`；native PyREPL完成真实terminal prepare、显示交互prompt、执行表达式输出`11`，并以
  Ctrl-D正常返回shell。最终QEMU按维护者指示由monitor直接终止。
- 决赛harness首次在受限sandbox内构建时，lwext4 C编译以`Bad system call` / `SIGSYS`失败；完全相同的
  repository wrapper在sandbox外成功build并启动QEMU，因此该次失败归类为环境限制，不是kernel证据。
- 一位独立agent审查owner/handoff、read-clear register ordering、marker atomicity、backpressure、BRKINT
  guards-out effect、termios rollback与回归边界。首轮发现的两处destructive LSR/RBR Apollyon均已修复；
  同一reviewer复核最终实现后为0 Apollyon / 0 Keter / 0 Euclid。

## Remaining Risk / Links

- LA64 compile/runtime、实体UART parity/framing injection、实体硬件、full LTP与长期压力均Not Run；RV64
  QEMU break、classifier KUnit或决赛Python不能替代这些证据。
- destructive LSR/RBR ordering由源码不变量、注释和classifier/drain KUnit共同保护，尚无fake-MMIO fixture；
  实体UART若暴露variant-specific read-clear差异，应在physical driver owner内补证据，不能把status policy移入TTY。
- marker exact-fit/backpressure/retry由Terminal层验证，worker cursor FIFO/retry由composition KUnit验证；尚无
  专门把concurrent read释放容量与marker retry合成一个stress case的长期fixture。
- Current contracts：[Serial TTY Data Plane](../../contracts/tty/data-plane.md)、
  [TTY Controlling Relation 与 Job Control](../../contracts/tty/job-control.md)、
  [Signal pending/action](../../contracts/signal/pending-routing.md)、
  [process-group signaling](../../contracts/task/process-group-signaling.md)。
- Linux reference：[`n_tty_receive_break()` / `n_tty_receive_parity_error()`](../../xref/linux-6.6.32/drivers/tty/n_tty.c)、
  [`n_tty_receive_char_special()`](../../xref/linux-6.6.32/drivers/tty/n_tty.c)。
- Runtime logs：`build/tty-rx-conditioning-rv64.log`、`build/tty-rx-python-rv64.log`（build-local，未入库）
- RFC / transaction / external source：None
- Issue / PR / commit：this change's Git commit
