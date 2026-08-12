# RFC-20260811-dwmac

**状态：** Accepted / R2 / Gate 1 Closed; Gate 2 Stopped / Not Cut Over
**修订：** R2
**负责人：** Anemone maintainers
**最后更新：** 2026-08-12
**领域：** driver / net / irq / mm / phy
**影响契约：** Accepted target：Refine `IRQ-FLOW-001`；Introduce `DWMAC-DESCRIPTOR-001`、`DWMAC-DMA-ADDR-001`、`DWMAC-CAUSE-001`、`DWMAC-NODE-001`
**执行记录：** [2026-08-11 DWMAC transaction](../../devlog/transactions/2026-08-11-dwmac.md)

## 摘要

本 RFC 把现有 JH7110 GMAC 驱动提升为 DWMAC owner，保留现有 JH7110 DWMAC4/5.20 行为，并新增按
DT `compatible` 匹配的 DWMAC1000 backend，用于 Loongson 2K1000 的 DWMAC 3.70a。`net::dwmac` 只拥有
共享的 per-node discovery、frame capability、IRQ/publication handoff 和既有 network attach；
`net::dwmac::dwmac4` 与 `net::dwmac::dwmac1000` 分别拥有并注册自己的 `Driver`、match table 和
concrete backend。DWMAC4 与 DWMAC1000 各自拥有寄存器、descriptor、DMA addressability、device-cause
和板级 glue。

本 RFC 明确不修改仓库当前 2K1000 DTB。该 DTB 的 ICU 使用 one-cell interrupt specifier，因此
2K1000 irqchip 依据手册建立完整 `IrqSense` source table，同时让 `request_irq()` 接受可选的
`Option<IrqSense>` expectation，在 descriptor publication 和 `unmask()` 前做 fail-closed 校验。
DWMAC1000 R0 只使用 normal 16-byte descriptor、32-bit DMA admission、CSR5 W1C 和 boot-time PHY
transaction；clock/reset/pinctrl 先按 firmware handoff 路线 A 验证。

## 背景

事实调查、Linux 6.6.32 对照、Loongson 2K1000 手册、PMON 路径和待验证路线保存在
[背景定位材料](./backgrounds/positioning.md)。该页是证据材料，不覆盖本 RFC 的规范性 target。

现有 [JH7110 GMAC RFC](../jh7110-gmac/index.md) 已 Closed，并明确把通用 DWMAC family、其它
SoC glue 和 generic PHY framework 列为非目标。本 RFC 重新解析 owner migration、DWMAC1000 legacy
descriptor/DMA/IRQ、2K1000 firmware handoff、PHY handoff 和多 Gate 实机 proof，因此按 RFC 处理。

仓库当前 `conf/platforms/2k1000-board.dts` 描述两个 enabled node：`snps,dwmac-3.70a`/
`snps,arc-dwmac-3.70a`、MMIO `0x40040000`/`0x40050000`、named `macirq` source 12/14、
`eth_wake_irq` source 13/15、`rgmii`/`rgmii-id` 和 MDIO child。两个 node 分别带有
`fa:9c:5b:e6:27:68` 和 `3e:f2:46:6c:3c:f5` 的 `local-mac-address`；node 没有 clocks、resets，且只有
GMAC1 显式包含 pinctrl。这些 resource 缺失不证明硬件无需初始化，只说明当前 DTB 没有给 kernel
对应的 resource owner。

## 目标

- 将 `driver/net/dwmac` 整理为 shared DWMAC owner，外置可共享的 frame、progression、publication
  adapter 和 IRQ resource 选择；由 `dwmac4` 与 `dwmac1000` 子模块分别注册 concrete `Driver`。
- 所有匹配且 enabled 的 node 独立 probe。驱动不固定 GMAC0/GMAC1 数组或数量上限；实际数量仍受
  memory、IRQ、DMA addressability 和 identity resource 限制。
- 严格按照 DT `compatible` 匹配 backend，不按 MMIO base、IRQ 数字、node ordinal 或 aliases 猜测
  family/实例。
- DWMAC1000 执行自己的 MAC/DMA register admission、DMA software reset、legacy CSR、normal
  descriptor setup、MDIO divider、interrupt enable 和 CSR5 cause handling。
- DWMAC1000 R0 固定 `ATDS=0`、normal descriptor、16-byte stride；读取并记录 `ENHDESSEL` 仅用于
  diagnostic。R0 不启用 enhanced/extended descriptor、PTP、timestamp 或 checksum offload。
