# RFC-20260808-jh7110-gmac

**状态：** Accepted
**修订：** R4
**负责人：** Anemone maintainers
**最后更新：** 2026-08-09
**领域：** driver / net / irq / mm
**影响契约：** `IRQ-FLOW-001`
**执行记录：** Git commit（R0 acceptance；Gate 0/1 closure；R2--R4 target renegotiation）

## 摘要

本 RFC 提议为 JH7110 的 DWMAC 5.20 GMAC 节点实现 boot-time、one-time-initialized 的
以太网驱动。所有 `status = "okay"` 且 compatible 匹配的节点都按 DT discovery 顺序独立
probe；每个节点拥有自己的 MMIO、DMA-coherent backing、descriptor ring、IRQ context、
`FrameProvider`、worker 和 recheck edge，不建立 GMAC bus、GMAC 全局 registry 或固定双实例表。

通用 clock/reset provider 拥有控制寄存器与 enable/reset transaction；GMAC 只按本节点 DT name 请求
boot-time consumer admission，不保存 provider 状态，也不执行失败回滚。firmware 继续拥有 clock
rate/mux、syscon/RGMII path 与 PHY handoff。本 RFC 不建立这些通用 owner，也不接管运行时 link
management。驱动只在硬件 DMA coherency、ordering、
DMA ownership、`macirq` 和 per-node resource 均成立后发布 netdev。多个已发布
GMAC 进入现有 initial domain 与唯一 global Stack；沿用现有单个 `[network.ipv4]` 配置，只给
配置选中的成功顺序 `eth<N>` 分配 IPv4 和唯一 default route。

QEMU 没有该设备，不能提供本 RFC 的实现验收。四个实现 Gate 各自完成 source/build/targeted-test
检查，但不产生硬件通过结论；只有所有 Gate 实现并检查后，才在 VisionFive 2 上执行一次完整
板级验收。板级验收通过前不做 current-contract cutover，也不关闭 RFC。

## 背景

仓库中的 VisionFive 2 DT 同时描述 `ethernet@16030000` 与 `ethernet@16040000`，两者具有独立
MMIO 和三项中断，`interrupt-names` 顺序为 `macirq`、`eth_wake_irq`、`eth_lpi`。当前运行时
观察已经确认 `ethernet@16040000` 提供 `local-mac-address`；GMAC0 仍必须在最终板级验收中
独立确认，不能从 GMAC1 推导。

当前 platform discovery 按 DT child order 同步注册设备；built-in platform driver 已在 DT
展开前注册，因此每个匹配节点都会独立进入 `probe()`。这个路径已经满足 per-node discovery
的基本形状，不需要增加 GMAC bus。当前 IRQ API 则只有三参数 `request_irq(device, handler,
private)`；它取得整个 `interrupts` 属性并直接交给 irqchip `xlate()`，不能从多项 specifier 中
选择 `macirq`。

现有网络路径已经拥有 boot-time netdev registry、initial-domain logical namespace、唯一
global Stack、per-interface worker 和单接口 static IPv4 control plane。logical reservation 发生在
成功 publication 后的 attach transaction 内。R4 接受这个既有线性化点：初始化或 publication 失败的
candidate 不消费 logical identity；成功 publisher 按 publication/attach 顺序连续取得 `eth<N>`。

`DmaRegion::sync_for_device()` 与 `sync_for_cpu()` 当前只有 fence。R2 接受用户提供的板级证据：
JH7110 GMAC 的 DMA 路径是 coherent，因此不要求额外的 clean/invalidate 才能实现 CPU/device
visibility；现有 fence 仍只承担 ordering，不能替代 descriptor ownership、MMIO doorbell/completion
顺序或 quiesce 证明。Linux 6.6.32 的 RISC-V 默认 coherent DMA 与 `dma-noncoherent` 覆盖规则可作为
外部佐证，但不替代 Anemone 的硬件验收。

## 目标

- 支持任意有限数量的匹配 GMAC DT 节点；数量只受内存、IRQ、DMA addressability 和 identity
  space 限制，不在 driver 中写死 GMAC0/GMAC1 分支或实例上限。
