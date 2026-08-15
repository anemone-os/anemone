# DWMAC 多后端与 2K1000 目标与不变量

**状态：** Accepted
**最后更新：** 2026-08-14
**父 RFC：** [RFC-20260811-dwmac](./index.md)
**适用修订：** R7

本文只定义本 RFC 的 target/proof obligations。当前 effective rule 仍以 `docs/src/contracts/` 为准；
实现类型、helper、文件布局和内部算法不由本文冻结。

## 规则分类

- **Correctness invariant：** owner、并发、生命周期、cleanup、内存安全、IRQ ordering 和 ABI 诚实性，不能以工程妥协降低。
- **Target guarantee：** R7保持DWMAC1000 capability-admitted Linux enhanced/extended mode、32-bit DMA、boot-time PHY和长期owner；per-port `no-map` reserved region通过strong-noncache映射提供最终DMA backing，GMAC 12--15使用实机证明的`LevelHigh`；Gate 1--4已闭合。
- **Implementation preference：** `IrqSense` 的具体 Rust 形状、ring helper、backend module layout 和 log wording。

## Target Invariants

### TARGET-001 — Per-node concrete ownership

**规则：** 每个 matching Ethernet node 独立拥有 MMIO window、DWMAC backend state、descriptor ring、DMA backing、PHY transaction、worker 和 failure state；`dwmac4`/`dwmac1000` variant module 各自拥有并注册对应 `Driver` 与 match table；`net::dwmac` common layer 不持有 concrete register/descriptor truth，也不制造第二份 variant registration state。DWMAC1000 owner 在 Gate 2 polling前创建，通过后由bound platform device长期保留；Gate 3只能原位采用该owner并首次增加IRQ context。

**Owner：** concrete DWMAC node provider。

**违反表现：** 两个 node 共享 ring/PHY/register state，固定 GMAC ordinal 分支，一个 node 的失败/cleanup 改变另一个 node 的 publication，Gate 2提前注册或留下不可达IRQ context，或Gate 3重建hardware owner并形成并列state。

**Proof：** Gate 1 implementation/source audit、Gate 3 JH7110 dual-node regression and independent 2K1000 port evidence。

### TARGET-002 — Compatible-only backend admission

**规则：** backend 只能由 DT compatible match 选择；MMIO base、IRQ number、node ordinal、aliases 和 board name 不得选择 family 或实例。

**Owner：** generic compatible dispatch + concrete backend admission。

**违反表现：** `0x40040000` 被当作 GMAC0 特判、source 12 被当作固定 port identity，或不匹配 compatible 的 node 进入 backend。

**Proof：** Gate 1 matching matrix and dispatch source audit、negative compatible test。

### TARGET-003 — Enhanced descriptor protocol

**规则：** DWMAC1000 R0 只有在 `DMA_HW_FEATURE.ENHDESSEL=true` 时 admission，使用 Linux enhanced/alternate 位表与
`dma_extended_desc` 的 32-byte stride，并置 `ATDS=1`；软件发布 descriptor 时清零extended status/timestamp
words，当前路径不消费硬件后续RX writeback，也不启用对应 runtime offload。normal descriptor 不属于支持范围，
也不存在 fallback。Gate 2按Kconfig合法范围分配
Gate 3可原位采用的最终ring/backing，只借用slot 0做bounded proof；通过且quiesced后在同一backing上恢复完整
RX device ownership、idle TX和末项EOR。

**Owner：** DWMAC1000 descriptor backend。

**违反表现：** 使用JH7110 DWMAC4 descriptor word layout、normal/enhanced位混写、`ATDS`与stride不匹配、EOR不只位于最后一项、`ENHDESSEL=false`仍启动、或Gate 3必须替换Gate 2的rings/backing。

**Proof：** Gate 2 backend implementation/register/KUnit/loopback probe；enhanced/extended probe 失败即停止并 target renegotiation。

### TARGET-004 — 32-bit DMA address admission

**规则：** 每个 DWMAC1000 descriptor base、ring、frame backing、chain next 和 derived offset 都满足完整半开区间 `[start,end) < 0x1_0000_0000`；任何 checked arithmetic overflow 或越界使 node fail before IRQ/publication/`eth<N>`。

**Owner：** DWMAC1000 DMA backend；allocator 仍拥有物理 frame allocation。

**违反表现：** 只检查 start、截断为 `u32` 后继续、或高地址 node 已 publication。

**Proof：** focused checked-range tests、Gate 2 backend source audit/runtime log/readback；R0 不引入 DMA32/bounce。

### TARGET-005 — Single `IrqSense` source truth

**规则：** 2K1000 irqchip 的 owner-local table 是 source electrical type、`INTEDGE/INTPOL` programming 和 controller flow 的唯一真相。实机 production IRQ 证明 GMAC 12/13/14/15 为 `LevelHigh`；44..48 为 edge/pulse。不能同时保留 range-derived trigger truth 和独立 polarity mask。

**Owner：** concrete Loongson 2K1000 irqchip。

**违反表现：** DWMAC 写 ICU、request caller 覆盖 table、level trigger 与 polarity 混淆，或 table/readback/flow 不一致。

**Proof：** Gate 1 irqchip implementation/table audit、ICU `EDGE/POL` readback、Gate 3 two-port runtime IRQ probe。

### TARGET-006 — Optional request expectation is assertion only

