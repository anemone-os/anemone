# ANE-CHG-20260808-tty-tab3-output

**Type:** Small Feature / local TTY ABI refinement
**Status:** Completed
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** device / TTY / termios / output processing

## Problem / Context

决赛RV64 rootfs中的GNU `less 668`可以取得controlling TTY、读取winsize并进入输入准备，但其
raw-mode `TCSETSW`在Anemone返回`EINVAL`。以Anemone当前`TCGETS`投影作为初始termios的host
qemu-user characterization表明，`less`只新增`TAB3/XTABS`：它保留`ICRNL`、`OPOST/ONLCR`与
`ISIG`，清除`ICANON/ECHO/ECHOE/ECHOK`，并要求output post-processing把tab展开到下一个8列
边界。此前TTY只真实执行`OPOST/ONLCR`，因此按current contract原子拒绝该candidate。

这不是可以静默丢弃的compatibility bit。`TAB3`会改变用户可见字节流；只扩allow-list会让
`tcsetattr()`成功却继续输出literal tab，违反ABI诚实性和完整transform-token progress规则。

## Decision

`TerminalOutput`继续唯一拥有TTY output processing；在既有queue/generation旁保存逻辑输出列。
每个源字节先由当前committed termios与逻辑列形成完整token，只有token全部进入Terminal-owned
queue后才同时推进源字节progress与列号。backpressure不推进二者；output flush丢弃backend work，
不回滚已经提交的output-processor stream位置。

`TABDLY`按asm-generic枚举解码，只接受`TAB0`与`TAB3/XTABS`。`TAB3`在`OPOST`开启时把tab原子
展开为1至8个空格；literal tab、CR、backspace、newline/`ONLCR`和ordinary/control bytes共同维护
同一逻辑列。`TAB1/TAB2` delay modes及其它changed unsupported output flags继续返回`EINVAL`，
不能借本轮形成通用unknown-bit success路径。

## Implementation Boundary

Target是让未修改的GNU `less 668`完成`TCSETSW -> noncanonical input -> TAB3 output -> termios
restore`，并保持每个源字节的atomic transform与partial-progress语义。`Terminal`仍是termios与output
truth owner；FileOps只负责asm-generic ABI投影/校验；UART仍只拥有physical TX serialization。

本轮不实现`TAB1/TAB2`、`OCRNL/ONOCR/ONLRET/OLCUC`、`IXON/IXOFF`、`IEXTEN`、完整Linux termios、
PTY、`man`前端、动态host窗口同步或terminal-emulator escape parsing。若真实guest中的同一GNU
`less`还要求其它changed flag，或正确实现必须把列truth移入UART/console owner，本小迭代停止并重新
解析ABI/owner/validation边界。

## Change

- `anemone-abi`增加asm-generic `TABDLY/TAB0/TAB1/TAB2/TAB3/XTABS`常量，不改变`Termios`布局或
  ioctl number。
- TTY ABI projection/validation真实round-trip `TAB0/TAB3`，以exact mode拒绝`TAB1/TAB2`。
- `TerminalOutput`增加owner-local逻辑列和最多8字节的atomic output token；实现TAB3 expansion以及
  CR、backspace、literal tab、newline/ONLCR和control-byte列更新。
- blocking write与poll共享同一个TerminalOutput-owned writable predicate，按当前列上任意一个源字节的
  最大token计算；TAB3因空间不足失败时不提交列或source progress。
- TTY owner将Kconfig output capacity合法性下限同步为一个最大TAB3 token的8字节，非法配置在内核
  编译期失败。
- inline KUnit覆盖GNU `less` candidate、TAB1/TAB2 rollback、列相关expansion、control byte、
  pass-through tab、ONLCR与backpressure重试。

## Contract Impact / Cutover

以下refinement已在2026-08-08维护者完成GNU `less` guest交互验收、最终source/build/docs检查通过后，
与实现一起原子cut over；链接的current contract是唯一effective正文：

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `TTY-TERM-001` | Refine | Terminal拥有termios、output queue与drain truth，不保存列相关output state | Terminal同时唯一拥有随完整token提交的逻辑输出列与`TAB0/TAB3`模式 |
| `TTY-OUTPUT-001` | Refine | 真实执行`OPOST/ONLCR`，每个token最多2字节 | `TAB3`按当前逻辑列原子展开为1至8个空格，失败不推进列或源字节progress |
| `TTY-ABI-001` | Refine | changed unsupported output flags原子`EINVAL` | `TAB0/TAB3`进入真实termios包络；`TAB1/TAB2`及其它changed unsupported flags仍原子拒绝 |

## Validation

- `just build --preset competition-final-rv64-release --bind smp=8 --bind memory=8G`通过，证明final
  production配置和两遍symbol-table build可编译。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`通过，证明default `kunit`
  feature下新增inline tests可编译；本项不是KUnit运行通过证据。
- `just app build tty-test --arch riscv64`通过；`just fmt kernel --check`、`git diff --check`与
  `mdbook build docs`通过。
- **User Run:** 维护者在决赛RV64 guest中确认GNU `less 668`进入可用全屏界面，并以`q`退出。
- **Not Run:** KUnit runtime、完整TTY auto/vi/job-control matrix、LA64 compile/runtime、LTP、实体UART与
  physical hardware。

## Remaining Risk / Links

- 逻辑列是TTY output-processor state，不声称跟踪host terminal在ANSI escape、console interleave或物理
  device行为后的实际cursor。该边界与Linux N_TTY不解析terminal escape protocol一致。
- Linux reference：[`do_output_char()`](../../xref/linux-6.6.32/drivers/tty/n_tty.c)。
- Current contracts：[Serial TTY data plane](../../contracts/tty/data-plane.md)、
  [TTY controlling relation与job control](../../contracts/tty/job-control.md)。
- RFC / transaction / issue / PR：None