- 对所有 DWMAC1000 descriptor base、ring、chain next、TX/RX frame backing 做完整 `[start,end)`
  32-bit DMA admission。初始化时打印低于 4 GiB 的板级假设 warning；任一地址达到或超过 4 GiB，该
  node 在 IRQ/publication/`eth<N>` 前失败，不引入 DMA32 allocator 或 bounce buffer。
- 保持当前 DTB 不变。2K1000 irqchip 按手册拥有 `IrqSense` 表：GMAC source 12/13/14/15 为
  `LevelLow`，独立 DMA source 44..48 为 edge/pulse；表项同时导出 `INTEDGE/INTPOL` 和 controller flow。
- 将 `request_irq()` 与 crate-local `request_irq_selected()` 增加 `expected: Option<IrqSense>`；`Some`
  只做 actual type admission assertion，不写 controller、不决定 flow、不形成第二份真相。DWMAC1000
  named `macirq` 使用 `Some(IrqSense::LevelLow)`。
- 外部 clock、reset、pinctrl 首先采用路线 A：消费 firmware handoff，做 capability/readability、DMA
  reset、MDIO 和 cold-boot characterization；失败时停止并回到 owner review，不写 LoongArch raw MMIO。
- 每个 node 以 per-node PHY transaction 从 `phy-handle`/MDIO 识别 PHY，执行 boot-time Clause 22、
  必要 soft reset、vendor/board fixup、autonegotiation 和 link snapshot。P1 不建立 global PHY registry
  或 runtime link manager。
- boot-time live DT 注入的 `local-mac-address` 是 publication-time MAC fact；缺失/非法 MAC 使 node
  失败，不随机生成、不从其它 node 借用。
- 成功 publication/attach 的 node 按 active logical reservation 成功顺序取得连续 `eth0`、`eth1`；
  失败 candidate 不消费 logical identity。physical path、netdev name 和 Stack `InterfaceId` 保持不同
  identity domain。
- 复用现有 `FrameProvider`、global Stack、worker、durable recheck、initial domain、shutdown 和
  static IPv4 control plane；不复制 socket、route 或 protocol state。

## 非目标

- 在后续 Gate 中继续修改当前已含两个板级 `local-mac-address` 的 `conf/platforms/2k1000-board.dts`，
  或引入新的 DT compatible、interrupt cell、clock/reset/pinctrl property；实现必须在当前 DTB 输入上工作。
- 新建通用 clock/reset/syscon/pinctrl/MDIO/PHY framework，或由 DWMAC 直接写 LoongArch controller
  register。路线 A 失败后的 provider 路线是后续 review，不是本 RFC 的隐式 fallback。
- DWMAC1000 enhanced/extended descriptor、PTP、timestamp、TSO、checksum offload、jumbo frame、多 queue、
  EEE/LPI runtime 和 Wake-on-LAN。
- runtime link renegotiation、PHY interrupt、cable hotplug、runtime detach/retry/restart、`free_irq` 或
  runtime device removal。
- DMA32 allocator、IOMMU、bounce buffer 或高于 4 GiB memory support。
- 通用 DWMAC family、其它 SoC glue、SGMII、1000BASE-X 或非当前 `rgmii`/`rgmii-id` board policy。
- 修改 existing network/socket ABI、IPv4 route policy、global Stack、`FrameProvider` 或 current contract
  在最终 cutover 之前的 effective semantics。
- 用 QEMU、host fake backend、编译通过或 Linux source 对照代替 2K1000/JH7110 hardware acceptance。

## Owner 与协议边界

