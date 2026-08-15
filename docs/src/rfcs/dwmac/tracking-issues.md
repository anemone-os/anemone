# DWMAC RFC Tracking Issues

**状态：** Closed
**最后更新：** 2026-08-15
**父 RFC：** [RFC-20260811-dwmac](./index.md)

本页保留R7 closure时各tracking issue的最终处置；effective语义已折回RFC正文与current contracts，本页不再
承担active状态或第二份target truth。

## ISSUE-001 — 2K1000 Route A external handoff

**等级：** Closed / Gate 3-final validation
**状态：** Closed by R7 runtime acceptance
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

**Final resolution (2026-08-15):** 后续Gate 3调试跨越多次独立启动并最终由用户确认2K1000网络正常可用；
R7关闭Route A当前板级target。其它bootloader或未来clock/reset provider变化不从本结论外推。

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
**证据：** current DTB does not carry interrupt flags；Gate 1最初按manual/Linux对照将GMAC配置为active-low。
后续production RX明确出现CSR5 `RI/NIS`而ICU没有pending；将source 12--15改为`LevelHigh`（`EDGE=0`、
`POL=0`）后handler进入并恢复ARP/ICMP与外部网络。这一动态证据推翻了早期仅凭readback完成的polarity判断。
**影响：** flow class can be `LevelMaskEoi` while electrical polarity remains wrong；可能造成 idle pending、IRQ
storm 或 lost event。
**Resolution (2026-08-15, corrected by R7):** concrete Loongson irqchip `IrqSense` table 仍是唯一source truth，
但GMAC 12--15的effective entry为`LevelHigh`。DWMAC没有写ICU；request expectation与table一致，动态
handler/traffic证据而非早期静态readback关闭本issue。

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
`IRQ-FLOW-001` 已由 Gate 4 的 `DWMAC-IRQ-CUTOVER` refine。

## ISSUE-007 — Gate 3 IRQ and production-traffic evidence

**等级：** Closed / Gate 3 validation
**状态：** Closed by R7 hardware acceptance
**证据：** Gate 3在同一次platform probe中原位采用Gate 2 owner并首次以
`Some(IrqSense::LevelHigh)` request named `macirq`。用户提供的网关抓包证明ARP reply返回目标MAC；修正
polarity和RX descriptor路径后，用户确认2K1000网络正常可用。owner-local KUnit、LA64/RV64 build与source
review通过；Gate 3专用ICU trace已按退出条件删除。
**Resolution (2026-08-15):** `DWMAC-IRQ-CUTOVER`与`DWMAC-FINAL-CUTOVER`在Gate 4生效。VisionFive 2
本轮hardware复跑明确Not Run，沿用既有hardware acceptance且不宣称新pass。