- 以每个 DT node 为独立 failure 和 ownership domain，建立一套 MMIO、单 RX queue、单 TX
  queue、有界 descriptor/frame backing、IRQ context、provider、worker 和 wake path。
- 增加 crate-local IRQ resource selector，使 driver 按 interrupt name 或 index 取得单个 specifier；
  现有 public `request_irq()` 保持不变。首版 JH7110 只选择 `macirq`，不请求 Wake/LPI IRQ。
- 在 publication 前完成 coherent DMA visibility 的调用语义、memory/MMIO ordering、DMA address
  representation 与 descriptor/frame ownership proof；不以 coherency 假设掩盖 ordering 或 ownership
  错误。
- 只有完成 clock/reset、MMIO/capability、rings、IRQ context、device-cause baseline 和 provider
  construction 的节点才允许 publish；初始化或 publication 失败不消费 `eth<N>`。
- 沿用现有 publication 后 attach-time reservation：成功 publisher 按 publication/attach 顺序连续取得
  `eth<N>`。同步 probe 保持成功 JH7110 节点之间的 DT 相对顺序；较早节点失败时，后续成功节点前移补位。
- 复用 `device/net`、`FrameProvider`、initial domain、global Stack、worker、control plane 与
  shutdown protocol，不复制 socket、route、logical identity 或 protocol state。
- 保持现有 SystemTarget 配置形状；所有成功 GMAC 都可成为 active L2 path，但只有
  `network.ipv4.interface` 选中的一个接口获得静态 IPv4 和唯一 default route。
- 在所有实现 Gate 检查通过后，以 VisionFive 2 双 GMAC 实机证据一次性验收完整路径。

这里的“任意有限数量”是“不按 GMAC0/GMAC1 固定展开”的实现保证，不承诺无界资源或
runtime hotplug。

## 非目标

- 新建或扩张通用 clock/reset/syscon/pinctrl/MDIO/PHY framework，以及 GMAC 直接解析 provider ID、写
  controller register 或缓存 controller 状态。
- 运行时 link renegotiation、cable hotplug policy、PHY interrupt、runtime detach/retry/restart。
- Wake-on-LAN、EEE/LPI interrupt、TSO、checksum offload、PTP、Jumbo Frame 或多 queue。
- 通用 DWMAC family、其它 SoC glue、SGMII、1000BASE-X 或非 `rgmii-id` 板型。
- 多 IPv4 配置、多 default route、metric、DHCP、runtime address/route API，或 control plane 在 committed
  logical name 之外主动扫描其它接口。R4 接受失败 candidate 不占号带来的 success-order identity 前移。
- GMAC bus、全局 GMAC registry、按硬编码 MMIO/IRQ 判断实例，或由 driver 分配 `eth<N>`。
- `free_irq`、完整 device removal 或在 orderly shutdown 中证明全部 DMA backing 已回收。
- 用 QEMU、host fake backend 或 build success 代替 VisionFive 2 验收。

## Owner 与协议边界

| 状态 / 能力 | 唯一 owner | 跨 owner handoff |
| --- | --- | --- |
| DT traversal、platform device 与 driver binding | generic discovery / platform bus | 每个 matching node 独立调用 concrete `probe()` |
| interrupt name/index 解析与单项 specifier | firmware-node / IRQ resource layer | 解析后的单个 specifier 交给 irqchip `xlate()` |
| controller mapping 与 dispatch flow | IRQ core / irqchip | handler 前后遵循 `IRQ-FLOW-001` |
| clock gate 与 reset transaction | generic clock/reset provider | GMAC 按本节点 DT name 请求 one-time consumer admission |
| clock rate/mux、syscon/RGMII path 与 PHY | firmware/board environment | GMAC 消费既有 board handoff，不建立运行时管理 |
| MAC/DMA registers、rings、frames、completion、current link truth | per-node JH7110 provider | callback-scoped frame capability与durable recheck edge |
| netdev identity与publication record | `device/net` | published capability交给 attach authority |
| logical identity、ifindex、`eth<N>` reservation与membership | initial-domain `LogicalInterfaces` | attach authority 在成功 publication 后创建并提交 reservation |
| protocol `InterfaceId`与mapping | global `DomainStack` | worker只持自己的 narrow pump port |
| IPv4 address、route、source/interface selection | `Ipv4ControlPlane` | 读取 committed logical member 与 SystemTarget deployment |
| worker admission与network shutdown | kernel attach authority | `PumpControl` 后接 device-local shutdown attempt |

