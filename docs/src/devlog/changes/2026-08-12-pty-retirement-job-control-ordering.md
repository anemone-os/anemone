# ANE-CHG-20260812-pty-retirement-job-control-ordering

**Type:** Small bug fix / local PTY lifecycle contract refinement
**Status:** Completed
**Date:** 2026-08-12
**Authors:** doruche, Codex
**Area:** PTY / opened-description final release / TTY job control / Signal ordering

## Problem / Context

PTY master opened-description final release先在pair owner内提交`PairPhase::Retired`，但随后要在guard外完成
Terminal cleanup、devpts binding retirement和TTY relation retirement。旧实现没有把这个cleanup窗口与TTY
guards-out effect形成同一个提交顺序。另一个CPU上的既有slave fd可以在pair已经retire后，仍从尚未撤销的
relation取得background-read decision，发布`SIGTTIN`或返回background `EIO`；master-input control signal、
winsize `SIGWINCH`和relation ioctl也有同类边界。

这不能由一次无锁`is_retired()`检查修复：检查与relation/topology/Signal owner调用之间仍可发生retirement。
尤其是late `SIGTTIN`可能晚于hangup `SIGCONT`发布，把reader留在stopped状态。VFS已经正确提供
opened-description final-release trigger；缺少的是PTY pair owner内对retirement与外部effect admission的共同仲裁，
不是generic VFS lifecycle能力。

## Decision

PTY pair增加不可克隆的bounded effect permit。pair phase与permit admission在同一个pair-owned临界区仲裁：

- retirement先提交时，后续operation不能取得permit；slave read直接投影为EOF，其它operation保持既有
  post-hangup errno；
- operation先提交时，它取得permit，才允许在pair guard外完成本轮relation/topology/Signal effect；
- permit只证明一个有界operation先于retirement提交，不携带liveness truth，不可复制，也不得跨blocking wait；
- retirement先禁止新permit、撤销devpts binding与relation discoverability，再等待旧permit排空，最后才发布
  relation hangup的`SIGHUP/SIGCONT`并通知普通waiter。

pair不进入relation、Signal或task job-control owner，relation snapshot和signal request也不反向驱动pair phase。
serial TTY没有PTY retirement episode，继续使用原有guards-out handoff，不建立伪造的permit。

## Implementation Boundary

Target是让master retirement与PTY guards-out effect形成一个可证明的二选一提交顺序，并由SMP8 public regression
覆盖master-final-close/background-slave-read。Owning subsystem是`device::tty::pty`；pair唯一拥有phase、active
bounded-effect count与drain event。TTY FileOps只请求窄permit并保持既有ABI投影；relation、Signal、task topology、
ThreadGroup job control、devpts、VFS与opened-description owner保持不变。

本轮不改变opened-description final-release API、VFS fd/close模型、Signal pending/action、job-control stop/continue、
relation generation、PTY UAPI或post-hangup errno；不实现orphaned process-group规则、`TOSTOP`或generic hardware
hangup。若修复需要修改generic VFS lifecycle、迁移上述owner、引入production test hook、改变ABI/acceptance或形成
第二个semantic cutover，本小迭代停止并升级RFC。

## Change

- `PtyPairState`在唯一inner中增加checked active-effect count，并持有只服务drain notification的`Event`；
  `PtyEffectPermit::Drop`以assertion保护underflow，最后一个permit发布drain hint。
- `TtyOperation`增加PTY-private bounded-effect entry。slave read在background policy前取得permit，持有到
  decision/signal/read attempt结束，并在任何blocking wait前释放；retirement-first admission failure投影为EOF。
- relation ioctl的bounded snapshot/effect、changed winsize的`SIGWINCH` tail和master input control-character signal使用
  同一permit规则；getter copyout与`TIOCSPGRP` copyin位于permit外，后者仍保持access check先于用户参数且
  post-hangup返回`ENOTTY`，其它既有errno不变。
- master final release固定为retire pair、Terminal cleanup、retire binding、撤销relation、等待pre-retirement
  permits、发布hangup effect、notify waiters；等待期间不持pair/relation/Terminal guard。
- owner-local inline KUnit确定性覆盖permit-first与retirement-first；public `pty-test`增加128轮SMP8回归，reader与
  closer分别收窄到不同fixed scheduler owner，使用pipe、`/proc/<pid>/status`与test-only recovery闭合late-stop失败。

## Contract Impact / Cutover

以下refinement与实现、测试在本checkpoint一起cut over；链接的current contract是唯一effective正文：

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `PTY-PAIR-001` | Refine | pair phase决定liveness，但guards-out effect没有与retirement共享提交点 | pair phase与bounded-effect admission由同一owner仲裁；permit不是第二份liveness truth |
| `TTY-JOBCTL-001` | Refine | relation/topology revalidation约束guards-out effect，未定义PTY retirement winner | PTY effect只有取得pre-retirement permit后才能在guards-out发布；retirement-first不产生stale job-control effect |
| `TTY-LIFE-001` | Refine | pair先retire、relation再撤销并发布hangup effect | relation先撤销；pre-retirement effects全部结束后，hangup `SIGHUP/SIGCONT`才发布 |

`PTY-ABI-001`、opened-description lifecycle和Signal/job-control contracts是受保护依赖，没有ABI或owner变化。

## Validation

- `just app build --arch riscv64 pty-test`通过；新增public fixture可由RV64 target编译。
- `just app build --arch loongarch64 pty-test`与
  `just build --preset qemu-virt-la64-release --bind smp=8 --bind memory=8G`通过；这是LA64静态构建证据，
  不外推runtime。
- `just build --preset qemu-virt-rv64-release --bind smp=8 --bind memory=8G`通过，包含新增inline KUnit。
- 每次QEMU前重新执行`just rootfs mkfs -c conf/rootfs/pty-acceptance-rv64.toml`；一次干净、单实例
  `just qemu --preset qemu-virt-rv64-release --bind smp=8 --bind memory=8G ...`中625/625 KUnit通过，
  `PTYTEST:PASS:retire-background-read-smp8`与`PTYTEST:SUMMARY:PASS:13`通过。
- SMP8 case先核对8个CPU均在available mask，再识别leader fixed owner，并以最多两轮完整CPU topology的bounded
  fork placement选择不同owner的closer；128轮中两种合法ordering都必须终止，late stop由durable proc state判失败。
- `just fmt kernel --check`、`just fmt pty-test --check`、`git diff --check`与`mdbook build docs`通过。
- 独立change review发现relation ioctl permit覆盖user-memory copy的Keter；修正为getter snapshot后先释放permit再
  copyout，`TIOCSPGRP`在既有access check和copyin后才为最终revalidation/effect取得permit。除该已修finding外，
  独立review未发现Apollyon或其它Keter；修正后source audit确认owner、drain predicate、effect coverage、SMP8
  cleanup与KUnit shape成立。

## Remaining Risk / Links

- LA64 runtime、实体硬件、LTP与长期压力Not Run，RV64 SMP8与双架构build不能外推这些范围。
- public race是bounded stress，不用概率替代owner-local proof；共同仲裁点与permit count由确定性KUnit和source
  review证明，SMP8只验证真实cross-CPU composition与late-stop recovery。
- Current contracts：[Unix98 PTY 与 devpts](../../contracts/tty/pty-devpts.md)、
  [TTY controlling relation 与 job control](../../contracts/tty/job-control.md)。
- RFC / transaction / external source：None
- Issue / PR / commit：this change's Git commit
