# JH7110 GMAC 多节点接入定位

**状态：** Background / Archived Pre-RFC / Not Normative
**最后更新：** 2026-08-08
**范围：** JH7110 GMAC0/GMAC1 以及未来同形 GMAC DT 节点的 boot-time 网络接入方案

**正式提案：** [RFC-20260808-jh7110-gmac](./index.md)

> 本文保留 RFC 形成前的定位材料，不是 current contract、RFC target、implementation plan 或代码
> 实现授权。规范性目标、owner、gate 与 acceptance 以正式 RFC 为准；本文中的历史判断不得覆盖
> 正式 RFC 或 live source。

## 文档目的

JH7110 的两个 GMAC 节点使用同一个 DWMAC/StarFive 设备形状，但当前网络驱动只有 VirtIO-Net
concrete backend。本文固定一条最小、可审查的方向：

- 每个 DT GMAC 节点独立匹配和 probe，不建立 GMAC bus；
- 所有匹配节点都可以拥有独立的 queue、DMA、IRQ、frame provider、netdev identity 和 worker；
- 复用现有 `device/net`、`FrameProvider`、global `DomainStack` 和 boot-time attach；
- 假定固件已经完成 clock、reset、syscon、RGMII 和 PHY 的板级初始化；
- 以 JH7110 非一致性 DMA 的真实 cache maintenance 作为进入 active path 的硬 gate；
- 沿用现有 SystemTarget 的单个 `[network.ipv4]` 配置，只给被选中的 `eth<N>` 分配 IPv4，
  其它 GMAC 可以 active 但不获得 L3 route；
- 不在本轮引入多 IPv4 配置、多默认路由、DHCP 或 runtime 网络配置 ABI。

## 当前事实

JH7110 VisionFive 2 的设备树同时描述 GMAC0 和 GMAC1。GMAC 节点包含独立的 MMIO、clock/reset
引用、多个中断、RGMII 配置和 PHY 子节点，见
[`visionfive2-board.dts`](../../../../conf/platforms/visionfive2-board.dts)。运行时 DTB 已观察到
`ethernet@16040000` 带有 `local-mac-address`；正式 RFC 仍需分别确认每个目标节点都能得到
有效 MAC，不能只根据一个节点的日志推导另一个节点。

现有网络框架已经支持多个 boot-time netdev 的方向：`device/net` 拥有 netdev identity，
initial domain 拥有 logical-interface namespace 和一个 global Stack，每个 worker 通过窄的
per-interface pump port 推进自己的 mapping。当前 IPv4 control plane 仍只有一个
`StaticIpv4Deployment`，其配置形状来自
[`qemu-virt-la64-final.toml`](../../../../conf/system-targets/qemu-virt-la64-final.toml)：

```toml
[network.ipv4]
interface = "eth0"
address = "10.0.2.15"
prefix = 24
default-gateway = "10.0.2.2"
```

因此“多个 GMAC”在本定位中首先表示多个 L2/frame path 和独立 attach，不表示每个 GMAC 自动
获得一个 IP。

## 目标方案

### Per-node discovery

platform discovery 产生的每个 `PlatformDevice` 独立与 GMAC driver 的 match table 比较。driver
匹配 JH7110 GMAC compatible 后，为该 node 建立一个 concrete provider；不引入 GMAC bus、全局
GMAC registry 或按实例分支的第二套设备框架。

每个 provider 只拥有自己的：

- MMIO register mapping；
- MAC/DMA state；
- RX/TX descriptor ring 和 frame backing；
- IRQ private context；
- current link/resource truth；
- `FrameProvider` token 和 recheck edge。

provider 不拥有 socket、route、fd、`InterfaceId`、ifindex/name 或其它 GMAC 的状态。

### Firmware handoff

本目标不实现 JH7110 通用 clock/reset/syscon/PHY framework。每个目标 node 的固件 handoff 是
probe 前提：

```text
firmware
  -> clock enabled
  -> reset deasserted
  -> RGMII/syscon path selected
  -> board PHY initialized

GMAC driver
  -> consume MAC/DMA/IRQ resources
  -> establish frame ownership
  -> publish netdev
```

driver 仍需读取和校验能由 DT 表达的 mode、MAC、MMIO 和中断事实。不能因为固件 handoff 而把
link-up 伪造成已知事实；无法证明时使用 `Unknown`。

### IRQ

