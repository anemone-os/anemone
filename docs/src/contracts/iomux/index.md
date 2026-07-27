# I/O Multiplexing 当前契约

**Owner：** iomux wait protocol / pollable source registration boundary
**覆盖范围：** `ppoll` / `pselect6` 的单轮 readiness scan、source-neutral persistent route、阻塞与 final recheck
**不覆盖：** Linux `pollfd` / `fd_set` ABI、具体 source predicate、scheduler wait-core completion、epoll watch/policy
**最后核验：** 2026-07-27

本目录只提取 epoll 后续设计会复用或改变的当前有效 iomux 规则，不枚举所有
pollable source，也不把 source-private 数据结构固定为长期 contract。

## Contract Surfaces

- [Poll wait 与 source registration](./poll-wait.md)：单轮 scan/subscribe/final-recheck、non-owning route gate 与 source hint publication。

## 邻接契约

- [Scheduler Latch wait round](../scheduler/latch-wait-round.md)：单轮 wait identity、producer capability 与 wait-core completion。
- [Opened-description lifecycle](../task/opened-description-lifecycle.md)：published fd identity 与 final-release 边界。