### Handoff 与线性化点

1. Platform bus 按 DT discovery order 进入 matching-node probe。GMAC 先通过 generic providers 按
   `clock-names`/`reset-names` 请求 boot-time admission；provider 独占寄存器与 transaction 语义。
2. IRQ resource layer 以 name `macirq` 解析 index，再按 interrupt parent 的 cell width 截取恰好
   一个 specifier；irqchip 只翻译该项，descriptor 发布后 IRQ core 才 unmask。
3. Provider 在 queue、RX refill、IRQ、cache/DMA proof 和 wake wiring 完成后，才将 ready
   capability移交 `device/net`。初始化失败不 publish，也不进入 logical identity owner。
4. Attach authority 按 successful publication order 创建 logical reservation，建立 Stack mapping 和
   inactive worker，最后在同一 authority transaction 中 commit reservation、记录 active path 并 activate
   worker。
5. Frame backing 的每次 CPU/device ownership transfer 以 descriptor ownership publication 或
   completion observation 为线性化点；coherent visibility 与 ordering fence 都必须在对应一侧成立。

### Failure 与 cleanup

- Clock enable 是 boot-lifetime 单调 admission；后续 clock/reset/resource/probe 失败不关闭已启用 clock。
  Reset transaction 失败只让当前 node fail closed，不由 GMAC 猜测、缓存或反向补偿 provider 状态。
  已完成的 reset 也不因后续 probe 失败而重放或回滚。
- provider ready/publication 前的 MAC、MMIO、IRQ、DMA 或 provider failure 不发布 netdev，也不消费 logical
  identity。已启用 clock/reset 仍按上述单向 admission 规则保留。
- IRQ 注册前的失败释放 owner-local allocations。IRQ 注册后因当前没有 `free_irq`，provider 必须
  disable device-side interrupt/DMA 并保留仍可被 IRQ/device 访问的 context/backing 到 reset/power-off。
- publication 后 attach 失败按 `Stack mapping -> logical reservation` 顺序回滚，并保留 provider 为
  published/unattached；attach 已取得的 ordinal 按现有 no-reuse 规则保持已消费。一个节点的失败不修改
  其它已提交节点的 identity、mapping、worker 或 rings。
- orderly shutdown 保持 `filesystem -> network -> device -> PowerOff`。network 先关闭 worker admission；
  concrete driver 随后停止本节点 IRQ generation/DMA。未证明 device quiesce 前不得释放 backing。
- control plane 只按配置中的 exact committed `eth<N>` 匹配；不存在或未 commit 时 fail closed，不扫描其它
  active GMAC。较早 candidate 初始化失败后，后续成功 provider 取得同一 success-order name 属于 R4 接受的
  identity 语义，不是 control-plane fallback。

## ABI 与可见语义

没有新增 syscall、ioctl 或用户态网络配置 ABI。SystemTarget 保持：

```toml
[network.ipv4]
interface = "eth0"
address = "10.0.2.15"
prefix = 24
default-gateway = "10.0.2.2"
```

新增的 kernel-internal IRQ surface 必须表达 `name` 或 `index` selector。name 必须在
`interrupt-names` 中唯一解析；index 必须位于 specifier 数量范围内；name/index 缺失、重复、长度
不匹配或越界必须在 IRQ mapping/unmask 前失败。driver 不解析 PLIC raw cell，也不把 `macirq` 的
当前数值当作 ABI。

