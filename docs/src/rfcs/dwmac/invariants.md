# DWMAC 多后端与 2K1000 目标与不变量

**状态：** Accepted
**最后更新：** 2026-08-11
**父 RFC：** [RFC-20260811-dwmac](./index.md)
**适用修订：** R0

本文只定义本 RFC 的 target/proof obligations。当前 effective rule 仍以 `docs/src/contracts/` 为准；
实现类型、helper、文件布局和内部算法不由本文冻结。

## 规则分类

- **Correctness invariant：** owner、并发、生命周期、cleanup、内存安全、IRQ ordering 和 ABI 诚实性，不能以工程妥协降低。
- **Target guarantee：** R0 承诺的 DWMAC4 migration、DWMAC1000 normal mode、32-bit DMA 和 boot-time PHY 能力；只能由 RFC review 修订。
- **Implementation preference：** `IrqSense` 的具体 Rust 形状、ring helper、backend module layout 和 log wording。

## Target Invariants

### TARGET-001 — Per-node concrete ownership

**规则：** 每个 matching Ethernet node 独立拥有 MMIO window、DWMAC backend state、descriptor ring、DMA backing、IRQ context、PHY transaction、worker 和 failure state；common layer 不持有 concrete register/descriptor truth。

**Owner：** concrete DWMAC node provider。

**违反表现：** 两个 node 共享 ring/PHY/register state，固定 GMAC ordinal 分支，或一个 node 的失败/cleanup 改变另一个 node 的 publication。

**Proof：** Gate 1 implementation/source audit and JH7110 dual-node regression、Gate 3 independent 2K1000 port evidence。

### TARGET-002 — Compatible-only backend admission

**规则：** backend 只能由 DT compatible match 选择；MMIO base、IRQ number、node ordinal、aliases 和 board name 不得选择 family 或实例。

**Owner：** generic compatible dispatch + concrete backend admission。

**违反表现：** `0x40040000` 被当作 GMAC0 特判、source 12 被当作固定 port identity，或不匹配 compatible 的 node 进入 backend。

**Proof：** Gate 1 matching matrix and dispatch source audit、negative compatible test。

### TARGET-003 — Normal descriptor protocol

**规则：** DWMAC1000 R0 使用 normal descriptor，`ATDS=0`，stride 为 16 bytes；OWN、length、first/last、end-ring、buffer address 和 next address 必须使用 legacy normal 位表。读取 `ENHDESSEL` 只用于 admission/logging，不自动升级 enhanced。

**Owner：** DWMAC1000 descriptor backend。

**违反表现：** 使用 JH7110 DWMAC4 descriptor word layout、normal/enhanced 位混写、stride 不匹配，或 capability 为 enhanced 时静默改模式。

**Proof：** Gate 2 backend implementation/register/KUnit/loopback probe；normal probe 失败即停止并 target renegotiation。

### TARGET-004 — 32-bit DMA address admission

**规则：** 每个 DWMAC1000 descriptor base、ring、frame backing、chain next 和 derived offset 都满足完整半开区间 `[start,end) < 0x1_0000_0000`；任何 checked arithmetic overflow 或越界使 node fail before IRQ/publication/`eth<N>`。

**Owner：** DWMAC1000 DMA backend；allocator 仍拥有物理 frame allocation。

**违反表现：** 只检查 start、截断为 `u32` 后继续、或高地址 node 已 publication。

**Proof：** focused checked-range tests、Gate 2 backend source audit/runtime log/readback；R0 不引入 DMA32/bounce。

### TARGET-005 — Single `IrqSense` source truth

**规则：** 2K1000 irqchip 的 owner-local table 是 source electrical type、`INTEDGE/INTPOL` programming 和 controller flow 的唯一真相。GMAC 12/13/14/15 为 `LevelLow`；44..48 为 edge/pulse。不能同时保留 range-derived trigger truth 和独立 polarity mask。

**Owner：** concrete Loongson 2K1000 irqchip。

**违反表现：** DWMAC 写 ICU、request caller 覆盖 table、`Level` 与 `LevelLow` 混淆，或 table/readback/flow 不一致。

**Proof：** Gate 1 irqchip implementation/table audit、ICU `EDGE/POL` readback、Gate 3 two-port runtime IRQ probe。

### TARGET-006 — Optional request expectation is assertion only

**规则：** `request_irq` 与 `request_irq_selected` 的 `Option<IrqSense>` 只比较 irqchip actual sense；比较必须发生在 domain mapping、descriptor publication 和 first unmask 前。mismatch 不得留下 mapping、descriptor 或 enabled source。

**Owner：** IRQ request owner；actual sense 仍由 irqchip table拥有。

