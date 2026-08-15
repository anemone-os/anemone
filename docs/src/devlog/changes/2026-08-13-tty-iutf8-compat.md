# ANE-CHG-20260813-tty-iutf8-compat

**Type:** Small Feature / local TTY ABI refinement
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** TTY / PTY / termios / Linux N_TTY compatibility

## Problem / Context

Anemone的asm-generic termios ABI尚未定义或接受`IUTF8`。changed bit会被`TCSETS*`原子拒绝，
canonical `VERASE`也固定删除一个byte。tracked Linux 6.6.32的N_TTY并不验证完整UTF-8；它只把
`10xxxxxx`识别为continuation byte，用于canonical erase和output/echo逻辑列。tmux 3.5a创建pane时会
显式启用`IUTF8`，因此当前拒绝既是独立ABI缺口，也会阻断真实PTY consumer。

同一审计还确认Linux 6.6.32稳定保存但不执行`XCASE`、`FLUSHO`、`PENDIN`、output delay modes以及
`OFILL/OFDEL`。这里的output delay modes指`NLDLY`、`CRDLY`、`TAB1/TAB2`、`BSDLY`、`VTDLY`、
`FFDLY`，不包含具有真实展开行为的`TAB3`。继续因mask过窄拒绝这些字段没有数据面收益；但其它具有真实行为的flag不能因此被
降级成success-no-op。

## Decision

实现tracked Linux 6.6.32 N_TTY的完整`IUTF8`语义：只识别continuation byte，不增加UTF-8 decoder、
codepoint cache或per-byte编码标签。`IUTF8`影响canonical `VERASE`的删除跨度，以及output/echo processor
对continuation byte的零列宽计算。malformed input沿用Linux规则：只有continuation bytes的pending suffix
不得部分删除；一旦向前找到任意非continuation byte，则把它与suffix一起删除。

明确接纳并精确round-trip以下compatibility set，但不为其伪造行为：

- `c_lflag`：`XCASE`、`FLUSHO`、`PENDIN`；
- `c_oflag`：`NLDLY`、`CRDLY`、`TAB1/TAB2`、`BSDLY`、`VTDLY`、`FFDLY`、`OFILL`、`OFDEL`。

`TAB3/XTABS`仍执行真实展开。`IMAXBEL`、其它具有Linux-visible行为的flag、unknown bits以及要求未实现
hardware action的字段继续原子`EINVAL`，失败保持旧snapshot。

## Change / Implementation Boundary

Target是让serial与PTY共享的`Terminal`唯一拥有并执行上述`IUTF8`语义，同时扩大到经过tracked Linux
6.6.32证明的明确无行为compatibility set。FileOps只负责asm-generic ABI投影/校验；discipline拥有
canonical pending edit；output processor拥有逻辑列。pair、UART、console和opened file不保存第二份UTF-8
或compatibility状态。

本轮不实现完整UTF-8校验、Unicode宽度、normalization、`IMAXBEL`满队列策略、flow control、`IEXTEN`
扩展编辑族、其它output transforms、job-control residual、external processor或serial runtime hardware
reconfiguration；不改变`Termios`布局、ioctl number、generation/drain/flush ordering、PTY lifecycle或relation。

成功update沿既有generation transaction一次提交；invalid candidate失败时termios、input edit、output queue与
逻辑列均不变。erase必须在确认完整删除跨度与完整echo token可接纳后再修改input。若实现需要PTY/serial
分叉状态、新owner/public API、compatibility flag行为、多个semantic cutover或降低下述验收强度，本小迭代
停止并重新分类。

## Change

- asm-generic ABI补齐`IUTF8`、明确compatibility set、`IMAXBEL`及相关枚举常量，不改变`Termios`布局或
  ioctl number；FileOps按精确mask投影、校验并在成功generation commit后观察compatibility变化；
- `Terminal`的committed termios保存`IUTF8`与compatibility snapshot；output processor按Linux continuation
  规则维护逻辑列，`TAB1/TAB2`仅保存而`TAB3`继续展开；
- discipline按Linux `eraser()`计算canonical UTF-8删除跨度，只保存tab erase所需的canonical起始列snapshot；
  erase的input跨度、完整echo容量与output列在同一个Terminal guard内原子提交；
