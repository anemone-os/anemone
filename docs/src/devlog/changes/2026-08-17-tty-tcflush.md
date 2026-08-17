# ANE-CHG-20260817-tty-tcflush

**类型：** 小迭代 / TTY 局部 ABI 与 worker handoff refinement
**状态：** Completed
**日期：** 2026-08-17
**作者：** doruche、Codex
**领域：** TTY / PTY / line discipline / serial worker / Linux ioctl ABI

## 问题与背景

TTY 与 PTY endpoint 原先拒绝 asm-generic `TCFLSH` ioctl。只在 ioctl 分支中调用
`TerminalOutput::clear()`不能形成正确语义：serial worker可能已经从physical port取得尚未提交line discipline的
RX batch，也可能正在执行Terminal output的`peek -> port submit -> consume`；PTY master相对slave还会反转
input/output视图。

仓库跟踪的Linux 6.6.32把`TCIFLUSH`、`TCOFLUSH`与`TCIOFLUSH`定义为三个有效scalar selector，并在
selector校验前执行terminal-modifying job-control检查。本轮交付完整selector family，不为其它值提供silent
compatibility。

## 决策与实现边界

本迭代使用一个closure checkpoint。ioctl front拥有selector解析、PTY视图投影、支持范围内的background
`SIGTTOU` policy与errno；Terminal、physical port和PTY pair继续分别拥有queue、hardware handoff与lifecycle truth。

对serial TTY，input flush丢弃line-discipline input、port已经接纳的raw RX，以及worker在flush前取得但尚未提交的
本地RX batch。output flush只丢弃尚未提交port的Terminal output；已经被UART接受的byte不撤回。worker的RX/TX
transfer与flush共享attachment-local mutex，input generation只用于退休pre-flush worker batch，不描述任何queue
内容。IRQ在port discard之后新接纳的hardware sample属于flush后输入，可以继续到达。

对PTY，既有pair operation/liveness owner负责flush与read、write、retirement之间的顺序。slave input与master
output映射line-discipline queue；slave output与master input映射Terminal output queue；`TCIOFLUSH`在同一次成功
admission中清除两者。flush不改变termios、winsize、logical output column、relation、pair lifecycle或
opened-description state。

本迭代不新增第二份queue truth、缓存readiness、测试驱动的production hook、持久request/ack协议或新public API。
orphaned process-group `EIO`仍在现有job-control contract范围外；本轮复用已交付的non-orphan foreground、
blocked/ignored与actionable `SIGTTOU`包络。若correctness需要新lifecycle owner、跨worker可取消等待、target弱化或
扩大task topology协议，本小迭代必须停止并重新分级。

## 实现

- ABI补齐`TCFLSH`、`TCIFLUSH`、`TCOFLUSH`与`TCIOFLUSH`常量；非法selector返回`EINVAL`；
- TTY backend capability同时提供wake与queue flush，但不携带queue、capacity、readiness或peer snapshot；
- serial backend以同一个transfer mutex覆盖worker RX batch、TX `peek -> submit -> consume`与flush，port只增加
  owner-local raw RX discard capability；
- Terminal按input/output/both清除owner-local queue，output clear保留已经提交的逻辑列，并在完整transaction后统一
  publish progress；
- PTY复用pair operation/liveness admission，ioctl front对master反转input/output投影；
- serial与PTY slave在selector校验前执行terminal-modifying access check，actionable background caller先取得
  `SIGTTOU`；PTY master不作为controlling-terminal view参与该检查；
- owner-local inline KUnit覆盖selector codec、input/output/both、flush后新数据、逻辑列、raw RX discard与
  worker-local batch退休；public `tty-test`和`pty-test`执行真实ioctl与byte-visible方向矩阵。

## 验证

- `just fmt kernel --check`、`just fmt pty-test --check`、`just fmt tty-test --check`、RV64两个app build、
  `just build --preset qemu-virt-rv64-release`与`git diff --check`通过；只有既有相邻warning；