JH7110 节点可以同时列出 `macirq`、Wake 和 LPI IRQ。首版只需要 `macirq`。当前
`request_irq()` 没有按 `interrupt-names` 或 index 选择单项的能力，而 PLIC parser 要求单个
interrupt cell；正式 RFC 必须扩展一个窄的按 index/name 选择接口，或明确修改目标 DTS 只暴露
`macirq`。

推荐的长期方向是 IRQ owner 提供：

```text
request_irq(device, interrupt_index_or_name, handler, private_data)
```

driver 只能选择自己节点的中断，不能直接进入 PLIC map/domain。IRQ handler 只确认硬件事件、
提交 durable recheck predicate 并发出 wake edge；协议推进仍由 worker 完成。

### Non-coherent DMA gate

JH7110 不是 DMA-coherent，不能复用 VirtIO-QEMU 的 fence-only 假设。进入 netdev publication
前必须证明：

- descriptor 和 frame backing 的 DMA 地址、对齐和 ownership；
- CPU -> device 的 cache clean/flush；
- device -> CPU 的 cache invalidate；
- RX refill、TX submit、completion 和错误回收的 cache 顺序；
- queue exhaustion、completion 和 provider failure 不会释放仍可能被设备访问的 backing。

现有 [`DmaRegion`](../../../../anemone-kernel/src/mm/dma.rs) 的 `sync_for_device()` /
`sync_for_cpu()` 目前只有 fence，因此它不是本目标的完成实现。该 gate 未关闭前，最多允许
DT/resource probe 或 owner-local hardware characterization，不允许 active frame path、
`publish()` 或真实 Stack attach。

### Frame path 和 attach

通过 DMA gate 后，GMAC provider 遵循现有 frame contract：

1. 完成 queue、backing、RX refill、IRQ 和 recheck wake 准备；
2. 实现 `FrameProvider` 以及 kernel-local `NetdevFrameProvider`；
3. 用该 node 的 origin、MAC、frame capacity 和 provider 发布 `ReadyNetdev`；
4. 由 attach authority 为该 provider 建立独立 logical reservation、Stack mapping、worker 和
   pump port；
5. 只有 mapping、worker、wake、time wiring 全部准备后才发布 active path。

一个 GMAC 的 probe、DMA、worker 或 attach 失败只保留它自己的 published/unattached 状态，不能
回滚或改变其它 GMAC 的 identity、mapping 或 worker。

## 多 GMAC identity 与 IPv4 选择

### Physical identity

`NetdevId` 仍由 `device/net` 分配；`InterfaceId` 仍由 global `DomainStack` 分配；两者不等价。
GMAC 的物理 node identity 使用稳定的 DT node origin/path，不能用 `NetdevId`、`InterfaceId`
或发现时的临时地址作为配置主键。

### Logical `eth<N>` identity

现有 SystemTarget 用 `interface = "eth0"` 选择网络接口。为保持该配置语义，同时支持多个
GMAC，`eth<N>` 必须在 per-node discovery/identity reservation 阶段按 DT 稳定顺序消费，而不能
在“成功 attach 的设备序列”中重新编号：

- 第一个 DT GMAC 对应固定 ordinal `eth0`；
- 第二个 DT GMAC 对应固定 ordinal `eth1`；
- 某个 GMAC attach 失败时保留它已经消费的 identity，不允许后续 GMAC 顶替编号；
- identity 不复用；
- logical identity、netdev identity 和 protocol `InterfaceId` 仍由各自 owner 保存，不能
  合并成一个综合记录。

这要求正式 RFC 重新解析当前 `LogicalInterfaces::reserve_external()` 的时机；不能仅在
driver 中复制一份 `eth<N>` 映射。

### IPv4 deployment

第一版直接沿用现有单接口配置：

- `network.ipv4.interface` 指向一个稳定的 `eth<N>`；
- `address` / `prefix` 只投影到该 interface；
- `default-gateway` 只产生一个 default route；
- 其它已 attach GMAC 保持无 IPv4 的 active L2 path；
- 配置 interface 缺失、重复或与已提交 logical member 不匹配时 fail closed；
- 不自动选择另一个 GMAC，不因 route 缺失而 fallback 到其它节点。

多 GMAC 多 IP、多 default route、metric、DHCP、runtime route/address change 和用户配置 ABI
留给后续独立 control-plane RFC，不作为本目标的隐含能力。

## Ownership 与 handoff

