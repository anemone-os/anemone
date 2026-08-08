# MM 当前契约

**Owner：** mm paging commit / user-fault local completion / frame-allocation accounting / OOM policy
**覆盖范围：** 本轮按触达提取的user-fault local TLB completion、global frame-pressure sampling、OOM victim policy与既有signal/exit handoff
**不覆盖：** mm全领域不变量、remote TLB shootdown、generic reclaim、swap、page cache/slab shrink、memcg、VMO通用lifecycle或用户RSS ABI
**最后核验：** 2026-08-08

本目录只登记已经迁移到contract层的共享规则，不声称枚举mm子系统全部不变量。

## Contract Surfaces

- [User Fault Local TLB Completion](./user-fault-local-tlb.md)：operation-local commit relation、access continuation与current-core completion。
- [OOM policy](./oom-policy.md)：worker-owned fixed-delay sampling、live frame stats、victim round与signal/exit handoff。

## 邻接契约

- [Task kthread timed wait](../task/kthread-wait.md)：stop/deadline完成、ordinary-wake重查和stale timeout isolation。
- [Build Configuration当前契约](../configuration/index.md)：KernelConfig materialization与kernel-consumer semantic validation。
- [Signal当前契约](../signal/index.md)：group-directed `SIGKILL` pending、notification与action selection。
- [Task当前契约](../task/index.md)：ThreadGroup terminal lifecycle与exit cleanup。
