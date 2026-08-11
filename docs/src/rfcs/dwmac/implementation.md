# DWMAC RFC 实施路线

**状态：** Accepted / Gate 1 Active
**最后更新：** 2026-08-11
**父 RFC：** [RFC-20260811-dwmac](./index.md)
**当前修订：** R0

本文保存本 RFC 的实现 Gate、probe、验证和停止边界；不冻结逐文件 write set，也不授权未审查的后续
Gate。事实调查属于 RFC 正文和 backgrounds，不单独占 Gate；Gate 1--3 必须各自产生可审查的实现，
source/hardware regression 是对应实现 Gate 的退出验证。只有 Gate 4 是不新增实现能力的最终闭合审查。

## 全局 Implementation Boundary

- **Target / non-goals：** 见 [RFC 正文](./index.md)；DTB 保持不变，DWMAC1000 R0 仅 normal descriptor、32-bit DMA、CSR5 W1C 和 boot-time PHY P1。
- **Owner / handoff / failure / cleanup：** concrete DWMAC backend owns MAC/DMA/rings/device cause；Loongson irqchip owns `IrqSense`/`EDGE/POL`/flow；generic IRQ core owns dispatch；Route A firmware owns external clock/reset/pinctrl；pre-publication failure cleans node-local state。
- **Protected ABI / contract / acceptance：** existing JH7110 visible behavior、network/Stack/attach、current DTB、`eth<N>` success-order semantics and userspace ABI；target contract delta remains Not Cut Over until final Gate 4。
- **Validation claim：** source/KUnit/build can close software protocol; only 2K1000/JH7110 hardware can close core, handoff, PHY, electrical IRQ and traffic claims。
- **Stop conditions：** target/owner/ABI/DTB boundary changes；normal mode fails；Route A/PHY owner unresolved；32-bit address invariant requires allocator/bounce；`IrqSense` expectation becomes a broader generic ABI。

## Gate 1 - DWMAC owner migration and IRQ foundation

**Purpose:** 将现有 JH7110 module 实现为 common + DWMAC4 concrete backend，同时实现 2K1000 irqchip
`IrqSense` 表和 optional request expectation。该 Gate 只建立 DWMAC/IRQ 的共享前提，不接入 DWMAC1000
node，也不发布新的 netdev 能力。

**Prerequisites:** RFC 正文/backgrounds 已记录当前 DT、Linux 6.6.32、Loongson manual/PMON、IRQ/network
contract 和 JH7110 live-source baseline；尚未关闭的 Route A、PHY、DMA 和 normal-mode 硬件问题留给 Gate 2。

**Protected Boundary:** DTB 不变；DWMAC 不写 ICU；`IrqSense` table 是 concrete irqchip owner；JH7110
DWMAC4 register/descriptor/clock/reset/PHY behavior、public network ABI 和 current contracts 不变。

**Deliverable:**

- common frame/progression/publication adapter 与 DWMAC4 concrete backend；JH7110-only clock/reset/PHY/DT
  glue 留在 DWMAC4，common 不拥有 concrete register/descriptor truth。
- compatible-driven per-node dispatch 形状，但只启用既有 JH7110 DWMAC4 match，不提供 DWMAC1000 fallback。
- 2K1000 `IrqSense` table：12/13/14/15 `LevelLow`，44..48 edge/pulse；表同时决定 `EDGE/POL` 和
  controller flow，reserved/GPIO/MSI 不伪造成已支持能力。
- `request_irq(expected: Option<IrqSense>)` 与 `request_irq_selected` named-resource path；`None` 保持现有
  caller 行为，`Some` mismatch 在 mapping/descriptor/unmask 前失败且不写 controller。

**Validation:** kernel build、owner-local KUnit、compatible/module source audit、IRQ `None`/match/mismatch、
2K1000 masked-source `EDGE/POL` readback，以及 VisionFive 2 双节点 DWMAC4 descriptor/PHY/IRQ/frame path/
shutdown regression。JH7110 regression 是本实现 Gate 的退出验证，不是独立 Gate。

**Cutover:** None；owner migration 对 existing visible semantics 保持中性，IRQ target delta 保持 Not Cut
Over 到 Gate 4。

