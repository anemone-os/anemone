# DWMAC RFC Tracking Issues

**状态：** Active
**最后更新：** 2026-08-12
**父 RFC：** [RFC-20260811-dwmac](./index.md)

这些问题仍影响 Gate、停止边界或 acceptance；它们不是普通 TODO。解决后的 target 语义必须折回 RFC 正文，
不要在此页复制第二份 target。

## ISSUE-001 — 2K1000 Route A external handoff

**等级：** Open / Gate 2 blocker
**状态：** Open
**证据：** 当前 Ethernet node 没有 clock/reset resource；Linux 允许资源缺失，但 SoC 仍有 GMAC clock divider，
且历史 JH7110 bring-up 曾因 register-zero 暴露 external clock/reset owner 问题。
**影响：** cold boot/warm reboot/不同 bootloader 使用历史下，DWMAC register、DMA reset、MDIO 和 GMAC1 pinmux
可能不可达；不能进入 production attach。
**修复位置：** Gate 2 Route A implementation and bring-up；失败回 RFC review 决定 Route B，不写 raw LoongArch fallback。

## ISSUE-002 — PHY ID and RGMII reset-effect

**等级：** Open / Gate 2 blocker
**状态：** Open
**证据：** DTB 只有 `phy-handle`/MDIO address，没有实际 PHY compatible、vendor fixup 或 PHY reset GPIO；GMAC1
使用 `rgmii-id`，soft reset 可能清除 internal delay。
**影响：** 不能在未识别 PHY/fixup owner 前执行 production soft reset，也不能伪造 link-up。
**修复位置：** per-node P1 characterization；target/PHY owner变化回 RFC review。

## ISSUE-003 — ICU polarity under one-cell DTB

**等级：** Open / Gate 1 and Gate 3 blocker
**状态：** Open
**证据：** current DTB does not carry interrupt flags；Anemone currently writes global `POLARITY=0`；2K1000
manual and Linux 6.6 integration identify GMAC 12/13/14/15 as level-low (`EDGE=0`, `POL=1`)。
**影响：** flow class can be `LevelMaskEoi` while electrical polarity remains wrong；可能造成 idle pending、IRQ
storm 或 lost event。
**修复位置：** concrete Loongson irqchip `IrqSense` table and readback；DWMAC must not write ICU。

## ISSUE-004 — DWMAC1000 normal descriptor capability

**等级：** Open / Gate 2 blocker
**状态：** Open
**证据：** Linux supports normal and alternate descriptors, while `DMA_HW_FEATURE.ENHDESSEL` only reports
alternate capability；actual 2K1000 core behavior still needs bounded normal TX/RX probe。
**影响：** if normal mode fails, R0 cannot silently select enhanced; target renegotiation is required。
**修复位置：** Gate 2 Probe P1；delete probe or absorb only after normal protocol proof。

## ISSUE-005 — Legacy 32-bit DMA backing

**等级：** Open / Gate 2 blocker
**状态：** Open
**证据：** DWMAC1000 base/descriptor address fields are 32-bit；Anemone `dma_alloc()` has no mask parameter；current
board memory is below 4 GiB but allocator is global。
**影响：** any high address must fail before publication；DMA32/bounce/IOMMU is outside this RFC。
**修复位置：** checked half-open range admission and warning; high address remains fail-closed/follow-up。

## ISSUE-006 — Request expectation shared API implementation

**等级：** Open / Gate 1 implementation blocker
**状态：** Closed / Gate 1 software implementation
**证据：** current `request_irq` has no expected type；target adds `Option<IrqSense>` to public kernel-internal
request and crate-local named-resource request。
**影响：** implementation must validate before mapping/descriptor/unmask, preserve `None` callers, and not create a
second controller type truth；`IRQ-FLOW-001` remains a target delta until final closure review。
**修复位置：** Gate 1 IRQ API implementation/source/KUnit；not a userspace ABI change。

**Resolution (2026-08-12):** `request_irq` 与 named-resource `request_irq_selected` 现在都接受
`Option<IrqSense>`；prepare -> sense validation -> commit/unmask 顺序在 mapping/descriptor/unmask 前 fail
closed。`None` callers 保持既有行为，mismatch KUnit 验证不会留下 mapping 或 unmask，且 mismatch log 记录
真实 hwirq/expected/actual。R1 的 variant-local Driver owner 修订不改变该 API 的 kernel-internal scope；
`IRQ-FLOW-001` 仍保持 Not Cut Over，待 Gate 4。
