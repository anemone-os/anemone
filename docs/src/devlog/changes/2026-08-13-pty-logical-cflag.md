# ANE-CHG-20260813-pty-logical-cflag

**Type:** Small bug fix / local PTY ABI refinement
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** TTY / PTY / termios / `c_cflag`

## Problem / Context

Serial TTY首版以physical driver提交的boot-applied baud、data bits和parity形成immutable
line snapshot，并在没有runtime hardware apply/rollback协议时拒绝改变这些字段。PTY复用shared
`Terminal`后，devpts却用`B38400`/8N1构造同一种physical snapshot；generic `TCSETS*`
validation随后要求整个`c_cflag`与该投影精确相等。结果是没有物理线路的PTY也被错误绑定到
boot-line规则，普通PTY read-modify-write candidate会得到`EINVAL`。

tracked Linux 6.6.32同样给Unix98 PTY初始`B38400 | CS8 | CREAD`，但`pty_set_termios()`
只把不适用于PTY的character size/parity/receiver字段归一化为`CS8 | CREAD`，其余logical
compatibility bits继续保留。现有RV64 final rootfs运行已经观察到PTY master fd上的
`tcsetattr(TCSANOW)`返回`EINVAL`；该日志没有保存candidate，因而只作为真实consumer触达证据，
不单独证明具体触发bit或后续tmux退出根因。

## Decision

`Terminal`继续唯一拥有完整committed termios与generation transaction。cflag policy成为该snapshot的
组成部分：serial保存immutable physical line；PTY保存logical `c_cflag`。`TCGETS`、validation和最终
commit都读取/提交同一份snapshot，不再让devpts伪造physical line，也不让FileOps或pair保存第二份
termios truth。

PTY初始logical profile为`B38400 | CS8 | CREAD`。首版接受asm-generic standard Bfoo speed、`CSTOPB`、
`PARODD`、`HUPCL`、`CLOCAL`、input-speed field、`CMSPAR`与`CRTSCTS`作为只参与ABI round-trip的
logical bits；每次提交清除`CSIZE | PARENB`并强制`CS8 | CREAD`。这些bit不驱动physical line、
pair lifecycle或data-plane decision。legacy `TCSETS*`没有`c_ispeed/c_ospeed`，所以output/input `BOTHER`、
`ADDRB`和其它未进入本轮包络的changed bit继续返回`EINVAL`并保持旧snapshot。serial仍要求完整
hardware-backed projection不变。

## Implementation Boundary

Target是分离physical serial与logical PTY的cflag policy，让PTY `TCSETS/TCSETSW/TCSETSF`真实提交、
归一化并从master/slave共享round-trip首版logical mask，同时保持serial immutable line语义。
Owning subsystem是`device::tty` shared `Terminal`；serial driver只提交boot line，devpts只请求一个
PTY terminal，pair/FileOps持窄capability。

本轮不改变`Termios` layout、ioctl number、termios generation/drain/flush ordering、pair lifecycle、
job control、hangup、readiness、opened-description或VFS owner；不实现serial runtime line
reconfiguration、`TCGETS2`/`BOTHER`、legacy `termio`/break ioctl、flow-control behavior、packet mode、
完整Linux termios或tmux自身语义。若正确实现需要master/slave各存termios、backend apply/rollback、
新的shared owner/public API、第二个semantic cutover或扩大acceptance，本小迭代停止并升级RFC。

## Change and Acceptance

- committed termios以内嵌profile区分physical line与PTY logical cflag；构造入口分别来自serial attach和
  PTY allocation；
- generic termios projection/validation按profile执行，成功candidate继续沿既有generation transaction
  原子提交；
- inline KUnit证明serial拒绝hardware change、PTY logical round-trip/normalization及unsupported rollback；
- public `pty-test`新增独立`termios-cflag-profile`，覆盖初始值、master/slave共享、三种`TCSETS*`模式、
  normalization、input-speed/`CMSPAR` round-trip和`BOTHER`/unsupported失败rollback；
- 双架构app/kernel build、RV64 SMP8 KUnit与`PTYTEST`、existing serial `TTYTEST`、格式和docs验证均已通过；
  实现、测试和下述current-contract refinement在同一closure checkpoint原子cut over。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Target refinement |
| --- | --- | --- | --- |
| `TTY-TERM-001` | Refine | shared Terminal持termios，但初始physical line snapshot被serial与PTY共用并永久决定`c_cflag` | committed termios内含endpoint profile；serial保存physical projection，PTY保存logical cflag，二者仍由同一Terminal/generation transaction唯一拥有；本checkpoint已生效 |
| `PTY-ABI-001` | Refine | PTY复用modern `TCGETS/TCSETS*`，但`c_cflag`只能与伪造的38400/8N1 physical projection精确相等 | PTY以38400/8N1作为初始logical profile，按首版mask round-trip并以`CS8 | CREAD`归一化不存在的physical framing fields；本checkpoint已生效 |

`TTY-PORT-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、`PTY-PAIR-001`、job-control、Signal、
opened-description与VFS contracts是受保护依赖，不发生contract delta。

## Validation

- RV64与LA64分别通过`just app build --arch <arch> pty-test`；
- RV64与LA64 SMP8 release kernel分别通过repository `just build` preset；LA64 runtime Not Run，不能从build外推；
- fresh RV64 PTY acceptance rootfs与SMP8 QEMU通过628/628 KUnit、`PTYTEST:SUMMARY:PASS:14`、retirement/
  capacity reuse及orderly PowerOff；新增`termios-cflag-profile`覆盖初值、master/slave共享、`TCSETS`、
  `TCSETSW`、`TCSETSF`、normalization和unsupported rollback；
- existing RV64 physical TTY wrapper通过628/628 KUnit、`TTYTEST:SUMMARY:PASS:50`、BusyBox vi/ash、host binary/
  ONLCR/drain byte oracle与orderly PowerOff，证明serial immutable line边界未回归；
- `just fmt kernel --check`、`just fmt pty-test --check`、`git diff --check`与`mdbook build docs`通过。

## Remaining Risk / Links

- tmux与package installation中其它`sync`、server-exit failure signature继续独立归因；本轮只关闭确定的
  PTY `c_cflag`模型与ABI缺口。
- Linux reference：[`pty_set_termios()`](../../xref/linux-6.6.32/drivers/tty/pty.c)。
- Current contracts：[TTY data plane](../../contracts/tty/data-plane.md)、
  [Unix98 PTY与devpts](../../contracts/tty/pty-devpts.md)。