**Stop / Exit:** migration 需要改变 JH7110 visible behavior、public API、IRQ/PHY semantics 或建立 shared raw
register abstraction；`IrqSense` 不能由 concrete irqchip 单一拥有；expectation 需要变成 caller-owned
configuration；任一 JH7110 hardware regression 未解释。停止并回 RFC review。

## Gate 2 - DWMAC1000 backend and bounded bring-up

**Purpose:** 实现 DWMAC1000 concrete backend、Route A admission 和 per-node PHY P1 transaction；先以不
publication 的 bounded hardware slice 验证寄存器、descriptor、DMA、MDIO 和 device-cause 协议，再把
probe 代码删除或吸收到 production backend。

**Prerequisites:** Gate 1 实现和退出验证关闭；RFC 中 live MAC、memory/DMA、IRQ、Route A、PHY ID/RGMII
delay 和 normal-mode facts 足以支撑实现。缺失事实是本 Gate 的启动/停止条件，不是新的事实 Gate。

**Protected Boundary:** DWMAC1000 backend 不改变 DWMAC4；不改变 DTB；不注册 active netdev、Stack
membership 或 `eth<N>`；probe 不形成长期 public API 或第二条 production path。

**Deliverable:**

- `snps,dwmac-3.70a` compatible/version/capability admission；DWMAC internal DMA SW reset 和 timeout diagnostics。
- `ATDS=0` normal descriptor/16-byte stride ring；`ENHDESSEL` diagnostic only；normal OWN/length/end-ring tests。
- full checked 32-bit DMA range for ring/base/frame/next；越界 before IRQ/publication failure。
- Route A capability/readability handoff check；MDIO divider/read/write；按已确认 PHY ID 实现 P1 reset/fixup/
  autonegotiation/link snapshot，不能闭合 `rgmii-id` reset effect 时禁止 production reset。
- `Some(IrqSense::LevelLow)` request wiring；CSR5 W1C mask/read/write；RX/TX bounded loopback or equivalent
  hardware event without publication。

**Validation:** owner-local KUnit/source tests、kernel build、cold/warm/bootloader-used bounded hardware bring-up；
记录 version/capability、DMA reset deadline、DMA addresses、MDIO/PHY/RGMII state、CSR5 before/after、
`EDGE/POL`/pending readback 和 failure logs；同时重跑 Gate 1 的 DWMAC4 regression。

**Cutover:** `DWMAC1000-CUTOVER` 仍 Not Cut Over；不写 current contracts。

**Stop / Exit:** Route A/PHY owner 不能闭合、normal descriptor failure、unexpected register family、DMA high
address、W1C readback failure、IRQ sense mismatch、reset/MDIO timeout，或需要 enhanced/DMA32/bounce/DTB
change。必须 target renegotiation 或 Follow-up RFC；不能把 probe 留成并列 backend。

### Probe P1 - Normal descriptor and device-cause slice

**Hypothesis:** 2K1000 DWMAC 3.70a accepts legacy normal descriptor with `ATDS=0`, and CSR5 low cause bits are W1C as Linux/PMON indicate。

**Protected Boundary:** 不 publish netdev、不修改 current contract、不沉淀 generic descriptor API、失败删除 probe path。

**Non-goals:** enhanced/extended/PTP、runtime PHY/link、traffic performance、high-address allocation。

**Validation:** one bounded TX and RX OWN/status transition；CSR5 read -> legal W1C write -> CSR5 readback；IRQ mask/pending/unmask ordering；two-port source mapping if hardware permits。

**Failure Signals:** OWN never clears、length/status layout mismatch、CSR5 cause persists、unchanged-status immediate repeat、`EDGE/POL` mismatch、DMA address truncation。

**Write-back:** evidence folds into Gate 2 implementation; failure stops Gate 2 and records target renegotiation.
Probe code is deleted or absorbed into the production backend before Gate 2 exits。

**Exit:** normal probe proven and absorbed into backend, or Not Cut Over with code disposition。

## Gate 3 - Per-node production attach

**Purpose:** 将 Gate 2 的 DWMAC1000 backend 接入 existing FrameProvider/worker/publication/attach path，完成
不固定实例数的 per-node production implementation。单端口 bring-up 只是该 Gate 内的第一段验证，不形成
独立 Gate 或 single-port-only production target。

