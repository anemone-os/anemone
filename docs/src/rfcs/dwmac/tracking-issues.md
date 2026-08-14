# DWMAC RFC Tracking Issues

**状态：** Active
**最后更新：** 2026-08-14
**父 RFC：** [RFC-20260811-dwmac](./index.md)

这些问题仍影响 Gate、停止边界或 acceptance；它们不是普通 TODO。解决后的 target 语义必须折回 RFC 正文，
不要在此页复制第二份 target。

## ISSUE-001 — 2K1000 Route A external handoff

**等级：** Open / Gate 3-final validation
**状态：** Gate 2 condition closed; repeatability matrix open
**证据：** 当前 Ethernet node 没有 clock/reset resource；Linux 允许资源缺失，但 SoC 仍有 GMAC clock divider。
`etc/log-board-la-28-exchanged.log` 的八次启动中，`ethernet@40040000` 三次 DMA SWR 成功、五次保持
`SWR=1`直到超时；`ethernet@40050000`八次成功。SWR发生在PHY link characterization前，因此无链路只能说明
外部RGMII clock/handoff相关，不能把reset timeout当作预期link-down结果。
**影响：** cold boot/warm reboot/不同 bootloader 使用历史下，DWMAC register、DMA reset、MDIO 和 GMAC1 pinmux
仍需重复性证明；这不再阻塞 Gate 2，但在 production attach 前仍不能宣称 Route A 已完成全矩阵。
**修复位置：** Gate 3/final acceptance 的 cold/warm/bootloader-used 矩阵；失败回 RFC review 决定 Route B，
不写 raw LoongArch fallback。

**Resolution (2026-08-14):** `etc/log-board-la-new-35.log` 连续三次有线 node 均通过 DMA reset、PHY、
Gate 2 polling 和 quiescence，关闭 Gate 2 blocker；矩阵验证保留为后续 Gate 3/final issue。

## ISSUE-002 — PHY ID and RGMII reset-effect

**等级：** Closed / Gate 2 hardware validation
**状态：** Closed
**证据：** DTB 只有 `phy-handle`/MDIO address，没有实际 PHY compatible、vendor fixup 或 PHY reset GPIO；GMAC1
使用 `rgmii-id`，soft reset 可能清除 internal delay。八次换线日志中，只要DMA reset成功，P1前有线PHY多次为
`BMSR=0x7969/0x796d`，但P1 reset/restart后在旧1秒MDIO deadline内均为`0x7949`。随后11次bounded sample全部
表现为TX OWN清除、RX OWN保持、RI/TI未出现，说明driver在PHY link/speed重新解析前启动了MAC loopback。
**影响：** Gate 2 已不再依赖未证实的 PHY reset；后续 production link lifecycle 仍受 Gate 3 约束。
**修复位置：** per-node P1 characterization 使用 generic Clause 22 resolution 和 Linux YT8511 config-init；
不猜测固定速率或 raw SoC register。

**Resolution (2026-08-14):** `new-35` 三次均记录 YT8511 ID、vendor page readback、resolved 1000/full-duplex
link 和 generic Clause 22 completion；当前实现不执行 soft reset，Gate 2 证据闭合该 issue。

## ISSUE-003 — ICU polarity under one-cell DTB

**等级：** Closed / Gate 1 hardware readback
**状态：** Closed
**证据：** current DTB does not carry interrupt flags；Gate 1 software 已改为由 owner-local `IrqSense` table
program `EDGE/POL`。用户提供的 2K1000 Gate 1 实机 readback 为 `EDGE=0x1f00000000000`、`POL=0xf000`：
前者精确覆盖 source 44..48，后者精确覆盖 source 12..15。2K1000 manual and Linux 6.6 integration identify
GMAC 12/13/14/15 as level-low (`EDGE=0`, `POL=1`)；实机值与 target 一致。
**影响：** flow class can be `LevelMaskEoi` while electrical polarity remains wrong；可能造成 idle pending、IRQ
storm 或 lost event。
**Resolution (2026-08-12):** concrete Loongson irqchip `IrqSense` table 是唯一 source truth，init-time
program/readback assert 与用户实机日志共同关闭本 issue；DWMAC 没有写 ICU。Gate 2的真实CSR5 polling/W1C
和Gate 3的pending/request/handler/unmask sequence仍按各Gate validation执行，不由本静态readback替代。

