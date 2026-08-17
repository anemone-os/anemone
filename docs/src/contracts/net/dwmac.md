# DWMAC Concrete Backend 当前契约

**Contract ID：** `DWMAC-NODE-001` / `DWMAC-DESCRIPTOR-001` / `DWMAC-DMA-ADDR-001` / `DWMAC-CAUSE-001`
**状态：** Active
**Owner：** concrete DWMAC backend per-node owner
**参与领域：** platform discovery / DWMAC4 / DWMAC1000 / IRQ / frame provider
**覆盖范围：** backend dispatch、DWMAC1000 descriptor/DMA/cause、per-node publication failure boundary
**不覆盖：** runtime PHY renegotiation、hotplug/removal、normal descriptor fallback、DMA above 4 GiB、IOMMU/bounce、PTP/TSO/checksum offload或其它DWMAC family
**实现位置：** `anemone-kernel/src/driver/net/dwmac/`、`conf/platforms/2k1000-board.dts`
**依赖：** `IRQ-FLOW-001`、`NET-FRAME-OWN-001`、`NET-FRAME-PROGRESS-001`、`NETDEV-LIFE-001`、`NET-ATTACH-001`
**Pending Successor：** None
**最后核验：** 2026-08-15；`DWMAC1000-CUTOVER` / `DWMAC-IRQ-CUTOVER` / `DWMAC-FINAL-CUTOVER` Effective

## `DWMAC-NODE-001` - Compatible-selected per-node owner

**规范性规则：** 每个enabled Ethernet node只按DT `compatible`进入一个concrete backend，并独立拥有MMIO、
PHY transaction、rings、DMA backing、device cause、IRQ context与frame capability。DWMAC1000在同一次platform
probe中先建立并characterize唯一owner，再原位增加IRQ/publication capability；不得reprobe、重建backing或复制
CSR5 truth。任一node在publication前失败不得消费`eth<N>`或阻止其它candidate继续；active identity只按成功
publication顺序分配。

**违反表现：** 按MMIO/IRQ/ordinal猜backend，两个node共享ring或PHY状态，Gate 2 owner被production owner替换，
失败node占用name，或为第二端口增加特判。

**验证 / Enforcement：** variant-local Driver/match table source audit；owner/ring KUnit；2K1000双node独立probe、
failure isolation与用户production network acceptance。

**最初来源 / 当前来源：** [DWMAC RFC R7](../../rfcs/dwmac/index.md)；
[R7 final closure](../../devlog/transactions/2026-08-11-dwmac.md#r7-gate-3-4-closure-and-contract-cutover---2026-08-15)。

## `DWMAC-DESCRIPTOR-001` - DWMAC1000 enhanced descriptor admission

**规范性规则：** DWMAC1000只有在`DMA_HW_FEATURE.ENHDESSEL`成立时才启动，固定使用Linux enhanced/alternate
descriptor与32-byte `dma_extended_desc` stride，并设置/readback CSR0 `ATDS=1`。RX/TX ring只有最后一项置
end-of-ring；OWN在payload、address、length和control提交后最后发布。normal descriptor与格式fallback不属于
当前能力，admission或readback不成立时必须在IRQ/publication前失败。

**违反表现：** normal/enhanced位混写、stride与ATDS不一致、多个EOR、OWN-first发布、或在capability缺失时
猜测descriptor格式。

**验证 / Enforcement：** descriptor layout/OWN/EOR/range KUnit、Gate 2 loopback descriptor/payload证据、
Gate 3 production RX/TX acceptance。

**最初来源 / 当前来源：** [DWMAC RFC R6/R7](../../rfcs/dwmac/index.md)；
[R7 final closure](../../devlog/transactions/2026-08-11-dwmac.md#r7-gate-3-4-closure-and-contract-cutover---2026-08-15)。

## `DWMAC-DMA-ADDR-001` - 32-bit reserved strong-noncache DMA backing

**规范性规则：** 每个2K1000 DWMAC1000 node从自己的DT `no-map` reserved `memory-region`取得最终ring/frame
物理区间，并只通过driver持有的strong-noncache `IoRemap`进行CPU访问。descriptor base、ring和每个frame backing
的完整半开区间必须位于`[0, 4 GiB)`；任何加法溢出、区域不足或越界都在DMA start/IRQ/publication前失败。
driver不得通过HHDM/DMW alias读写该backing，也不得静默截断地址。

**违反表现：** cached与uncached alias混用、CPU看不到OWN writeback、高地址截断、两个端口重叠region，或把
reserved region当作普通allocator可回收页面。

**验证 / Enforcement：** DT per-port region解析与non-overlap审查、checked layout/range KUnit、2K1000
descriptor OWN与production traffic实机证据。

**最初来源 / 当前来源：** [DWMAC RFC R7](../../rfcs/dwmac/index.md)；
[R7 final closure](../../devlog/transactions/2026-08-11-dwmac.md#r7-gate-3-4-closure-and-contract-cutover---2026-08-15)。

## `DWMAC-CAUSE-001` - CSR5 cause clear before level completion

**规范性规则：** DWMAC1000 owner对每个raw CSR5 sample只向定义的W1C cause window写`1`，并立即回读。
Gate 2在CSR7为0时轮询该协议；production IRQ handler在controller `eoi/unmask`前完成同一device-cause clear，
再发布durable recheck。MAC/PCS/MMC read-to-clear cause由同一handler消费；irqchip不猜CSR5，DWMAC不写ICU。
W1C后仍有不可解释cause时fail closed quiesce，不把notification或诊断counter当作cause truth。

**违反表现：** 写回process-state/reserved位，先unmask后清CSR5，IRQ core代清device cause，或旧cause在
handler tail持续造成unchanged-status repeat。

**验证 / Enforcement：** W1C/classification KUnit、Gate 2 RI/TI/readback、Gate 3 handler/pending与网络实机
acceptance；`IRQ-FLOW-001`约束controller transaction。

**最初来源 / 当前来源：** [DWMAC RFC R7](../../rfcs/dwmac/index.md)；
[R7 final closure](../../devlog/transactions/2026-08-11-dwmac.md#r7-gate-3-4-closure-and-contract-cutover---2026-08-15)。

## 当前接受边界

- 2K1000当前板级target覆盖YT8511、`rgmii`/`rgmii-id`、单queue、标准MTU和32-bit reserved DMA region。
- runtime PHY/link变化、cable hotplug、runtime detach/restart与高地址DMA需要独立设计。
- VisionFive 2本轮未重新执行hardware regression；DWMAC4沿用既有hardware acceptance与保持行为的build/source证据。