**Prerequisites:** Gate 2 backend/bounded bring-up closure；Route A、PHY、MAC、IRQ、DMA 和 normal descriptor
evidence 已关闭；Gate 1 DWMAC4 regression remains green。

**Protected Boundary:** failure node does not consume `eth<N>`；其它 matching node 独立继续 probe；DTB、
existing network ABI、global Stack、attach owner 和 DWMAC4 behavior 不变。

**Deliverable:**

- 每个 matching node 独立完成 PHY transaction、rings、IRQ、FrameProvider、worker、publication 和 attach；
  使用 live DT MAC 与 `Some(IrqSense::LevelLow)`。
- active logical identity 只在成功 publication/attach 时按成功顺序连续分配；不按 MMIO、IRQ、alias 或
  candidate ordinal 固定 `eth<N>`。
- 同一 per-node path 支持两个当前 2K1000 node；不增加 GMAC0/GMAC1 特判、固定数组或 second-port adapter。
- 任一 node 的 pre-publication failure 清理 node-local owner，不留下 enabled IRQ/DMA/provider orphan，且不
  阻止其它 node 成功。

**Validation:** 先执行单端口 cold/warm boot、link snapshot、RX/TX/abnormal IRQ 和 shutdown，再在同一 Gate
执行双端口 cold/warm boot、concurrent traffic、success-order identity、one-node failure isolation、shutdown/
reboot；确认无 unchanged-status immediate IRQ repeat，并重跑 VisionFive 2 双端口 regression。

**Cutover:** production implementation 完成，但 target contracts 仍保持 Not Cut Over，等待 Gate 4 独立闭合
审查；不得把单端口中间结果描述为 RFC target closure。

**Stop / Exit:** publication-before-admission、incorrect MAC/name、PHY delay/link failure、IRQ storm/loss、
cleanup orphan、second-port 特判，或任一 correctness invariant/owner/handoff/ABI/acceptance/validation claim
变化。停止并进入 Target Renegotiation/Follow-up RFC。

## Gate 4 - Final acceptance and closure review

**Purpose:** 对 Gate 1--3 已完成实现和证据做独立最终审查，确认 RFC target、correctness invariants、
architecture-friction scan、acceptance 和 current-contract delta 全部闭合。本 Gate 不新增功能实现。

**Prerequisites:** Gate 1--3 均以各自 source/build/KUnit/hardware evidence 退出；没有 probe、single-port-only
path、未解释 regression 或 blocking tracking issue。

**Protected Boundary:** 不以 closure review 修改 target、owner、handoff、failure、cleanup、ABI、acceptance
或 validation strength；不在文档中把未运行的硬件证据写成 Passed。

**Deliverable:** 独立 source/owner/lifecycle review、最终验证矩阵、tracking issue disposition、Architecture
Friction Scan，以及与真实实现一致的 current-contract 更新和 RFC closure 记录。

**Validation:** 复核 Gate 1--3 的命令与 hardware logs；抽查 compatible dispatch、normal descriptor、32-bit
DMA、Route A/PHY transaction、IRQ type/W1C、per-node failure isolation、双端口 traffic/shutdown/reboot 和
JH7110 regression。发现实现缺口必须退回对应实现 Gate，不能在 Gate 4 内补代码后直接 closure。

**Cutover:** independent review 通过后，原子执行 `DWMAC-IRQ-CUTOVER`、`DWMAC1000-CUTOVER` 和
`DWMAC-FINAL-CUTOVER`，更新真实受影响的 current contracts 并关闭 RFC；否则保持 Not Cut Over。

**Stop / Exit:** 任一 correctness invariant、owner/handoff、ABI、acceptance 或 validation claim 未闭合或发生
变化；停止并进入 Target Renegotiation/Follow-up RFC，不批准 closure。

## Target Renegotiation

若 normal-only、Route A、32-bit DMA、PHY P1 或 `IrqSense` evidence 不能成立，必须记录真实失败证据、已完成 slice、代码处置、受影响 target/contract/acceptance 和 options。可选路线是 Route Correction、Accepted Reduced Target、Follow-up RFC 或 Not Cut Over；agent 不能自行批准 reduced target。