**规则：** `request_irq` 与 `request_irq_selected` 的 `Option<IrqSense>` 只比较 irqchip actual sense；比较必须发生在 domain mapping、descriptor publication 和 first unmask 前。mismatch 不得留下 mapping、descriptor 或 enabled source。

**Owner：** IRQ request owner；actual sense 仍由 irqchip table拥有。

**违反表现：** expectation 写 controller、改变 flow、存储第二份类型 truth，或 mismatch 后 source 已 unmask。

**Proof：** `None`/matching/mismatching KUnit、named `macirq` selection test、source audit。

### TARGET-007 — Device cause before level completion

**规则：** DWMAC1000 owner对每个raw CSR5 sample先取完整legal W1C窗口并向CSR5写`1`清除，再独立按admitted/enabled mask分类RI/TI与异常evidence；disabled legal cause也必须清除。Gate 2在CSR7=0时轮询并完成该事务；Gate 3 handler必须在level flow的`eoi/unmask`前沿用相同raw-clear/evidence分离并发布durable recheck/wake。ICU transaction不代替CSR5 clear。

**Owner：** concrete DWMAC device handler。

**违反表现：** 先unmask后W1C、只清summary或enabled subset、向CSR5写`u32::MAX`、把晚到事件误判为上一事件未清，或因旧cause未清形成unchanged-status immediate repeat。

**Proof：** Gate 2 W1C mask/classification KUnit和RX/TX/abnormal polling probe；Gate 3 pending readback before unmask与IRQ flow trace。

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

**规则：** 只有所有 node-local admission、DMA、IRQ、PHY、provider 和 worker prerequisites 成功后才 publication；成功 active reservation 按 publication/attach success order 连续分配 `eth<N>`，失败 candidate 不占号。Gate 2 的 platform bind 只提交通过polling且已quiesce的长期hardware owner，不构成IRQ或network publication。Gate 2 failure只有在TX/RX process-state quiescence已证明后才释放资源；未证明时必须fail-stop。Gate 3 IRQ commit后的owner/context lifecycle由同一对象继续承担。

**Owner：** device/net publication + attach authority；node provider owns local cleanup attempt。

**违反表现：** failed node consumes name、Gate 2 bound node注册IRQ或产生netdev、disabled owner仍使能CSR7/MAC/DMA、未证明quiescence即释放backing、Gate 3 IRQ commit后context不可达、publication observes incomplete link/MAC/DMA state，或cleanup依赖不存在的runtime removal。

**Proof：** failure injection、success-order multi-node implementation test、Gate 3 shutdown/reboot evidence；Gate 4 只复核 closure evidence。

## 状态所有权与生命周期

1. `IrqSense` table owns electrical/controller configuration；`request_irq` expectation is a one-shot caller assertion，不是缓存的第二份 state。
2. DWMAC backend owns descriptor ownership transitions and device-cause clear；worker consumes recheck hint and revalidates ring state，不反向拥有 CSR5 truth。
3. Gate 2在CSR7=0且不调用`request_irq`的前提下运行bounded polling；每个raw legal CSR5 sample由同一owner分类、W1C并立即回读。只有characterization passed且TX/RX process state已quiescent时才把owner写入device的一次性`drv_state`并bind。
4. Gate 2 failure只有在quiescence已证明时才能返回并释放DMA backing；不能证明时当前没有安全retention carrier，必须fail-stop，不能由`Drop`隐式回收。
5. Gate 3只允许在同一次platform `probe()`中通过private continuation，让同一owner由`Characterized`/disabled首次增加IRQ context并转为production attach；IRQ descriptor必须在commit/unmask前取得同一owner的strong reference。不得依赖reprobe、重映射MMIO、重建ring、替换一次性`drv_state`、复制CSR5 truth，或采用旁路全局registry形成第二份状态源。
6. `local-mac-address`、PHY snapshot、DMA address admission、characterization result 和 logical `eth<N>` identity 属于不同 fact domains；不能用 platform bind 或一个 domain 的 fallback 伪造另一个 domain 的 success。

## RFC-local Proof Obligations

- DWMAC4 extraction preserves existing JH7110 visible behavior before DWMAC1000 semantic work begins。
- An enhanced/extended descriptor probe must perform at least one bounded TX and RX OWN/length/status transition without netdev publication。
- Gate 2 polling must prove real RI/TI、descriptor/payload completion and immediate CSR5 legal-W1C readback while CSR7 remains zero；Gate 3 separately proves `mask -> handler/W1C -> eoi -> unmask`。
- Any Loongson pending trace belongs only to Gate 3/final IRQ diagnosis；it must not become a DWMAC-callable controller API or DWMAC-owned state。
- Gate 2 ownership tests must distinguish quiesced pass retained、quiesced failure released and quiesce-timeout fail-stop；Gate 3 adoption tests must prove the first/only IRQ request and no MMIO/ring reconstruction。
- Route A and PHY P1 probes must define failure signal, write-back and exit before any probe code is retained in production。

## 禁止退化项

- 不把 JH7110 DWMAC4 descriptor/register helpers作为 DWMAC1000 enhanced/extended backend 的实现。
- 不把 `dma-mask` property、coherency fence 或当前 low-memory DTS 当作未经检查的 DMA proof。
- 不让 DWMAC driver 通过 broad IRQ API 写 ICU polarity/flow。
- 不以成功编译、QEMU non-empty register 或单端口结果推导双端口/实机 correctness。
- 不在本 RFC 内偷偷加入 normal fallback、DMA32/bounce、clock/reset provider 或 generic PHY framework。