| 状态/能力 | 唯一 owner | Handoff / failure |
| --- | --- | --- |
| DT traversal、compatible match、per-node probe | platform discovery / concrete DWMAC backend | 每个 node 独立失败；失败 node 不 publication |
| interrupt name/index selection | firmware-node IRQ resource layer | 选择单一 `macirq` specifier 后交给 irqchip |
| `IrqSense` source table、`EDGE/POL`、route、mask、controller flow | Loongson 2K1000 irqchip | DTB one-cell 下由 SoC table 拥有；readback 失败阻止 IRQ admission |
| IRQ dispatch order | IRQ core / irqchip | `LevelMaskEoi` 为 `mask -> handler -> eoi -> unmask` |
| `request_irq` expected type | IRQ request owner | actual 由 irqchip table 给出；mismatch 在 mapping/publication/unmask 前失败 |
| DWMAC MAC/DMA registers、descriptor、DMA address | concrete DWMAC4/DWMAC1000 backend | 自己 reset、program、quiesce；不共享另一 family 的 MMIO/bit layout |
| DWMAC CSR5 RI/TI/AIS/W1C | concrete backend device handler | IRQ tail `eoi/unmask` 前清合法 W1C mask，再发布 recheck |
| clock/reset/pinctrl external access | firmware in Route A；future provider only after review | handoff 失败不写 raw SoC register；node fail before publication |
| PHY identity/reset/fixup/link snapshot | per-node PHY transaction | transaction 失败释放 node-local MDIO state；不建 global PHY owner |
| MAC address fact | boot-time live DT / DWMAC probe | `local-mac-address` 非法或缺失时 node fail |
| frame capability / worker / recheck | existing network owners + per-node provider | DWMAC only hands off narrow capability |
| netdev publication / active logical identity | device/net + attach authority | publication success 后按成功顺序分配 `eth<N>`；失败不占号 |

### Initialization and IRQ handoff

每个 node 顺序必须为：compatible admission -> resource/MMIO/capability -> Route A handoff check -> DWMAC
internal DMA reset -> per-node MDIO/PHY transaction -> descriptor/DMA backing 和 32-bit checks ->
`IrqSense`/controller admission -> device-cause baseline and interrupt enable -> `FrameProvider`/worker ->
`request_irq(..., Some(LevelLow))` -> publication/attach。可能失败的步骤必须在 IRQ unmask 和 netdev publication
前完成，避免现有系统缺少 runtime removal 时留下 enabled orphan。

```text
ICU source pending
  -> IRQ core mask
  -> read CSR5 and DMA interrupt enable
  -> write only DWMAC W1C cause mask
  -> publish durable recheck/wake
  -> complete RX/TX descriptors
  -> irqchip eoi (Loongson level eoi is no-op)
  -> unmask
```

ICU ack/mask/eoi 不清 CSR5；DWMAC W1C 不清 ICU source。旧 cause 在 unmask 后立即重投是 bug，新 cause
产生的下一次 dispatch 是允许行为。

## ABI 与可见语义

- 不新增 userspace syscall、socket、netlink 或文件 ABI。`eth<N>`、MAC、link snapshot、publication failure
  和现有 network attach 语义是唯一可见 surface。
- `request_irq`/`request_irq_selected` 是 kernel-internal API surface，不是 userspace ABI；旧 caller
  传 `None` 保持行为，DWMAC1000 显式传 `Some(IrqSense::LevelLow)`。
- `eth<N>` 只表示成功 active publication 顺序，不承诺与 physical node、MMIO base、IRQ source 或 DT alias
  对应。失败 node 不产生可见 netdev、不占用 name。
- `local-mac-address` 是板级输入；非法/缺失时 fail closed，不随机补 MAC。
- R0 对 enhanced/extended/PTP、DMA 高地址、runtime PHY/link 和 wake/LPI 行为返回不支持或 probe failure，
  不静默伪装成可用能力。

## Contract Impact

这些是 target contract delta；在 RFC closure/cutover 前不覆盖 current contract。

| Contract ID | 变化 | Target 摘要 | Cutover |
| --- | --- | --- | --- |
| `IRQ-FLOW-001` | Refine | irqchip-owned `IrqSense` table；optional request expectation 在 mapping/publication/unmask 前 fail closed；device cause 仍由 concrete driver 清除 | `DWMAC-IRQ-CUTOVER` |
| `DWMAC-NODE-001` | Introduce | per-node DWMAC backend ownership、failure isolation、success-order active publication | `DWMAC-FINAL-CUTOVER` |
| `DWMAC-DESCRIPTOR-001` | Introduce | DWMAC1000 normal descriptor、`ATDS=0`、16-byte stride；enhanced/extended/PTP excluded | `DWMAC1000-CUTOVER` |
| `DWMAC-DMA-ADDR-001` | Introduce | 所有 legacy DWMAC1000 DMA 地址完整位于 `[0, 4GiB)`；越界 node fail before publication | `DWMAC1000-CUTOVER` |
| `DWMAC-CAUSE-001` | Introduce | CSR5 W1C cause clear before level IRQ completion/unmask；只写合法 cause mask | `DWMAC-IRQ-CUTOVER` |

### Dependencies

