# Socket 当前契约

**Owner：** general Socket front、concrete family ops、Unix endpoint/listener/connection/direction与pathname namespace各自拥有的state
**覆盖范围：** common Socket file/ABI/wait boundary、IPv4 UDP/ICMP raw/TCP与Unix stream/seqpacket的共同dispatch，以及filesystem pathname Unix connection-oriented Socket的state、namespace、address、data plane与lifecycle
**不覆盖：** IPv6、任意其它raw protocol、Unix datagram/abstract namespace、ancillary data、通用mutable option/error bag、socket timeout或async I/O
**最后核验：** 2026-08-06

本目录登记`SOCKET-UNIX-CUTOVER`、`ICMP-RAW-CUTOVER`、`SOCKET-UNIX-SEQPACKET-CUTOVER`及`NET-TCP-CUTOVER`已经生效的最小共享规则。它不建立名为Socket core的并列runtime owner：general front只拥有共同外壳和operation orchestration，UDP、ICMP raw、TCP与Unix仍分别拥有family-private事实。

## Contract Surfaces

- [Front、ABI 与 wait](./front-abi-wait.md)：`SOCKET-FRONT-001`、`SOCKET-ABI-001`与`SOCKET-WAIT-001`。
- [Unix state、stream、address 与 lifecycle](./unix-stream-lifecycle.md)：`UNIX-SOCKET-STATE-001`、`UNIX-SOCKET-STREAM-001`、`UNIX-SOCKET-ADDRESS-001`与`UNIX-SOCKET-LIFECYCLE-001`。
- [Unix pathname namespace](./unix-namespace.md)：`UNIX-SOCKET-NAMESPACE-001`。
- [Unix seqpacket](./unix-seqpacket.md)：`UNIX-SOCKET-SEQPACKET-001`。

## 邻接契约

- [Network Protocol Socket](../net/protocol-socket.md)：UDP、ICMP raw与TCP各自的protocol facts及共同wait handoff由network owner负责。
- [UDP Socket](../net/udp-socket.md)、[IPv4 ICMP Raw Socket](../net/icmp-raw-socket.md)与[IPv4 TCP Socket](../net/tcp-socket.md)：family Endpoint与packet/stream transaction由各自network owner负责。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：fd publication、status/flags sharing与semantic final release。
- [VFS creation与make node](../vfs/make-node.md)：pathname create admission、umask/final metadata与backend/dentry publication。
- [Poll wait](../iomux/poll-wait.md)与[Epoll protocol](../epoll/protocol.md)：source-neutral route、final predicate scan及RDHUP consumer projection。
