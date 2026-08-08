# RFC-20260808-jh7110-gmac

**状态：** Accepted
**修订：** R0
**负责人：** Anemone maintainers
**最后更新：** 2026-08-08
**领域：** driver / net / irq / mm
**影响契约：** `IRQ-FLOW-001`、`NET-IFACE-DOMAIN-001`、`NET-ATTACH-001`
**执行记录：** Git commit（R0 acceptance；Gate 0 execution follows）

## 摘要

本 RFC 提议为 JH7110 的 DWMAC 5.20 GMAC 节点实现 boot-time、one-time-initialized 的
以太网驱动。所有 `status = "okay"` 且 compatible 匹配的节点都按 DT discovery 顺序独立
probe；每个节点拥有自己的 MMIO、非一致 DMA backing、descriptor ring、IRQ context、
`FrameProvider`、worker 和 recheck edge，不建立 GMAC bus、GMAC 全局 registry 或固定双实例表。

板级 clock、reset、syscon/RGMII path 与 PHY 被视为 firmware handoff 前提。本 RFC 不建立这些
子系统的通用 owner，也不接管运行时 link management。驱动只在真实 non-coherent cache
maintenance、DMA ownership、`macirq` 和 per-node resource 均成立后发布 netdev。多个已发布
GMAC 进入现有 initial domain 与唯一 global Stack；沿用现有单个 `[network.ipv4]` 配置，只给
配置选中的稳定 `eth<N>` 分配 IPv4 和唯一 default route。

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
global Stack、per-interface worker 和单接口 static IPv4 control plane。当前 logical reservation
发生在成功 publication 后的 attach transaction 内；若把失败节点排除在编号之外，后续节点会
顶替 `eth<N>`，不满足本 RFC 的稳定 DT identity 目标。

`DmaRegion::sync_for_device()` 与 `sync_for_cpu()` 当前只有 fence。JH7110 不是 DMA-coherent，
因此 fence-only 实现不能作为 descriptor 或 frame backing 的 cache correctness 证明，也不能
进入 active frame path。

## 目标

- 支持任意有限数量的匹配 GMAC DT 节点；数量只受内存、IRQ、DMA addressability 和 identity
  space 限制，不在 driver 中写死 GMAC0/GMAC1 分支或实例上限。
- 以每个 DT node 为独立 failure 和 ownership domain，建立一套 MMIO、单 RX queue、单 TX
  queue、有界 descriptor/frame backing、IRQ context、provider、worker 和 wake path。
- 扩展 IRQ 公共接口，使 driver 按 interrupt name 或 index 请求单个 specifier；首版 JH7110
  只选择 `macirq`，不请求 Wake/LPI IRQ。
- 在 publication 前完成真实 non-coherent cache clean/invalidate、memory ordering、DMA address
  representation 与 descriptor/frame ownership proof。
- 让每个匹配节点在 probe admission 时按稳定 DT discovery order 消费一个 `eth<N>` reservation；
  后续失败只 abort membership，不复用 ordinal，也不让下一节点顶替。
- 让所有 boot-time external netdev publisher（包括现有 VirtIO provider）使用同一个 opaque
  reservation handoff；QEMU 的单网卡仍显示为 `eth0`，不保留按 provider 类型分叉的编号语义。
- 复用 `device/net`、`FrameProvider`、initial domain、global Stack、worker、control plane 与
  shutdown protocol，不复制 socket、route、logical identity 或 protocol state。
- 保持现有 SystemTarget 配置形状；所有成功 GMAC 都可成为 active L2 path，但只有
  `network.ipv4.interface` 选中的一个接口获得静态 IPv4 和唯一 default route。
- 在所有实现 Gate 检查通过后，以 VisionFive 2 双 GMAC 实机证据一次性验收完整路径。

这里的“任意有限数量”是“不按 GMAC0/GMAC1 固定展开”的实现保证，不承诺无界资源或
runtime hotplug。

## 非目标

- 通用 clock、reset、syscon、pinctrl、MDIO 或 PHY framework，以及 driver 主动完成板级上电。
- 运行时 link renegotiation、cable hotplug policy、PHY interrupt、runtime detach/retry/restart。
- Wake-on-LAN、EEE/LPI interrupt、TSO、checksum offload、PTP、Jumbo Frame 或多 queue。
- 通用 DWMAC family、其它 SoC glue、SGMII、1000BASE-X 或非 `rgmii-id` 板型。
- 多 IPv4 配置、多 default route、metric、DHCP、runtime address/route API 或自动接口 fallback。
- GMAC bus、全局 GMAC registry、按硬编码 MMIO/IRQ 判断实例，或由 driver 分配 `eth<N>`。
- `free_irq`、完整 device removal 或在 orderly shutdown 中证明全部 DMA backing 已回收。
- 用 QEMU、host fake backend 或 build success 代替 VisionFive 2 验收。