- [NETDEV-LIFE-001](../../contracts/net/netdev-lifecycle.md)：publication 与 terminal lifecycle baseline。
- [NET-BOUNDARY-001 / NET-FRAME-OWN-001 / NET-FRAME-PROGRESS-001](../../contracts/net/frame-path.md)：frame capability 和 progress handoff。
- [NET-ATTACH-001](../../contracts/net/attach-lifecycle.md)：attach、initial domain 和 shutdown baseline。
- [IRQ-FLOW-001](../../contracts/interrupt/index.md)：当前 controller/device cause ordering baseline；本 RFC 只提出上述 Refine。

## Implementation Boundary

### 允许改变

- DWMAC owner 目录、common/DWMAC4/DWMAC1000 concrete backend、同 owner 的内部 module/import/re-export 和 targeted tests。
- Loongson 2K1000 concrete irqchip 的 source table、`IrqSense` readback/configuration、request expectation validation，以及必要的 kernel-internal `request_irq` surface。
- DWMAC1000 descriptor/register/DMA/MDIO/PHY transaction 和 node-local failure handling。
- RFC-local target contract pages、implementation evidence 和最终 cutover documentation。

### 必须保持

- 当前 DTB 字节内容、compatible/resource/name 形状、existing JH7110 visible behavior 和 current network/socket ABI。
- IRQ core owner/handoff：device driver 清 device cause，irqchip 不猜 CSR5，DWMAC 不写 ICU registers。
- per-node state ownership、failure-before-publication、success-order `eth<N>` 和既有 attach/Stack/worker semantics。
- DWMAC family 的寄存器/descriptor owner isolation；不能为了共享抽象暴露大而全的 raw register trait。

### 停止条件

- 需要修改 DTB、增加 compatible/resource、改变用户可见 net semantics、移动 Stack/attach owner 或改变现有 JH7110 ABI。
- DWMAC1000 normal descriptor 在实际 core 上不成立，必须改用 enhanced/extended；这需要 target renegotiation。
- Route A 证明 clock/reset/pinctrl handoff 不成立，或 PHY reset/fixup owner 无法闭合；回到 RFC review 决定 Route B。
- allocator 无法满足 32-bit admission 且需要 DMA32/bounce/IOMMU；不降低 correctness invariant，停止并提出新 target。
- `IrqSense`/request expectation 需要扩大成新的通用 interrupt ABI，或出现第二份 electrical/flow truth。

## Acceptance 与 Validation

RFC 被接受只表示 target/owner/boundary/contract delta 获得 review，不表示硬件已工作。最终 closure 至少需要：

- source review：compatible dispatch、per-node owner、descriptor bit layout、DMA checked arithmetic、IRQ `IrqSense` table、request expectation、CSR5 W1C 和 failure/cleanup 顺序；
- targeted KUnit/source tests：normal descriptor OWN/length/end-ring、32-bit range rejection、sense matching/mismatch、named `macirq` selection、W1C mask 和 no-publication-on-failure；
- build/format：kernel build、DTS consumer discovery audit、DWMAC4 protected regression；
- 2K1000 hardware probe：version/capability、DMA reset、`EDGE/POL` readback、MDIO、PHY IDs/link、MAC source、cold/warm/bootloader handoff、RX/TX/abnormal IRQ sequence；
- 2K1000 runtime：single-port attach/traffic first, then independent two-port traffic, success-order `eth<N>`、shutdown/reboot 和 no immediate unchanged-status IRQ repeat；
- JH7110 hardware regression：existing dual-port behavior remains unchanged after owner migration。

用户提供的 2K1000 Gate 1 实机日志证明两个 enabled DWMAC1000 node 都按 compatible 进入 variant-local
`dwmac1000` Driver，并在任何 Gate 2 hardware transaction 或 netdev publication 前以 `NotSupported`
fail closed。同次启动的 ICU readback 为 `EDGE=0x1f00000000000`、`POL=0xf000`，精确对应 edge bits
44..48 与 active-low bits 12..15，关闭 Gate 1 的 controller programming proof。该日志不证明 DWMAC1000
register/descriptor/PHY/traffic。RiscV/JH7110 hardware regression 仍为 Not Run；QEMU 不能替代这些
hardware acceptance。
每个 node 的失败、DMA 越界、PHY/MDIO timeout、IRQ type mismatch 和 handoff failure 都必须在 publication 前
可观察并保持其它 candidate 独立继续 probe。

