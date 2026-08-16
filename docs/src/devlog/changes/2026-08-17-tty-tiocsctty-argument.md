# ANE-CHG-20260817-tty-tiocsctty-argument

**Type:** Small bug fix / local TTY ABI refinement
**Status:** Completed
**Date:** 2026-08-17
**Authors:** doruche, Codex
**Area:** TTY / PTY / controlling relation / Linux ioctl ABI

## Problem / Context

Anemone的`TIOCSCTTY` FileOps在进入relation owner前把所有非零参数解释为unsupported privileged steal并
返回`EPERM`。[`xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tiocsctty`](https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/tree/drivers/tty/tty_jobctrl.c?id=91de249b6804473d49984030836381c3b9b3cfb0#n365)
则先处理精确relation幂等与caller资格，只有目标TTY已经由另一session控制时才让`arg=1`请求steal。因此
session leader在全新、尚未绑定的PTY slave或serial TTY上使用`arg=1`本应执行普通acquisition，当前实现却
错误拒绝；现有serial guest test还把该偏差固定成了oracle。

## Decision / Implementation Boundary

Target是按relation状态解释显式`TIOCSCTTY`参数：unbound endpoint上的任意参数进入普通acquisition，精确
relation的任意参数保持幂等；只有endpoint由另一live session控制时，`arg=1`才是privileged steal request。
Anemone当前没有privileged authority与旧session rebind/cleanup协议，因此真实steal继续记录notice并返回
`EPERM`，且不能改变旧relation。

TTY relation registry继续唯一拥有session-terminal binding、foreground selector、generation以及
`Idempotent / Empty / Conflict`判断；ioctl boundary只把`arg == 1`转换为“若占用则请求steal”的窄输入。
task topology继续拥有session membership与caller资格，serial/PTY endpoint、Session与opened file不保存参数
或relation副本。

本轮不实现privileged steal、capability/credential检查、non-leader `TIOCNOTTY`、implicit acquisition变化、
relation cleanup signal、foreground/job-control新语义或新的public kernel API；不改变errno、file access规则、
generation retry、detach、hangup或PTY retirement ordering。若修复需要移动relation owner、引入第二份binding
truth、增加rebind cleanup或扩大上述target，本小迭代停止并升级RFC。

## Change

- FileOps不再按参数值提前拒绝，转而把state-conditional steal request交给relation owner；
- relation owner只在另一live session实际占用endpoint时记录unsupported steal并返回`EPERM`，普通acquisition、
  exact-relation idempotence、stale cleanup/retry与其它拒绝路径保持原owner和顺序；
- serial `tty-test`覆盖unbound `arg=1`、write-only exact relation上的其它非零参数、live occupied
  `arg=1 -> EPERM`及旧relation保持；public `pty-test`覆盖`O_NOCTTY`打开后的unbound PTY `arg=1` acquisition、
  occupied PTY拒绝及旧relation保持；
- current `TTY-ABI-001`与实现、测试在同一closure checkpoint原子cut over；Closed TTY RFC保持历史冻结。

## Validation

- tracked Linux 6.6.32 `tiocsctty()` source review确认精确relation幂等先于caller/file检查，只有
  `tty->ctrl.session`存在时才检查`arg == 1 && capable(CAP_SYS_ADMIN)`；
- RV64 `tty-test`与`pty-test` app build通过；`qemu-virt-rv64-release` kernel build通过，既有相邻warning未由
  本轮引入；
- fresh RV64 public PTY rootfs与SMP8 QEMU通过692/692 KUnit、`PTYTEST:SUMMARY:PASS:17`、新增unbound/occupied
  relation matrix、retirement/capacity coverage与orderly PowerOff；
- RV64 serial wrapper通过692/692 KUnit、`TTYTEST:SUMMARY:PASS:56`、新增argument-context matrix、BusyBox
  vi/ash、host byte oracle与orderly PowerOff；
- `just fmt kernel --check`、`just fmt tty-test --check`、`just fmt pty-test --check`、`git diff --check`与
  `mdbook build docs`通过。

**Not Run：** LA64 build/runtime、hardware、LTP、真实privileged steal。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `TTY-ABI-001` | Refine | 只交付`arg=0`普通acquisition；FileOps把任意非零参数提前解释成unsupported steal并返回`EPERM` | 参数只在live occupied relation上取得特殊含义；unbound acquisition与exact-relation idempotence不按参数拒绝，`arg=1` actual steal仍notice + `EPERM`且保持旧relation |

`TTY-REL-001`、`TTY-JOBCTL-001`与`TTY-LIFE-001`作为受保护依赖不变：relation registry、topology、Signal、
job-control与cleanup owner均未移动，也没有第二个semantic cutover。

## Remaining Risk / Links

- Privileged steal仍明确不在当前ABI；后续若有真实consumer，必须同时定义authority、旧session
  discoverability撤销、foreground disposition、external effects与cleanup，而不能把本轮bool输入扩张成第二份状态。
- Linux reference：`xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tiocsctty`。
- Current contract：[TTY controlling relation与job control](../../contracts/tty/job-control.md)。