## ISSUE-004 — DWMAC1000 enhanced descriptor hardware acceptance

**等级：** Closed / Gate 2 hardware validation
**状态：** Closed
**证据：** Linux 6.6 selects enhanced/alternate descriptor ops from `DMA_HW_FEATURE.ENHDESSEL` and uses
`dma_extended_desc` for a 3.70a core. The implementation now requires `ENHDESSEL=true`, sets `ATDS=1`, uses a
32-byte stride, and rejects normal descriptor fallback. A real 2K1000 sample still has to prove enhanced TX/RX
OWN/status transitions and matching descriptor readback.
**影响：** no target ambiguity remains; the enhanced-only path is now hardware-proven for Gate 2.
**修复位置：** Gate 3 must adopt the same retained enhanced/extended owner; normal descriptor fallback remains
unsupported.

**Resolution (2026-08-14):** `new-35` repeated the enhanced 32-byte/`ATDS=1` path three times with TX/RX OWN
transitions, matching descriptor readback, 68-byte payload and quiescence; no normal fallback was used.

## ISSUE-005 — Legacy 32-bit DMA backing

**等级：** Closed / Gate 2 admission validation
**状态：** Closed
**证据：** DWMAC1000 base/descriptor address fields are 32-bit；Anemone `dma_alloc()` has no mask parameter；current
board memory is below 4 GiB but allocator is global。
**影响：** any high address remains unsupported and must fail before publication；DMA32/bounce/IOMMU is outside this RFC。
**修复位置：** checked half-open range admission；future high-address support requires a separate RFC。

**Resolution (2026-08-14):** all three `new-35` runs used checked below-4 GiB backing, and all three KUnit runners
passed the high/overflowing-range rejection matrix. The accepted Gate 2 support range is therefore fail-closed and
closed without adding a guessed allocator mask.

## ISSUE-006 — Request expectation shared API implementation

**等级：** Closed / Gate 1 software implementation
**状态：** Closed / Gate 1 software implementation
**证据：** current `request_irq` has no expected type；target adds `Option<IrqSense>` to public kernel-internal
request and crate-local named-resource request。
**影响：** implementation must validate before mapping/descriptor/unmask, preserve `None` callers, and not create a
second controller type truth；`IRQ-FLOW-001` remains a target delta until final closure review。
**修复位置：** Gate 1 IRQ API implementation/source/KUnit；not a userspace ABI change。

**Resolution (2026-08-12):** `request_irq` 与 named-resource `request_irq_selected` 现在都接受
`Option<IrqSense>`；prepare -> sense validation -> commit/unmask 顺序在 mapping/descriptor/unmask 前 fail
closed。`None` callers 保持既有行为，mismatch KUnit 验证不会留下 mapping 或 unmask，且 mismatch log 记录
真实 hwirq/expected/actual。R2 保持 R1 variant-local Driver owner，不改变该 API 的 kernel-internal scope；
`IRQ-FLOW-001` 仍保持 Not Cut Over，待 Gate 4。

## ISSUE-007 — Gate 3 IRQ and production-traffic evidence

**等级：** Open / Gate 3 validation
**状态：** Implementation present; hardware acceptance not run
**证据：** Gate 3 代码已在同一次 platform probe 中从 Gate 2 retained owner 创建唯一 IRQ context，使用
`Some(IrqSense::LevelLow)` 完成 named `macirq` 的首次 request，并通过既有 publication/attach path；owner-local
KUnit、2K1000/LoongArch build 和 VisionFive 2/RiscV build 均通过。当前没有新镜像的 2K1000 serial evidence，
也没有 irqchip pending-before-unmask trace、handler/W1C/level-flow、single/dual-port traffic、failure isolation、
shutdown/reboot 或 cold/warm/bootloader-used 矩阵。
**影响：** 不能把 Gate 3 implementation/build evidence 写成 IRQ、traffic、lifecycle 或 Route A final acceptance；
`DWMAC-IRQ-CUTOVER`、`DWMAC-FINAL-CUTOVER` 和 current contracts 保持 Not Cut Over。
**修复位置：** Gate 3 实机验证与 R2 延期的 VisionFive 2 DWMAC4 regression；若 pending observation 需要扩大
generic IRQ/controller API，立即回 IRQ owner review，不能添加第二份 pending truth。