| 状态/能力 | 唯一 owner | 其它参与方持有 |
| --- | --- | --- |
| DT node match 与 provider construction | concrete JH7110 driver | platform `Device` binding |
| queue、DMA、descriptor、completion、current link | per-node provider | frame token、recheck edge |
| netdev identity 与 publication record | `device/net` | immutable snapshot、published capability |
| `eth<N>` membership/ordinal | initial-domain logical owner | reservation/immutable snapshot |
| `InterfaceId` 与 smoltcp mapping | global `DomainStack` | per-interface pump port |
| IPv4 address/route/source selection | `Ipv4ControlPlane` | immutable selection result、pump wake |
| worker admission、shutdown barrier | kernel attach authority | narrow `PumpControl` |

没有一个 GMAC driver 可以同时拥有 netdev identity、logical ordinal、IP route 或 Stack object。

## 主要 gates

### Gate 0：DT、IRQ 和 firmware handoff

- 固定每个 JH7110 GMAC 的 compatible、MMIO、MAC、RGMII mode 和 `macirq` 来源；
- 确认每个运行时 node 的 `local-mac-address`；
- 选择 IRQ 按 index/name 扩展或 DTS 单 IRQ 方案；
- 证明 firmware clock/reset/syscon/PHY handoff 对每个 node 都成立。

### Gate 1：Non-coherent DMA

- 建立真实 cache maintenance 和 descriptor/frame ownership proof；
- 通过 deterministic descriptor、cache、exhaustion、completion 和 unwind tests；
- Gate 未通过时不发布 active netdev。

### Gate 2：Per-node provider 与稳定 identity

- 多节点独立 provider/IRQ/wake/ring；
- 稳定 DT order -> `eth<N>` reservation；
- 一个 node 失败不影响其它 node；
- host topology 验证 one-Stack/multi-provider isolation。

### Gate 3：Production frame attach

- 多 GMAC 的 RX/TX、worker、Stack mapping 和 current single-interface IPv4 deployment；
- 至少一个 GMAC active、其它 GMAC attach 或明确保持 unattached；
- 实机证明多节点独立 progression 和严格 shutdown order。

## 验收下界与停止条件

最低验收包括：

- DT 中任意数量、但受内存和 identity domain 限制的 GMAC 节点可以逐个 discover；
- 节点资源、MAC、IRQ、DMA backing 和 identity 不串线；
- 一个节点失败不改变其它节点的 `NetdevId`、`eth<N>`、mapping 或 worker；
- 选中的 `network.ipv4.interface` 获得正确地址、prefix 和唯一 default route；
- 非一致性 DMA 在 RX/TX completion 和资源回收中没有可观察 stale-cache 或 use-after-submit；
- 真实硬件上至少证明两个 GMAC 的独立 frame path，或记录未运行的硬件边界。

出现以下任一情况必须停止并回到 RFC review：

- 需要改变 `FrameProvider` 的 owner/lifetime 语义；
- 需要让 driver 拥有 logical identity、route 或 Stack state；
- DMA cache correctness 只能依赖未验证的 fence 或 firmware 口头假设；
- 需要把多 IP、多 default route 或 DHCP 偷渡进单接口 `[network.ipv4]` 配置；
- 需要 runtime detach/retry/restart 或扩大为通用 DWMAC/PHY/clock framework。

## 依赖与潜在 contract impact

当前定位依赖但不提前修改：

- [NET-BOUNDARY-001 / frame path](../../contracts/net/frame-path.md)；
- [NETDEV-LIFE-001](../../contracts/net/netdev-lifecycle.md)；
- [NET-IFACE-DOMAIN-001](../../contracts/net/interface-domain.md)；
- [NET-CONTROL-PLANE-001](../../contracts/net/control-plane.md)；
- [NET-ATTACH-001](../../contracts/net/attach-lifecycle.md)。

正式 RFC 可能需要明确的 contract delta：

- `NET-IFACE-DOMAIN-001`：将 logical `eth<N>` ordinal 的 reservation 从成功 attach 时机前移到
  稳定 per-node discovery/identity transaction；
- platform IRQ contract：增加按 interrupt index/name 选择的 `request_irq` 能力；
- `NET-FRAME-PROGRESS-001`：在不改变 frame owner 语义的前提下，纳入 JH7110 non-coherent DMA
  cache maintenance proof；
- 若未来要求多个 GMAC 同时拥有 IP，再单独 Refine `NET-CONTROL-PLANE-001`，不在本定位中
  预先登记多 IP 或多 default route。

本文不触发任何 contract cutover，也不授权进入上述 gate 的实现。