## 风险与反馈

- `DMA_HW_FEATURE.ENHDESSEL` 为 1 但 normal mode 在 2K1000 上不可靠：Gate 2 停止，不静默切换 enhanced。
- 当前 DTB 不表达 polarity，vendor Linux fork 与主线 polarity 编程存在差异：以手册/mainline 对照形成 target，并以 ICU `EDGE/POL` readback 和真实 RX/TX 事件作最终证据。
- Route A 可能在 cold boot、warm reboot 或不同 firmware 使用历史下失败：失败信号写回 tracking issue，不能添加 raw SoC fallback。
- PHY `rgmii-id` delay 可能被 soft reset 清除：Gate 2 先完成 PHY ID/reset-effect characterization，未闭合时不执行 production reset。
- 全局 allocator 未来可能返回高于 4 GiB backing：DWMAC1000 node fail closed；DMA32/bounce 是后续 RFC，不是本 RFC 的隐藏扩展。

## 文档与证据

- [目标与不变量](./invariants.md)：唯一 owner、descriptor/DMA/IRQ/PHY/publication proof obligations。
- [实施路线](./implementation.md)：三个实现 Gate、一个最终 closure Gate、probe、退出条件和验证安排。
- [Tracking Issues](./tracking-issues.md)：仍影响 Gate/acceptance 的事实缺口。
- [背景材料](./backgrounds/index.md)：事实定位与 Linux/Loongson 对照。
- 外部源码：`xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac_lib.c#dwmac_dma_interrupt`、`xref:linux-6.6.32:arch/mips/boot/dts/loongson/loongson64-2k1000.dtsi#L136-L161`；固定 commit 的手册和 PMON 链接见背景页。
- 执行记录：[2026-08-11 DWMAC transaction](../../devlog/transactions/2026-08-11-dwmac.md)。

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| Draft | 2026-08-11 | 将 DWMAC4 owner migration、DWMAC1000 R0、2K1000 IRQ/DMA/PHY/Route A 边界形成正式 RFC 草案；DTB 保持不变。 | 本 RFC review；尚无实现 evidence |
| R0 | 2026-08-11 | 接受 Draft target、owner、Implementation Boundary、contract delta、Gate 顺序和停止条件；只授权 Gate 1。 | [R0 acceptance](../../devlog/transactions/2026-08-11-dwmac.md#r0-acceptance-and-gate-1-authorization---2026-08-11) |
| R1 | 2026-08-12 | 接受 owner 形状修订：`net::dwmac::dwmac4` 与 `net::dwmac::dwmac1000` 各自拥有并注册 `Driver`/match table；`net::dwmac` 只保留 shared probe/frame/publication helper。target contract、ABI、DTB、visible semantics、Gate 顺序和 validation strength 不变；重新授权 Gate 1。 | [R1 revision and Gate 1 re-authorization](../../devlog/transactions/2026-08-11-dwmac.md#r1-revision-and-gate-1-re-authorization---2026-08-12) |
| R2 | 2026-08-12 | 接受 Gate 1 validation staging 修订：以 source/KUnit/build/review、2K1000 双 node compatible dispatch + expected `NotSupported` fail-closed，以及精确 `EDGE/POL` readback实机证据关闭 Gate 1；RiscV/JH7110 regression 保持 Not Run并移交 Gate 3/final closure，不降低最终 proof。target、owner、ABI、DTB、visible semantics、Contract Impact 和 current contracts 不变；Gate 2 未授权。 | [R2 Gate 1 closure](../../devlog/transactions/2026-08-11-dwmac.md#r2-gate-1-validation-revision-and-closure---2026-08-12) |

## Closure

RFC 当前未 closure、未 cutover、未更新 current contracts。R2 已关闭 Gate 1。用户随后授权执行 Gate 2，
但该 Gate 因 Route A、PHY reset-effect、normal descriptor 和 32-bit DMA admission 的 Open blocker
以及缺少 2K1000 bounded hardware evidence 而停止；实现保持 registration-only、Not Cut Over。不得从
Gate 1 的 expected `NotSupported` 日志、build 或 QEMU 外推 DWMAC1000 runtime correctness；RiscV/JH7110
regression 仍必须在 Gate 3/final closure 补齐。后续重新进入 Gate 2 需要补齐 tracking issues 所要求的
权威硬件事实；任一 target/owner/acceptance 变化仍须回 RFC review。
