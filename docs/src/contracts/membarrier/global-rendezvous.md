# Global Membarrier Rendezvous 当前契约

**Contract ID：** `MEMBARRIER-GLOBAL`
**状态：** Active
**Owner：** membarrier global rendezvous protocol
**参与领域：** Linux syscall ABI / IPI transport / scheduler context switch / architecture memory ordering
**覆盖范围：** `MEMBARRIER_CMD_QUERY`、`MEMBARRIER_CMD_GLOBAL` 与成功返回所需的全 CPU full data-memory barrier
**不覆盖：** Linux RCU / `nohz_full` 实现形状、expedited / private / sync-core / rseq / registrations query、CPU hotplug等价语义、实时性或性能
**实现位置：** `anemone-kernel/src/syscall/membarrier.rs`、`anemone-kernel/src/exception/ipi.rs`、`anemone-kernel/src/sched/mod.rs`、`anemone-kernel/src/sync/mod.rs`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-29

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| 支持的 Linux command bit 与参数解释 | syscall membarrier adapter | 无 | 保证 `QUERY` 不公布未实现能力 |
| IPI message queue、payload lifetime 与 completion | IPI transport | syscall 只提交无状态 `MemoryBarrier` capability 并等待结果 | 让每个远端在线 CPU 在确认前执行 full fence |
| outgoing-to-incoming task execution boundary | scheduler | membarrier 不持有 task、runqueue 或 mapping snapshot | 覆盖未运行、换出和迁移线程与 IPI 的交错 |

本协议没有 persistent barrier round、per-task、per-mm 或 per-runqueue membarrier 状态。

## MEMBARRIER-GLOBAL-001 — 成功的 GLOBAL 建立全 CPU full-fence rendezvous

**规则：** `MEMBARRIER_CMD_QUERY` 只能返回 `MEMBARRIER_CMD_GLOBAL` bit；`flags != 0`、
未知命令和其它 Linux membarrier 命令必须返回 `EINVAL`。`MEMBARRIER_CMD_GLOBAL` 必须按
以下顺序执行：调用 CPU full data fence；向所有其它在线 CPU 同步广播无状态
`MemoryBarrier` IPI；每个 handler 在发布 completion 前执行 full data fence；调用方等待
全部 completion；调用 CPU 再执行 full data fence。scheduler 必须在旧 task 已停止、下一
task 尚未恢复的每个统一切换边界执行同一个 full data fence，包括 mapping 相同的切换。

只有完整 rendezvous 后才能返回 0。IPI allocation failure 返回 `ENOMEM`，online topology
竞态返回 `EAGAIN`；错误返回不宣称 barrier 完成。`cpu_id` 在唯一支持的 flags=0 命令中被
忽略。RV64 full fence 必须生成为 `fence rw, rw`，LA64 SMP ordering fence 必须生成为
`dbar 16` (`orwrw`)；`sfence.vma`、`fence.i` / `ibar` 不能替代 ordinary data fence。

**违反表现：** `QUERY` 公布 expedited/private/rseq bit；空 IPI 在没有远端 full fence 时确认；
调用方缺少前置或后置 fence；同 mapping task switch 绕过 scheduler fence；handler 获取
scheduler lock、分配、调度或发起反向同步 IPI；部分 IPI 失败后仍返回成功。

**验证 / Enforcement：** syscall command/flags KUnit；SMP=2 live global rendezvous KUnit；
IPI payload broadcast-copy KUnit；syscall/IPI/scheduler source audit；RV64/LA64 release build 与
action-local disassembly 检查。用户态 syscall wrapper、LTP 与 CPU-hotplug matrix 不属于当前
cutover evidence。

**最初来源：** [Minimal global membarrier 小迭代](../../devlog/changes/2026-07-29-minimal-global-membarrier.md)。

**当前来源：** [Minimal global membarrier 小迭代](../../devlog/changes/2026-07-29-minimal-global-membarrier.md)。

## 跨领域局部义务

| Obligation ID | 参与方 | 必须完成的动作 | Handoff / 线性化点 | 失败 / Cleanup 责任 |
| --- | --- | --- | --- | --- |
| `MEMBARRIER-GLOBAL-001A` | syscall adapter | 验证命令/flags，在 synchronous broadcast 前后执行 local full fence | post-fence 完成后才返回 0 | 映射 IPI error；没有 persistent round 需要 rollback |
| `MEMBARRIER-GLOBAL-001B` | IPI transport / handler | enqueue-before-ring；远端 full fence 后发布已有 completion | handler completion Release / caller Acquire observation | transport 唯一释放 message；handler 不获取其它 owner 状态 |
| `MEMBARRIER-GLOBAL-001C` | scheduler | 每个 old-stopped / next-not-resumed 边界执行 full fence | `local_pick_next()` 后、mapping/context restore 前 | 无额外状态或 cleanup |
