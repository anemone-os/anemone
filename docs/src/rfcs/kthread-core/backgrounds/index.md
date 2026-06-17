# KThread Core 背景材料

本目录保存 [RFC-20260616-kthread-core](../index.md) 的历史上下文和事实依据。背景材料不覆盖 canonical 结论。

## 与 v0 的关系

早期 v0 草案的主要价值是记录 “kthread 应进入 procfs-visible topology，并有 kthreadd anchor” 这个方向；但它仍包含现在已被否定的独立 registry、park/unpark、closure-first 等倾向。

当前 RFC 是 corrective successor，不是 v0 的小修。

## 本轮收敛结论

1. kthread 使用 Linux 风格 singleton thread group leader 模型，每个 kthread TG 只有 leader 自己。
2. 必须有显式 kthread-aware topology type 区分 user process 与 kthread；`kthreadd` 的特殊身份由 `Tid::KTHREADD` 派生，不单独编码为 topology type。
3. kthread 不加入 ordinary process group / session；job-control 相关字段第一版是 inert procfs view。
4. kthread exit 必须走专用路径，不复用完整 user-process `kernel_exit()`。
5. kthread exit 后撤销 procfs-visible topology，不保留 user-visible zombie。
6. external lifecycle handle 是 strong control handle，不是 weak-only ref。
7. 不引入独立 `KThreadRegistry` / `KThreadId`。
8. `KThreadService` 不属于 core；当前 consumer 用 explicit loop 即可。
9. park/unpark 从 core 中拆除。
10. `wake()` 保留为纯 wake capability，不表达业务 request truth。
11. monomorphic function + `AnyOpaque` start argument entry API 进入第一阶段；closure builder 和泛型 owned payload API 是 optional follow-up。
12. 当前 RFC slug 为 `kthread-core`，旧 `kthread` RFC 标为 historical baseline / superseded。
13. `kthreadd` TID/TGID 必须固定为 2，使用 explicit reserved handle 保证。

## 当前代码事实

- legacy `anemone-kernel/src/task/kthread/create.rs` 已有 `kthreadd` create queue、completion 和 typed start exactly-once reclaim；create queue 和 completion 是可保留资产，typed reclaim 只作为历史事实，纠偏实现改为 `AnyOpaque` start payload 与 task-local launch slot。
- ordinary kthread 当前通过 `TaskBinding::Leader` 发布，并继承 `kthreadd` 的 `pgid/sid`。这证明 singleton leader 方向可用，但 type 和 PG/session 边界错误。
- `anemone-kernel/src/task/kthread/mod.rs` 已把 kthread lifecycle state 与 `TaskSchedState` 分离，这是可保留资产。
- `anemone-kernel/src/task/kthread/service.rs` 是上层 service/request 设施，不应留在 core contract。
- `anemone-kernel/src/task/api/exit/mod.rs` 的 `kernel_exit()` 当前仍处理 clear-child-tid、robust futex、thread group cleanup、reparent、child-exited event、vfork completion 等 user-process 语义。
- `/proc` root readdir 通过 `for_each_thread_group_from()` 枚举动态数字目录。
- `/proc/<tgid>/status` 已输出 `Kthread:` 字段。
- `/proc/<tgid>/cmdline` 对无 userspace task 返回空。
- inode shrinker 与 OOM killer 当前都能用 explicit loop 表达业务 state，不需要 `KThreadService`。

## `kthreadd` TID 2 事实

legacy code 不保证 `kthreadd` 是 TID 2：

- TID allocator 从 1 开始普通分配。
- BSP `kinit` 创建 root task 时消耗 TID 1。
- AP `kinit` 在 `INIT_SYNC_COUNTER` 后也调用 `Task::new_kernel()`，可能在 `init_kthreadd()` 前消耗 TID 2。
- `init_kthreadd()` 当前没有 reserved TID，也没有 assert `tid == 2`。

因此固定 TID 2 必须通过 TID allocator reserved handle 保证。

## 文档接续

本次提升后的公开文档边界：

- [RFC-20260616-kthread-core](../index.md) 是后续纠偏实现的 canonical source。
- 旧 [RFC-20260614-kthread](../../kthread/index.md) 标为 `Superseded` / historical baseline。
- `docs/src/rfcs.md` 只做轻量导航，说明 `kthread-core` 是 corrective authority，旧 `kthread` 是当前实现历史记录。