- 最终review发现初版把tab erase编码成普通backspace，导致`OPOST`关闭时bytes发出但逻辑列不回退。修正为
  owner-local `EraseTab` echo operation，像Linux `ECHO_OP_ERASE_TAB`一样绕过普通OPOST，并增加原反例的KUnit
  与public PTY byte oracle；未引入第二份output-column truth或PTY/serial分叉路径；
- public `pty-test`和existing `tty-test`扩展IUTF8、compatibility、rollback、canonical erase、column与TAB3覆盖，
  RV64 TTY wrapper增加UTF-8/TAB3 host byte oracle。

## Validation

- tracked Linux 6.6.32 source review逐项对照`is_continuation()`、`eraser()`、`do_output_char()`与
  `ECHO_OP_ERASE_TAB`，并确认明确compatibility fields不进入N_TTY行为分支；`IMAXBEL`、unknown bits及其它
  semantic flags仍原子拒绝；
- corrected RV64 PTY SMP8运行通过633/633 KUnit、`PTYTEST:SUMMARY:PASS:16`、retirement/capacity matrix与
  orderly PowerOff；新增public oracle重演非零canonical base、`OPOST`关闭后tab erase、再启用TAB3的列状态；
- corrected RV64 TTY wrapper通过633/633 KUnit、`TTYTEST:SUMMARY:PASS:55`、UTF-8/TAB3 host byte oracle、
  BusyBox vi/ash与orderly PowerOff；
- RV64与LA64的`pty-test`、`tty-test` app build均通过；RV64 PTY runtime候选和LA64 SMP8 KUnit-enabled release
  kernel build通过。LA64 runtime未从build结果外推；
- independent final change review确认上轮tab erase finding已闭合，最终为0 Apollyon、0 Keter、0 Euclid；
  Architecture Friction Scan没有发现第二份状态真相、owner穿透、public API扩大、test特判或降低ABI oracle；
- `just fmt kernel --check`、`just fmt pty-test --check`、`just fmt tty-test --check`、`git diff --check`与
  `mdbook build docs`通过。

**Not Run：** LA64 runtime、LTP、tmux、实体UART/hardware。serial `tty-test`没有单独重演`OPOST`关闭的tab
erase；该共享Terminal生产路径由public PTY oracle与两次RV64 runtime KUnit覆盖，review判定为非阻断残余风险。

## Contract Impact / Cutover

以下refinement已在2026-08-13完成上述验收与独立review后，与实现一起原子cut over；链接的current contract是
唯一effective正文：

| Contract ID | 变化 | 先前effective baseline | Effective refinement |
| --- | --- | --- | --- |
| `TTY-TERM-001` | Refine | Terminal保存既有termios与逻辑列；`IUTF8`和明确compatibility set均被拒绝 | 同一committed snapshot保存并执行`IUTF8`，精确compatibility set只保存/投影且不驱动数据面 |
| `TTY-INPUT-001` | Refine | canonical `VERASE`固定删除一个byte | `IUTF8`按Linux continuation规则原子删除一个可能多byte的erase unit；tab erase同步提交raw echo与列回退 |
| `TTY-OUTPUT-001` | Refine | 每个ordinary non-control byte推进一列，TAB3按该列展开 | `IUTF8` continuation byte原样输出但零列宽；tab erase独立于`OPOST`回退逻辑列 |
| `TTY-ABI-001` | Refine | `IUTF8`、`TAB1/TAB2`及其它changed unsupported flags原子拒绝 | 完整`IUTF8`行为与精确Linux no-behavior compatibility set进入真实包络；其它行为/unknown flags继续原子拒绝 |

`PTY-ABI-001`保持依赖，不改变PTY lifecycle、pair owner或PTY-specific ABI。

## Remaining Risk / Links

- Current contracts：[TTY data plane](../../contracts/tty/data-plane.md)、
  [TTY controlling relation与job control](../../contracts/tty/job-control.md)、
  [Unix98 PTY与devpts](../../contracts/tty/pty-devpts.md)。
- Linux reference：[`n_tty.c`](../../xref/linux-6.6.32/drivers/tty/n_tty.c)、
  [`termbits.h`](../../xref/linux-6.6.32/include/uapi/asm-generic/termbits.h)。
- RFC / transaction / issue / PR：None。
