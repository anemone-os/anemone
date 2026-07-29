# Network 当前契约

**Owner：** `device/net` registry、initial-domain logical-interface owner、concrete frame provider、domain Stack与kernel attach authority各自拥有的network state
**覆盖范围：** boot-time netdev publication、initial-domain logical membership、static IPv4 control plane、production local handoff、frame ownership/progress、bounded global-Stack pump、attach与terminal shutdown handoff
**不覆盖：** endpoint/socket/fd/readiness、runtime IP reconfiguration、runtime hotplug/detach/restart、完整teardown、remote external UDP proof、LA64/virtio-pci或SMP shutdown guarantee
**最后核验：** 2026-07-29

本目录登记`net-frame-path` R1和`net-udp` R0 Stage 1-2已经cut over的最小共享规则，不声称枚举network领域全部不变量。
各surface明确自己的唯一状态或协议owner；`net`目录不是并列runtime owner，也不保存综合lifecycle truth。

## Contract Surfaces

- [Frame path](./frame-path.md)：`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、
  `NET-FRAME-PROGRESS-001`与`NET-STACK-PUMP-001`。
- [Netdev lifecycle](./netdev-lifecycle.md)：`NETDEV-LIFE-001`。
- [Interface domain](./interface-domain.md)：`NET-IFACE-DOMAIN-001`。
- [IPv4 control plane](./control-plane.md)：`NET-CONTROL-PLANE-001`。
- [Attach lifecycle](./attach-lifecycle.md)：`NET-ATTACH-001`及与System Power的orderly shutdown handoff。

## 邻接契约

- [System Power shutdown lifecycle](../power/shutdown-lifecycle.md)：`power`唯一拥有terminal episode与
  `filesystem -> network -> device`全局顺序；network只拥有owner-local cleanup。
