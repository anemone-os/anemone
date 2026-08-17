# Network 当前契约

**Owner：** `device/net` registry、initial-domain logical-interface owner、concrete frame provider、domain Stack与kernel attach authority各自拥有的network state
**覆盖范围：** boot-time netdev publication、initial-domain logical membership、static IPv4 control plane、UDP、ICMP raw与TCP Socket/Endpoint protocol、production local/external handoff、frame ownership/progress、bounded global-Stack pump、attach与terminal shutdown handoff
**不覆盖：** runtime IP reconfiguration、runtime hotplug/detach/restart、完整teardown、connected/IPv6 UDP、IPv6 TCP、任意其它raw protocol、hardware或SMP runtime guarantee
**最后核验：** 2026-08-15

本目录登记`net-frame-path` R1和`net-udp` R0已经cut over的最小共享规则，不声称枚举network领域全部不变量。
各surface明确自己的唯一状态或协议owner；`net`目录不是并列runtime owner，也不保存综合lifecycle truth。

## Contract Surfaces

- [Frame path](./frame-path.md)：`NET-BOUNDARY-001`、`NET-FRAME-OWN-001`、
  `NET-FRAME-PROGRESS-001`与`NET-STACK-PUMP-001`。
- [DWMAC concrete backend](./dwmac.md)：`DWMAC-NODE-001`、`DWMAC-DESCRIPTOR-001`、
  `DWMAC-DMA-ADDR-001`与`DWMAC-CAUSE-001`。
- [Netdev lifecycle](./netdev-lifecycle.md)：`NETDEV-LIFE-001`。
- [Interface domain](./interface-domain.md)：`NET-IFACE-DOMAIN-001`。
- [IPv4 control plane](./control-plane.md)：`NET-CONTROL-PLANE-001`。
- [Protocol Socket](./protocol-socket.md)：`NET-PROTOCOL-BOUNDARY-001`与`NET-SOCKET-WAIT-001`。
- [UDP Socket](./udp-socket.md)：`NET-SOCKET-ENDPOINT-001`与`NET-UDP-TRANSACTION-001`。
- [IPv4 ICMP Raw Socket](./icmp-raw-socket.md)：`NET-ICMP-RAW-INGRESS-001`、
  `NET-ICMP-RAW-ENDPOINT-001`与`NET-ICMP-RAW-TRANSACTION-001`。
- [IPv4 TCP Socket](./tcp-socket.md)：`NET-TCP-ENDPOINT-001`、`NET-TCP-STREAM-001`与
  `NET-TCP-LIFECYCLE-001`。
- [Attach lifecycle](./attach-lifecycle.md)：`NET-ATTACH-001`及与System Power的orderly shutdown handoff。

## 邻接契约

- [System Power shutdown lifecycle](../power/shutdown-lifecycle.md)：`power`唯一拥有terminal episode与
  `filesystem -> network -> device`全局顺序；network只拥有owner-local cleanup。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：semantic final release与fd alias publication。
- [IOMUX poll wait](../iomux/poll-wait.md)与[Epoll protocol](../epoll/protocol.md)：wait registration、cancellation与
  final readiness harvest；UDP、ICMP raw与TCP source只提供各自当前predicate与recheck hint。