**违反表现：** expectation 写 controller、改变 flow、存储第二份类型 truth，或 mismatch 后 source 已 unmask。

**Proof：** `None`/matching/mismatching KUnit、named `macirq` selection test、source audit。

### TARGET-007 — Device cause before level completion

**规则：** DWMAC handler 在 level flow 的 `eoi/unmask` 前读取 CSR5、取 enabled and legal W1C cause、向 CSR5 写 `1` 清除，并发布 durable recheck/wake。ICU transaction 不代替 CSR5 clear。

**Owner：** concrete DWMAC device handler。

**违反表现：** 先 unmask 后 W1C、只清 summary、向 CSR5 写 `u32::MAX`、或因旧 cause 未清形成 unchanged-status immediate repeat。

**Proof：** W1C mask unit tests、RX/TX/abnormal probe、pending readback before unmask、IRQ flow trace。

### TARGET-008 — Route A external handoff

**规则：** 本 RFC 不新增 clock/reset/pinctrl resource。driver 只能消费 firmware handoff，并在 DWMAC core access、DMA reset、MDIO 和 pin/RGMII characterization 中 fail closed；不得写 raw LoongArch controller register。

**Owner：** firmware/board environment owns external state；DWMAC owns internal DMA reset and core programming。

**违反表现：** register-zero 后 magic write、missing resource 被伪装成 enabled、或不同 cold/warm boot 状态未被记录。

**Proof：** cold/warm/bootloader matrix；handoff failure进入 tracking/review，不进入隐藏 fallback。

### TARGET-009 — PHY transaction integrity

**规则：** per-node PHY P1 transaction 识别 `phy-handle`/MDIO PHY，执行必要 Clause 22 reset/fixup/autonegotiation/link snapshot；`rgmii-id` delay 在 reset 后必须有明确 owner。transaction 失败不发布 node。

**Owner：** per-node PHY transaction；不建立 global PHY registry/runtime link manager。

**违反表现：** 未识别 PHY 就写 vendor registers、soft reset 后丢 delay、伪造 link-up 或跨 node 借用 PHY state。

**Proof：** Gate 2 PHY ID/reset-effect implementation/characterization、Gate 3 single- and two-port independent PHY evidence。

### TARGET-010 — Publication identity and cleanup

**规则：** 只有所有 node-local admission、DMA、IRQ、PHY、provider 和 worker prerequisites 成功后才 publication；成功 active reservation 按 publication/attach success order 连续分配 `eth<N>`，失败 candidate 不占号。所有 pre-publication failure 清理 node-local allocations and disabled resources。

**Owner：** device/net publication + attach authority；node provider owns local cleanup attempt。

**违反表现：** failed node consumes name、IRQ enabled without provider、publication observes incomplete link/MAC/DMA state，或 cleanup 依赖不存在的 runtime removal。

**Proof：** failure injection、success-order multi-node implementation test、Gate 3 shutdown/reboot evidence；Gate 4 只复核 closure evidence。

## 状态所有权与生命周期

1. `IrqSense` table owns electrical/controller configuration；`request_irq` expectation is a one-shot caller assertion，不是缓存的第二份 state。
2. DWMAC backend owns descriptor ownership transitions and device-cause clear；worker consumes recheck hint and revalidates ring state，不反向拥有 CSR5 truth。
3. Node-local state must reach a quiescent disabled state before any failure is returned after MMIO/DMA setup；because current kernel has no general `free_irq`/runtime removal, all failure-prone work precedes first unmask/publication。
4. `local-mac-address`、PHY snapshot、DMA address admission 和 logical `eth<N>` identity 属于不同 fact domains；不能用一个 domain 的 fallback 伪造另一个 domain 的 success。

## RFC-local Proof Obligations

- DWMAC4 extraction preserves existing JH7110 visible behavior before DWMAC1000 semantic work begins。
- A normal descriptor probe must perform at least one bounded TX and RX OWN/length/status transition without netdev publication。
- IRQ probe must distinguish controller pending from CSR5 cause and record the sequence `mask -> read/W1C -> pending clear -> unmask`。
- Route A and PHY P1 probes must define failure signal, write-back and exit before any probe code is retained in production。

## 禁止退化项

- 不把 JH7110 DWMAC4 descriptor/register helpers作为 DWMAC1000 normal backend 的实现。
- 不把 `dma-mask` property、coherency fence 或当前 low-memory DTS 当作未经检查的 DMA proof。
- 不让 DWMAC driver 通过 broad IRQ API 写 ICU polarity/flow。
- 不以成功编译、QEMU non-empty register 或单端口结果推导双端口/实机 correctness。
- 不在本 RFC 内偷偷加入 enhanced fallback、DMA32/bounce、clock/reset provider 或 generic PHY framework。
