# ANE-CHG-20260803-pipe-dynamic-capacity

**Type:** Small feature / owner-local capacity cutover
**Status:** Completed
**Date:** 2026-08-03
**Authors:** doruche, Codex
**Area:** pipe / fcntl / Kconfig / ring buffer

## Problem / Context

匿名 pipe 过去用固定两页 backing 保存 bytes，再用独立 logical-capacity 字段实现
`F_SETPIPE_SZ`。因此缩容只改变 admission，扩容无法获得真实存储；backing 与用户可见容量也形成两份
需要同步的 truth。已有 `F_GETPIPE_SZ`、`F_SETPIPE_SZ(0)` 和 `FIONREAD` ABI 入口不足以证明实际扩容、
wrapped FIFO 搬运或失败不变性。

本轮选择 16 页作为每个匿名 pipe 的首个 Kconfig maximum。在 4 KiB page 下，它提供 64 KiB 的
真实最大容量；一次 resize 最多暂时持有约 128 KiB 的 old+candidate backing，并把 spin lock 内的
最坏 bulk copy 限制在 64 KiB。该局部上界可以在现有 pipe owner 内闭合，不引入资源账本、privilege
policy、procfs ABI 或新的 shared contract。

## Decision

`PipeInner::buf.capacity()` 同时是实际 backing 和用户可见容量的唯一真相源，不再保存独立 logical
capacity。resize 固定采用“锁外 fallible allocation、锁内复核/copy/swap、锁外 drop/notify”：只有
`mem::replace` 是容量与 FIFO 内容的发布点，并发 operation 只能观察完整旧 ring 或完整新 ring。

默认值继续由 `pipe_capacity_pages` 拥有；`pipe_max_capacity_pages = 16` 只定义单 pipe hard maximum。
xtask 只反序列化、物化默认值并生成内核常量，不判断两个参数的范围或关系。默认至少两页、default/max
均为 2 的幂、max 不小于 default，以及 page-to-byte 乘法可表示性，全部由 `fs::pipe` 的
`static_assert!`/const evaluation 在内核构建时验证。

`F_SETPIPE_SZ` 把请求向上规范化为至少一页且 page count 为 2 的幂：超过 signed return domain 返回
`EINVAL`，超过 Kconfig maximum 返回 `EPERM`，低于 unread bytes 返回 `EBUSY`，候选分配失败返回
`ENOMEM`，same-size 不分配。成功返回真实 byte capacity；一页 `PIPE_BUF` admission、FIFO、partial
progress、`O_NONBLOCK`、`SIGPIPE`、`FIONREAD` 与 endpoint lifecycle 保持不变。

## Change

- 在 `utils::ring_buffer` 增加 exact-capacity、fallible、构造后不分配的 `HeapRingBuffer<T: Copy>`，
  提供 bulk push/pop、FIFO iterator 和至多两个 readable slices；现有 static ring consumer 不迁移。
- pipe 改用 heap ring 并删除独立 capacity 字段；初始 backing 和 pipe owner 在 inode/file publication
  前 fallibly 创建，resize candidate 与 replaced backing 都在 pipe lock 外释放。
- `fs::pipe` 在同一 owner 内按 `mod`、`io`、`poll`、`capacity` 目录化。read/write predicate、endpoint
  count、poll registry 和 capacity 仍由同一个 `PipeInner` lock domain 拥有，没有扩大 public module
  surface。
- successful growth 在 commit 后发布 direct-writer Event recheck hint；只有 PIPE_BUF-writable predicate
  从 false 变为 true 时 snapshot 并通知 writer PollRoute，最终 readiness 仍由 source lock 下重算。
- `anemone-rs` 增加有实际 `fcntl-test` consumer 的窄 pipe-size/FIONREAD wrapper；`pipe-capacity` suite
  覆盖默认值、rounding/limit、64 KiB 真实扩容、wrapped grow/shrink、`EBUSY` failure atomicity、满 pipe
  `EAGAIN` 和 grow 后 `POLLOUT`。

本轮不实现 per-user soft/hard page accounting、`/proc/sys/fs/pipe-*`、`CAP_SYS_RESOURCE` override、
packet-mode pipe、named FIFO、page sharing、zero-copy 或其它 buffer consumer 迁移。

## Validation

- `just test xtask` 74/74 通过；pipe config 测试证明默认值机械物化/生成，并证明语义非法的
  default/max 组合仍由 xtask 原样生成、延迟到内核编译拒绝。`just fmt kernel --check`、
  `just fmt fcntl-test --check`、
  `just fmt user-test --check` 与 `git diff --check` 通过。
- `fcntl-test` 和 `user-test` 的 RV64、LA64 app build 通过；RV64、LA64 canonical kernel build 串行
  通过。sandbox 内 lwext4 C compile 的 `Bad system call`/`SIGSYS` 由完全相同命令在 sandbox 外成功
  复核为环境限制。
- RV64 focused runtime 中 392/392 KUnit 通过；`fcntl-test pipe-capacity` 的四组用例全部通过并正常
  关机，证明真实 64 KiB backing、FIFO 搬运、失败不变性和 grow 后 readiness。
- RV64 focused LTP 的 glibc/musl `pipe12` 各 6 项、`epoll_wait06` 各 9 项通过；总计
  `attempted=59 passed=57 failed=2 infra_failed=0 skipped=1`。两个 failure 都是 `pipe15` BROK，原因是
  缺少 `/proc/sys/fs/pipe-user-pages-soft`，没有归为本轮动态容量回归。
- 一位独立 agent 只读审查 Kconfig owner、ring soundness、resize 并发线性化、failure/cleanup、ABI/errno、
  Event/PollRoute 和测试 oracle，未发现 Apollyon、Keter、Euclid 或需记录的 Safe finding；review 只执行
  source/test/log 审计与 `git diff --check`，没有重新运行 build、KUnit 或 QEMU。
- LA64 runtime、hardware、依赖 pipe procfs controls 的 `fcntl30`、`fcntl35`、`fcntl37` 均 Not Run；
  专门的并发 resize stress、阻塞 writer/ppoll waiter 跨 grow 睡眠再唤醒也未运行。build、RV64、
  KUnit 或静态审查证据不替代这些边界。

## Remaining Risk / Links

- resize 是低频控制路径，但会在 pipe spin lock 下复制至多 64 KiB unread bytes。若未来提高 maximum，
  必须重新评估 old+candidate 内存和最长临界区，不能以 lock 外 snapshot、generation 或第二份 capacity
  truth 偷渡复杂度。
- 并发 resize 可以分别在锁外持有未发布 candidate；当前 hard maximum 与 allocator failure 不是 Linux
  per-user resource policy，也不声明 Linux page-slot/merge/fragmentation 实现等价。
- Current contract dependency：[I/O Multiplexing poll wait](../../contracts/iomux/poll-wait.md)；本轮不
  Introduce、Refine、Replace 或 Remove current contract ID。
- Register / limitations：[pipe procfs knobs](../../register/current-limitations.md#ane-20260528-pipe-procfs-knobs-stage1)、
  [splice family copy-backed stage 1](../../register/current-limitations.md#ane-20260617-splice-family-copy-backed-stage1)。
- Prior owner evidence：[Pipe Event wait](./2026-08-02-pipe-event-wait.md)。
- Runtime logs：`build/pipe-dynamic-capacity-rv64.log`、
  `build/pipe-dynamic-capacity-ltp-rv64.log`（build-local，未入库）
- RFC / transaction / external source：None
- Issue / PR / commit：this change's Git commit
