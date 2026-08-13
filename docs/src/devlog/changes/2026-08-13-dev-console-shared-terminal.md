# ANE-CHG-20260813-dev-console-shared-terminal

**Type:** Small iteration / local device and TTY contract refinement
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** console / TTY / devfs / boot I/O

## Problem / Context

`/dev/console`已经由console owner以Linux设备号5:1发布，boot fd 0/1/2和`/dev/ttyS<N>`也已经打开
boot-selected serial endpoint的真实shared `Terminal`。但console节点仍返回旧anonymous console
FileOps：read固定EOF、write要求UTF-8并直接走printk console fan-out，poll/ioctl不具备TTY语义。于是同一个
boot-selected console具有两套打开语义，`/dev/console`不能共享termios、winsize、readiness、binary output与
input；这既不符合当前endpoint模型，也偏离Linux中`/dev/console`作为当前system console访问入口的地位。

## Decision

console继续唯一拥有selected-console truth、5:1设备号和devfs node publication；TTY继续唯一拥有endpoint、
`Terminal`、FileOps和transport progress。boot finalize由TTY按immutable console selection解析selected endpoint，
只向console交付一个“构造一次真实opened TTY file”的窄capability。console节点的每次open通过该capability取得
与boot fd及对应`ttyS<N>`相同的shared `Terminal`和TTY FileOps，不复制termios/readiness/queue，不让console
访问TTY registry或endpoint表示，也不让TTY接管`/dev/console`。

`/dev/console`仍不是caller-relative controlling-terminal入口；打开它不会建立controlling relation，`/dev/tty`
继续独立按caller relation解析。旧anonymous console stdin/stdout helper保留给其既有非devfs用途，不作为
`/dev/console` fallback。

## Implementation Boundary

Target是让boot-selected `/dev/console` open复用selected serial endpoint的shared `Terminal`/TTY FileOps，关闭
旧EOF/UTF-8/non-TTY分叉。Owning subsystem不迁移：console拥有selection与node，TTY拥有semantic endpoint，boot I/O
只编排窄capability handoff与prepare-before-publish。

本轮不改变设备号、console registration/selection policy、printk/early console、UART owner、TTY endpoint编号、
boot fd安装、controlling relation、`/dev/tty`、PTY/devpts、runtime console切换、hotplug/unpublish、open flags或
generic VFS/devfs API。若需要复制Terminal状态、暴露TTY registry/endpoint、让console取得TTY协议所有权、引入
runtime rebinding或修改generic opened-description activation，本小迭代停止并升级RFC。

## Change and Acceptance

- TTY boot prepare保留selected endpoint并形成console可消费的窄open capability；console以该capability准备5:1
  node，全部fallible prepare完成后仍按console -> TTY顺序单向publish；
- `/dev/console`每次open构造独立opened description，但共享selected `Terminal`、wake/progress与真实TTY FileOps；
- public `tty-test`核对`console`与boot fd/`ttyS0`交叉termios、winsize、writability、nonblocking empty read、
  binary output，并证明打开console不会使`/dev/tty`获得controlling relation；
- acceptance为RV64/LA64 app与release kernel build、RV64 SMP8 KUnit及完整`TTYTEST` wrapper、格式与docs验证；
  LA64 runtime若未执行必须明确标记Not Run。

## Contract Impact / Cutover

| Contract ID | 变化 | 先前effective baseline | Target refinement |
| --- | --- | --- | --- |
| `TTY-ENDPOINT-001` | Refine | console持selection并独立发布5:1；boot fd和`ttyS<N>`共享Terminal，但`/dev/console`仍使用anonymous console语义 | console继续拥有selection/node；TTY提供selected endpoint窄open capability；`/dev/console` open与boot fd/对应`ttyS<N>`共享Terminal/FileOps，不建立controlling relation；本checkpoint已生效 |
| `BOOT-PROTOCOL-001` | Refine | TTY按selection准备boot三fd，boot coordinator按console -> TTY publish | TTY同时准备selected Terminal open capability，console据此完成node prepare；publication顺序与`InitStdio` handoff保持不变；本checkpoint已生效 |

`TTY-PORT-001`、`TTY-TERM-001`、`TTY-INPUT-001`、`TTY-OUTPUT-001`、TTY relation/job-control、PTY/devpts、
device-number、generic devfs/VFS contracts是受保护依赖，不发生其它contract delta。

## Validation

- RV64与LA64分别通过`just app build --arch <arch> tty-test`；RV64与LA64 SMP8 release kernel分别通过
  repository `just build` preset。两架构kernel build必须串行，因为它们共享`build/generated/kernel.lds`；一次
  误并行的RV64 link明确读到LA64 linker script并失败，串行重跑后通过，不作为源码失败；
- fresh RV64 SMP8完整TTY wrapper通过628/628 KUnit、`TTYTEST:SUMMARY:PASS:52`、新增
  `console-shared-terminal`/`console-binary-write`、BusyBox vi/ash、host binary/ONLCR/drain byte oracle及orderly
  PowerOff；同一wrapper默认SMP1 run也通过相同628/628与52/52；
- `just fmt kernel --check`、`just fmt tty-test --check`、`git diff --check`与`mdbook build docs`通过；
- LA64 runtime Not Run，不能从双架构build或RV64 runtime外推。
- latest-byte独立review为Apollyon 0 / Keter 0；唯一Euclid是current contract仍把LA64 compile写成Not Run，
  已按上述实际build证据修正，复审确认其余owner/lifecycle/ABI与Architecture Friction均无阻断项。

## Remaining Risk / Links

- runtime console switching、virtual console和多selected Terminal策略仍不在当前模型内；本轮只闭合boot-selected
  physical serial console的打开语义。
- Current contracts：[TTY data plane](../../contracts/tty/data-plane.md)、
  [Boot Protocol](../../contracts/task/boot-protocol.md)。历史TTY RFC和transaction只作背景材料，不作为本次
  实施或扩展授权来源，也不在本轮修改。