- RV64 public PTY SMP8运行通过784/784 KUnit、`PTYTEST:PASS:tcflush-direction-matrix`、
  `PTYTEST:SUMMARY:PASS:19`并有序关机；
- RV64 serial wrapper运行通过784/784 KUnit、`TTYTEST:PASS:tcflush-input`、
  `TTYTEST:PASS:tcflush-sigttou-stop`、`TTYTEST:SUMMARY:PASS:59`、BusyBox vi/ash、host byte oracle与有序关机；
- focused LTP把glibc/musl `ioctl02 -d /dev/ttyS0`各尝试一次；重试时KUnit 784/784通过，但两次均在执行
  `TCFLSH`断言前被LTP框架读取`/proc/meminfo`的`Expected 1 conversions got 0`前置条件阻断。因此该结果是
  Not Run / Inconclusive，不计为功能PASS或FAIL；临时profile与group修改已全部恢复；
- `pty03`、`pty04`与`pty05`依赖本轮之外的line discipline、`TCXONC`等能力，未作为本次focused LTP运行。
- 最终独立变更审阅确认0 Apollyon、0 Keter、0 Euclid；Architecture Friction Scan没有发现第二份状态真相、
  owner穿透、private representation泄漏、扩大public API、测试特判、无退出条件临时桥、隐含failure/cleanup或
  无真实义务的新抽象。

**未运行：** LA64 build/runtime、实体UART/hardware。LA64按用户授权不构建；RV64与源码审查证据不得外推到这些
边界。

## 合约影响与切换

以下refinement在本closure checkpoint完成全部验收后与实现原子cut over；链接的current contract是唯一effective
正文：

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `TTY-INPUT-001` | Refine | 显式flush只覆盖既有termios/BRKINT内部路径 | `TCIFLUSH`清discipline、pre-flush worker batch与port raw RX；flush后输入继续可达 |
| `TTY-OUTPUT-001` | Refine | output clear是owner-local primitive，没有公开selector ABI | `TCOFLUSH`与serial TX transfer仲裁，只丢弃尚未提交port的output且不回退逻辑列 |
| `TTY-JOBCTL-001` | Refine | terminal-modifying `SIGTTOU`仅覆盖既有relation ioctl | serial与PTY slave的`TCFLSH`在selector校验前复用non-orphan background policy；master跳过 |
| `TTY-ABI-001` | Refine | TTY ioctl拒绝`TCFLSH` | 三个asm-generic selector成功，非法值`EINVAL`，serial/slave/master投影真实可观察 |
| `PTY-ABI-001` | Refine | PTY没有显式queue-flush ioctl | slave/master按各自read/write view映射两条共享Terminal队列并服从pair lifecycle |

`TTY-PORT-001`、`TTY-TERM-001`、`TTY-LIFE-001`与`PTY-PAIR-001`保持受保护依赖；没有移动queue、relation或
lifecycle owner。

## 剩余风险与链接

- serial output已提交UART的边界由transfer lock与source audit证明，public PTY提供完整双向byte oracle；实体UART
  没有运行，不能声称hardware FIFO可撤回；
- focused LTP的阻断发生在目标断言之前，只保留为环境前置条件证据；
- Current contracts：[TTY data plane](../../contracts/tty/data-plane.md)、
  [TTY controlling relation与job control](../../contracts/tty/job-control.md)、
  [Unix98 PTY与devpts](../../contracts/tty/pty-devpts.md)；
- Linux参考：`xref:linux-6.6.32:include/uapi/asm-generic/ioctls.h#TCFLSH`、
  `xref:linux-6.6.32:drivers/tty/tty_ioctl.c#__tty_perform_flush`、
  `xref:linux-6.6.32:drivers/tty/tty_jobctrl.c#tty_check_change`、
  `xref:linux-6.6.32:drivers/tty/pty.c#pty_flush_buffer`；
- RFC / transaction / register：None。