## Owner 与协议边界

| 状态 / 能力 | 唯一 owner | 跨 owner handoff |
| --- | --- | --- |
| DT traversal、platform device 与 driver binding | generic discovery / platform bus | 每个 matching node 独立调用 concrete `probe()` |
| interrupt name/index 解析与单项 specifier | firmware-node / IRQ resource layer | 解析后的单个 specifier 交给 irqchip `xlate()` |
| controller mapping 与 dispatch flow | IRQ core / irqchip | handler 前后遵循 `IRQ-FLOW-001` |
| MAC/DMA registers、rings、frames、completion、current link truth | per-node JH7110 provider | callback-scoped frame capability与durable recheck edge |
| netdev identity与publication record | `device/net` | published capability交给 attach authority |
| logical identity、ifindex、`eth<N>` reservation与membership | initial-domain `LogicalInterfaces` | driver/provider只携带opaque reservation token |
| protocol `InterfaceId`与mapping | global `DomainStack` | worker只持自己的 narrow pump port |
| IPv4 address、route、source/interface selection | `Ipv4ControlPlane` | 读取 committed logical member 与 SystemTarget deployment |
| worker admission与network shutdown | kernel attach authority | `PumpControl` 后接 device-local shutdown attempt |

### Handoff 与线性化点

1. Platform bus 按 DT discovery order 进入 matching-node probe。probe 的第一项网络动作是向
   logical owner 取得 unpublished external reservation；这个动作线性化 `eth<N>` ordinal。
2. IRQ resource layer 以 name `macirq` 解析 index，再按 interrupt parent 的 cell width 截取恰好
   一个 specifier；irqchip 只翻译该项，descriptor 发布后 IRQ core 才 unmask。
3. Provider 在 queue、RX refill、IRQ、cache/DMA proof 和 wake wiring 完成后，才将 ready
   capability连同 opaque logical reservation 移交 `device/net`。
4. Attach authority 建立 Stack mapping 和 inactive worker，最后在同一 authority transaction 中
   commit 原 reservation、记录 active path 并 activate worker。
5. Frame backing 的每次 CPU/device ownership transfer 以 descriptor ownership publication 或
   completion observation 为线性化点；cache operation 与 ordering fence 都必须在对应一侧完成。

### Failure 与 cleanup

- reservation 之后的 MAC、MMIO、IRQ、DMA 或 provider failure 必须 abort token；ordinal 保持已消费，
  不发布 logical member、netdev 或 Stack mapping。
- IRQ 注册前的失败释放 owner-local allocations。IRQ 注册后因当前没有 `free_irq`，provider 必须
  disable device-side interrupt/DMA 并保留仍可被 IRQ/device 访问的 context/backing 到 reset/power-off。
- publication 后 attach 失败按 `Stack mapping -> logical reservation` 顺序回滚，并保留 provider 为
  published/unattached；一个节点的失败不修改其它节点的 identity、mapping、worker 或 rings。
- orderly shutdown 保持 `filesystem -> network -> device -> PowerOff`。network 先关闭 worker admission；
  concrete driver 随后停止本节点 IRQ generation/DMA。未证明 device quiesce 前不得释放 backing。
- 配置选中的 `eth<N>` 不存在或未 commit 时 fail closed；不得选择另一个 active GMAC。

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

