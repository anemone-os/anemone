# Netdev Lifecycle 当前契约

**Contract ID：** `NETDEV-LIFE-001`
**状态：** Active
**Owner：** `device/net` boot-time netdev registry与publication protocol
**参与领域：** generic bus / concrete NIC driver / frame provider / `device/net` / kernel attach authority
**覆盖范围：** boot-time netdev identity、normalized facts、frame-capability publication与published/unattached state
**不覆盖：** logical-interface identity/ifindex/name/membership、protocol `InterfaceId` mapping、active attach、runtime hotplug/unpublish/reuse、link control plane或route selection
**实现位置：** `anemone-kernel/src/device/net/`、`anemone-kernel/src/driver/net/`
**依赖：** [NET-BOUNDARY-001](./frame-path.md#net-boundary-001--frame-slice依赖方向与object-fence)、[NET-FRAME-OWN-001](./frame-path.md#net-frame-own-001--frame-backing只有一个访问owner)
**Pending Successor：** None
**最后核验：** 2026-07-31

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| generic device binding | generic bus / `Device` | driver capability | 是否绑定concrete driver |
| netdev identity与registry record | `device/net` | opaque published handle / immutable identity projection | boot内publication与attach input |
| logical identity、ifindex/name/kind与membership | initial-domain logical-interface owner | opaque netdev association | domain-local interface view；不由本契约拥有 |
| normalized publication facts | `device/net`，来源为provider observation | stable snapshot | publication-time描述；允许stale |
| queue、DMA、completion与current link/resource truth | concrete driver/provider | frame capability、recheck edge | owner-local hardware progression |

## NETDEV-LIFE-001 — boot-time identity与publication是单向transaction

**规则：** driver必须先完成owner-local queue、frame backing、RX refill、IRQ与notification准备，最后才把
ready capability一次性发布给`device/net`。registry是netdev identity与published handle的唯一owner；成功
identity在本次boot内稳定且不复用。它不分配或缓存logical-interface ifindex/name/membership。generic device
identity、netdev identity、domain-local logical identity/ifindex、stack-local `InterfaceId`与queue token互不等价。

published/unattached是合法durable fact。link-up不是publication或attach前提；link-down不撤销identity、frame
capability或stack mapping。registry保存的link是publication-time snapshot，允许stale且不得驱动runtime pump；
current link/resource truth仍由provider拥有，notification只请求重读。provider不能证明link时必须使用
unknown/unavailable，不得伪造成up。

**失败 / Cleanup：** publication前失败不留下registry entry。若IRQ registration或device state无法安全回滚，
driver可以抑制notification并保留owner-local resource到reset/power-off，但不得释放或复用device仍可能访问的
backing；generic bus无需因此取得netdev lifecycle truth。

**违反表现：** 先publish再准备queue/IRQ；失败留下半初始化entry；link-down重建identity；`device/net`重新分配
logical ifindex/name；netdev identity与logical/`InterfaceId`共用可解引用ID；snapshot反向驱动queue/pump；多个
registry缓存并列lifecycle truth。

**验证 / Enforcement：** registry KUnit覆盖不同origin/facts、单调netdev identity、duplicate failure isolation与
snapshot；source audit确认registry不再保存ifindex/name，VirtIO probe在queue/RX/IRQ/notification准备后publish且
失败不留entry；RV64/LA64 final fresh-disk运行分别证明virtio-mmio/virtio-pci publication进入后续active attach，
logical identity由独立domain KUnit与source audit验证。

**最初来源：** [Network Frame Path RFC R1](../../rfcs/net-frame-path/index.md)。

**当前来源：** [Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的
`NET-UDP-DOMAIN-CUTOVER`。

## 当前接受边界

- 第一版只支持boot-time persistent netdev；没有runtime unpublish、generation、stale-handle recovery或reuse。
- RV64 QEMU证明一张virtio-mmio NIC，LA64 QEMU证明一张virtio-pci NIC；多实例identity/failure isolation由host与
  registry KUnit证明。hardware、其它NIC/deployment与`smp>1`均Not Run。
- link facts没有control-plane subscription或用户ABI；snapshot只表示publication point。
