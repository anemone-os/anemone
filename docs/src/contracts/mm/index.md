# MM 当前契约

**Owner：** mm paging commit / user-address-space TLB completion / kernel heap / frame allocation / OOM policy
**覆盖范围：** 本轮按触达提取的user-address-space local/remote TLB completion、kernel heap layout/slab/backing handoff、frame placement与global pressure accounting、OOM victim policy与既有signal/exit handoff
**不覆盖：** mm全领域不变量、generic reclaim、swap、page cache/slab reclaim、memcg、VMO通用lifecycle、CPU hotplug/ASID或用户RSS ABI
**最后核验：** 2026-08-16

本目录只登记已经迁移到contract层的共享规则，不声称枚举mm子系统全部不变量。

## Contract Surfaces

- [User Address-Space TLB Completion](./user-fault-local-tlb.md)：operation-local commit relation、current-core completion、destructive remote ordering与retirement lifetime。
- [Kernel Heap](./kernel-heap.md)：small/large layout domain、slab span publication、current-CPU cache与bounded central exchange。
- [Frame Allocation](./frame-allocation.md)：RAII final release、global pressure truth、current-CPU order-0 magazine与admitted miss recovery。
- [OOM policy](./oom-policy.md)：worker-owned fixed-delay sampling、live frame stats、victim round与signal/exit handoff。

## 邻接契约

- [Task kthread timed wait](../task/kthread-wait.md)：stop/deadline完成、ordinary-wake重查和stale timeout isolation。
- [Build Configuration当前契约](../configuration/index.md)：KernelConfig materialization与kernel-consumer semantic validation。
- [Signal当前契约](../signal/index.md)：group-directed `SIGKILL` pending、notification与action selection。
- [Task当前契约](../task/index.md)：ThreadGroup terminal lifecycle与exit cleanup。