多 GMAC 可见语义是稳定 `eth<N>`：ordinal 由 matching-node DT discovery order 决定，不由成功
attach 顺序决定。失败节点留下编号空洞。`NetdevId`、logical identity/ifindex/name 与 protocol
`InterfaceId` 继续是不同 identity domain。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `IRQ-FLOW-001` | Refine | [controller flow 与 device cause handoff](../../contracts/interrupt/index.md#irq-flow-001---controller-flow与device-cause-handoff) | descriptor 建立前由 IRQ resource owner 按 name/index 唯一选择单项 firmware specifier；既有 controller/device cause 顺序不变 | `JH7110-GMAC-CUTOVER` |
| `NET-IFACE-DOMAIN-001` | Refine | [reservation 在 attach transaction 中消费 identity](../../contracts/net/interface-domain.md#net-iface-domain-001--initial-domain拥有logical-interface-namespace) | matching candidate 可在 probe admission 预留 logical identity；abort 仍不复用，driver只携带 token | `JH7110-GMAC-CUTOVER` |
| `NET-ATTACH-001` | Refine | [attach authority 创建并提交 reservation](../../contracts/net/attach-lifecycle.md#net-attach-001--attach-publicationrollback与best-effort-shutdown) | attach 可消费 publication 携带的既有 reservation；rollback、active publication 与 shutdown 顺序不变 | `JH7110-GMAC-CUTOVER` |

三项 Refine 只在最终板级验收通过后原子 cut over。Gate 0--3 的局部检查不提前修改 current
contract。

### Dependencies

- [NETDEV-LIFE-001](../../contracts/net/netdev-lifecycle.md#netdev-life-001--boot-time-identity与publication是单向transaction)：ready-before-publication 与 published/unattached。
- [NET-BOUNDARY-001 / NET-FRAME-OWN-001 / NET-FRAME-PROGRESS-001 / NET-STACK-PUMP-001](../../contracts/net/frame-path.md)：frame capability、唯一 backing owner、有界 backpressure 与唯一 Stack progression。
- [NET-CONTROL-PLANE-001](../../contracts/net/control-plane.md#net-control-plane-001--initial-domain唯一决定ipv4-routesourceinterface)：单一 SystemTarget interface/address/default route 选择。
- [SYSTEM-POWER-ORDERLY-001](../../contracts/power/shutdown-lifecycle.md#system-power-orderly-001)：network 先于 device shutdown。
- [STM-TARGET-001](../../contracts/configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract)：现有 `[network.ipv4]` 配置形状。

## Implementation Boundary

- **允许改变：** JH7110 net driver；按 name/index 选择单项 interrupt 的 FwNode/IRQ resource surface；
  RISC-V/JH7110 non-coherent DMA cache capability；logical reservation 的提前取得与跨 publication
  handoff；attach 对预留 token 的消费；owner-local tests、target config/Kconfig 与必要 module registration。
- **必须保持：** per-node provider 是 queue/DMA/IRQ truth 的唯一 owner；driver 不拥有 logical
  namespace、route 或 Stack；现有 SystemTarget schema、socket ABI、single-control-plane 语义、
  VirtIO 的可见单接口行为、frame callback boundary 和 shutdown participant order。VirtIO 的内部
  reservation handoff 可以迁移，但不得形成第二套编号规则。
- **实现提示：** 预计涉及 `driver/net`、`exception/intr/irq`、firmware-node DT parsing、`mm/dma`、
  `device/net` 与 `net/domain`；这些是非穷举提示，不是逐文件 write set。
- **验证 claim：** Gate 检查只证明 source/build/targeted semantics；JH7110 IRQ、cache、PHY handoff、
  双口收发和稳定跨启动 identity 只能由最终 VisionFive 2 acceptance 证明。
- **停止条件：** 任何证据要求 driver 接管 clock/reset/syscon/PHY owner，改变 SystemTarget 为多 IP，
  让 driver 保存 `eth<N>`/route/Stack truth，依赖 fence-only cache correctness，使用固定 GMAC 数组，
  改变 frame ownership，或降低最终板级验收时，必须回到 RFC review / Target Renegotiation。

## Acceptance 与 Validation

接受本 R0 RFC 只表示同意上述 target、owner、contract delta 与实施路线，不表示硬件能力已经
交付。实现 closure 必须依次满足：

1. Gate 0--3 全部实现，并在每个 Gate 后完成其 source audit、build、targeted tests 和
   Architecture Friction Scan。
2. 所有 Gate 完成后，才在 VisionFive 2 上执行[最终板级验收](./implementation.md#最终板级验收)。
3. 板级证据覆盖每个目标 node 的 MAC/MMIO/`macirq`、真实 RX/TX、ring wrap 与重复流量、稳定
   `eth<N>`、单接口 static IPv4、无 fallback、failure isolation 和 orderly shutdown。
4. 只有上述证据同时成立，才执行单个 `JH7110-GMAC-CUTOVER`、更新三项 current contract，并将
   RFC 关闭。

QEMU 可用于保护既有 VirtIO/network regression，但结果必须标记为 regression-only，不能计入本
RFC acceptance。当前 Draft 的 implementation、board runtime 与 contract cutover 均为 Not Run。

## 风险与反馈

最大风险是 JH7110 可用的 cache maintenance 机制、descriptor cache-line sharing、IRQ 注册后的
不可回收生命周期，以及提前 reservation 跨越 driver/device/net/attach owner 的 token handoff。对应
correctness obligations 见[目标与不变量](./invariants.md)，逐 Gate deliverable、检查和 hard stop 见
[实施路线](./implementation.md)。任何实现反馈若改变 target、owner、cleanup、contract delta 或
acceptance，必须先回到 RFC review，不能把较弱路径记作完成。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- [RFC 前定位材料](./positioning.md)（非规范）
- commit / PR / optional transaction：None
- 外部源码证据：None；硬件描述基线使用仓库跟踪的
  [`visionfive2-board.dts`](../../../../conf/platforms/visionfive2-board.dts)。

## 修订记录

当前为已接受的 `R0`；接受没有改变三项 current contract，也没有产生硬件通过结论。

## Closure

Not Closed。R0 已接受；Gate 0--3、最终板级验收与 current-contract cutover 均未完成。