多 GMAC 可见语义是 success-order `eth<N>`：只有成功初始化并 publish 的 provider 参与编号，ordinal
由 successful publication/attach order 决定并连续分配。同步 platform probe 保持成功 JH7110 节点之间的
DT 相对顺序；失败 candidate 不留空洞，后续成功 provider 前移。`NetdevId`、logical
identity/ifindex/name 与 protocol `InterfaceId` 继续是不同 identity domain。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `IRQ-FLOW-001` | Refine | [controller flow 与 device cause handoff](../../contracts/interrupt/index.md#irq-flow-001---controller-flow与device-cause-handoff) | descriptor 建立前由 IRQ resource owner 按 name/index 唯一选择单项 firmware specifier；既有 controller/device cause 顺序不变 | `JH7110-GMAC-CUTOVER` |

该 Refine 只在最终板级验收通过后 cut over。Gate 0--3 的局部检查不提前修改 current
contract。

### Dependencies

- [NETDEV-LIFE-001](../../contracts/net/netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)：ready-before-publication 与 published/unattached。
- [NET-IFACE-DOMAIN-001](../../contracts/net/interface-domain.md#net-iface-domain-001--initial-domain拥有logical-interface-namespace)：成功 publication 后由 attach transaction 创建、提交或 abort logical reservation。
- [NET-ATTACH-001](../../contracts/net/attach-lifecycle.md#net-attach-001--attach-publicationrollback与best-effort-shutdown)：既有 publication/attach rollback 与 shutdown 顺序。
- [NET-BOUNDARY-001 / NET-FRAME-OWN-001 / NET-FRAME-PROGRESS-001 / NET-STACK-PUMP-001](../../contracts/net/frame-path.md)：frame capability、唯一 backing owner、有界 backpressure 与唯一 Stack progression。
- [NET-CONTROL-PLANE-001](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface)：单一 SystemTarget interface/address/default route 选择。
- [SYSTEM-POWER-ORDERLY-001](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)：network 先于 device shutdown。
- [STM-TARGET-001](../../contracts/configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract)：现有 `[network.ipv4]` 配置形状。

## Implementation Boundary

- **允许改变：** JH7110 net driver；按 name/index 选择单项 interrupt 的 FwNode/IRQ resource surface；
  RISC-V/JH7110 coherent DMA sync/order capability；per-device staged provider state；owner-local tests、
  target config/Kconfig 与必要 module registration。
- **必须保持：** per-node provider 是 queue/DMA/IRQ truth 的唯一 owner；driver 不拥有 logical
  namespace、route 或 Stack；现有 SystemTarget schema、socket ABI、single-control-plane 语义、
  VirtIO 的可见单接口行为与现有 publication/attach reservation path、frame callback boundary 和
  shutdown participant order。
- **实现提示：** 预计涉及 `driver/net`、`exception/intr/irq`、firmware-node DT parsing、`mm/dma`、
  `device/net` 与 `net/domain`；这些是非穷举提示，不是逐文件 write set。
- **验证 claim：** Gate 检查只证明 source/build/targeted semantics；JH7110 IRQ、coherency/order、PHY handoff、
  双口收发和相同成功集合下的跨启动 identity 只能由最终 VisionFive 2 acceptance 证明。
- **停止条件：** 任何证据要求 driver 接管 clock/reset/syscon/PHY owner，改变 SystemTarget 为多 IP，
  让 driver 保存 `eth<N>`/route/Stack truth，把 fence-only 当作硬件 coherency 保证，未证明 ordering、
  ownership 或 quiesce，使用固定 GMAC 数组，改变 frame ownership，或降低最终板级验收时，必须回到
  RFC review / Target Renegotiation。

## Acceptance 与 Validation

接受本 R4 RFC 只表示同意上述 target、owner、contract delta 与实施路线，不表示 Gate 1 或完整硬件能力已经
交付。实现 closure 必须依次满足：

1. Gate 0--3 全部实现，并在每个 Gate 后完成其 source audit、build、targeted tests 和
   Architecture Friction Scan。
2. 所有 Gate 完成后，才在 VisionFive 2 上执行[最终板级验收](./implementation.md#最终板级验收)。
3. 板级证据覆盖每个目标 node 的 MAC/MMIO/`macirq`、真实 RX/TX、ring wrap 与重复流量、success-order
   `eth<N>`、单接口 static IPv4、exact-name control-plane selection、failure isolation 和 orderly shutdown。
4. 只有上述证据同时成立，才执行单个 `JH7110-GMAC-CUTOVER`、更新 `IRQ-FLOW-001` 并将
   RFC 关闭。

QEMU 可用于保护既有 VirtIO/network regression，但结果必须标记为 regression-only，不能计入本
RFC acceptance。Gate 0 implementation 与用户确认通过的 VisionFive 2 Gate 0 board diagnostic 已关闭；
完整 board acceptance 与 contract cutover 仍为 Not Run。

## 风险与反馈

最大风险是 coherency 与 ordering 语义未被 production call site 正确使用、DWMAC DMA addressability、
IRQ 注册后的不可回收生命周期、Gate 2 staged provider 的 per-device owner，以及 success-order 编号在
candidate 失败时产生的预期前移。对应
correctness obligations 见[目标与不变量](./invariants.md)，逐 Gate deliverable、检查和 hard stop 见
[实施路线](./implementation.md)。任何实现反馈若改变 target、owner、cleanup、contract delta 或
acceptance，必须先回到 RFC review，不能把较弱路径记作完成。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- [RFC 前定位材料](./positioning.md)（非规范）
- commit / PR / optional transaction：Git commits（R0 acceptance；Gate 0 closure；R2--R4 target
  renegotiation；Gate 1 closure 与 review follow-up）；optional transaction 为 None，本 RFC 的执行证据
  由 Git 与本页 closure 保存。
- 外部源码证据：固定 `xref:linux-6.6.32:arch/riscv/Kconfig#ARCH_DMA_DEFAULT_COHERENT`、
  `xref:linux-6.6.32:drivers/of/address.c#of_dma_is_coherent`、
  `xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/dwmac4.h`、
  `dwmac4_dma.h`、`hwif.h` 与 `common.h`；硬件描述基线使用仓库跟踪的
  [`visionfive2-board.dts`](../../../../conf/platforms/visionfive2-board.dts)。

## 修订记录

- `R0`：接受初始 target、owner、contract delta 与 acceptance。
- `R1`：按板级 authority 只接受每节点 `local-mac-address`，删除通用 `mac-address` precedence；IRQ
  selector 保持 crate-local，现有 public `request_irq()` 不扩张。其它 target、owner、contract delta 与
  acceptance 不变。
- `R2`：接受用户提供的 VisionFive 2 板级证据，将 JH7110 GMAC DMA target 从 non-coherent 修订为
  coherent。删除强制 clean/invalidate 和 cache-line ownership isolation，保留独立的 fence/MMIO
  ordering、DMA addressability、descriptor/frame layout、ownership 与 quiesce proof；其它 target、
  owner、contract delta 与 acceptance 不变。
- `R3`：接受 provider-owned clock/reset consumer admission。GMAC 按本节点 DT name 请求能力，generic
  providers 唯一拥有 register 与 transaction；clock enable 和已完成 reset 均不回滚，reset failure 只使
  当前 node fail closed。firmware 继续拥有 rate/mux、syscon/RGMII 与 PHY；public API、contract delta
  和最终 acceptance 不变。
- `R4`：接受 success-order logical identity。只有完成硬件/provider 初始化并 publish 的节点进入既有
  attach-time reservation；初始化或 publication 失败不消费 `eth<N>`，后续成功 provider 连续前移。
  control plane 仍只匹配 exact committed name，不主动扫描 fallback。Gate 2 以 per-device `drv_state`
  保留未发布 provider 并成功 bind，Gate 3 再迁移到 production publication/worker owner；删除
  `NET-IFACE-DOMAIN-001` 与 `NET-ATTACH-001` target Refine，保留它们为 unchanged dependencies。

## Closure

Not Closed。R4 target 已接受；Gate 0 implementation/board diagnostic 与 Gate 1 software implementation
均已关闭。Gate 0 诊断确认两
个 GMAC 节点都能独立读取 DWMAC capability，但 Gate 0 仍按设计在 DMA/IRQ/attach 前返回
`NotYetImplemented`；Gate 1 已建立 stopped RX/TX rings、coherent ordering、40-bit address validation、
device-cause handler 与 registered-IRQ retention，并继续在 publication 前返回临时错误。QEMU 不含
JH7110。用户板级证据已闭合 coherency 前提，R3 已闭合 clock/reset consumer admission owner 与
no-rollback failure semantics；R4 已接受 success-order `eth<N>` 与 Gate 2 per-device staged bind。
Gate 1 硬件状态仍为 Not Run，Gate 2--3、最终 VisionFive 2 验收与 current-contract cutover 均未完成。
