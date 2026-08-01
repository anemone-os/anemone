# Membarrier 当前契约

**Owner：** membarrier global rendezvous protocol
**覆盖范围：** 最小 Linux membarrier ABI、全 CPU data-memory fence rendezvous 与 task-switch 局部义务
**不覆盖：** expedited registration、private scope、sync-core、rseq、CPU hotplug、延迟或性能保证
**最后核验：** 2026-07-29

本目录只登记已经生效的最小 global membarrier 规则，不声称 Anemone 已实现完整 Linux
`membarrier(2)` 命令族。

## Contract Surfaces

- [Global rendezvous](./global-rendezvous.md)：`QUERY` / `GLOBAL` ABI、同步 fence IPI、
  task-switch barrier 与失败边界。

## 邻接契约

- [Scheduler 当前契约](../scheduler/index.md)：wait/wake placement contract 不覆盖本协议使用的
  通用同步 IPI；membarrier handler 不建立反向 scheduler completion。
