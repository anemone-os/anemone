# ANE-CHG-20260802-pipe-event-wait

**Type:** Bugfix / internal wait-path correction
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** pipe / scheduler Event / iomux source notification

## Problem / Context

匿名 pipe 的 blocking read/write 已有明确的 source predicate，却在 predicate 不满足时释放
pipe lock、调用 `yield_now()`，再重新取锁检查。该循环不会丢失 pipe 状态变化，但会让本应
睡眠的 task 持续保持 runnable。LTP 的功能结果即使通过，也不能区分这种 yield loop 与真实的
source wait。

pipe 同时保存 `PollRoute`，为 `poll`、`select` 与 `epoll` 提供 persistent recheck route。
该路径属于 iomux composition，不能兼作直接 read/write 的 completion truth。本轮需要长期记录
Event 与 PollRoute 的职责分离、source predicate owner、锁外 publication 和 signal failure
mapping；这些边界可在一个 owner-local checkpoint 内闭合，因此不建立 RFC。

## Decision

匿名 pipe 的直接 blocking I/O 使用 interruptible `Event` 等待。Event wake 只表示相关 pipe
状态可能变化，调用者必须在 pipe owner 下重新计算 predicate；Event 不保存或传输 readiness、
可用 byte 数、endpoint count、errno 或 operation result。现有 `PollRoute`、snapshot/register/
final-scan 和 poll predicate 保持不变。

`PipeInner` 唯一拥有 buffer、logical capacity、reader/writer count 与 poll route registry。
外层 `Pipe` 只组合该 lock domain 和 read/write recheck Event。reader predicate 是 buffer 非空
或最后一个 writer 已关闭；writer predicate按当前请求长度决定：atomic request需要完整空间，
larger request只需要至少一个可写 byte，二者都在最后一个 reader关闭时继续。

每个 Event listener都使用 register-then-recheck，并在相关 state commit后 wake all；不同 writer
拥有不同 request length，reader也会竞争同一批 bytes，因此 wake payload或 listener quota不能
代替 owner admission。read/write commit、最后 endpoint close与 capacity increase均先提交
`PipeInner` truth，释放 pipe guard和 read operation gate，再 publish Event和 notify PollRoute。

## Change

- `Pipe` 调整为 `Arc<Pipe>`，其中 `SpinLock<PipeInner>` 保存原有全部可变 state，两个 `Event`
  分别服务 direct read/write recheck。
- `pipe_rx_read()`、`pipe_rx_read_user_transaction()` 与 `pipe_tx_write()` 移除 `yield_now()` loop，
  改为 operation-local predicate上的 interruptible `Event::listen()`；signal/force继续映射现有
  `EINTR`。
- 两条 read path都不会持有 `PipeRx::operation` 入睡；竞争 reader取得该 gate后重新检查 buffer
  与 writer count，实际消费后释放 gate再通知 writer。
- read/write progress、最后 endpoint close和 successful capacity increase补齐对应 Event hint；
  既有 poll route snapshot、registration、pruning与 guard-out notification保持原语义。

本轮只处理匿名 pipe。named FIFO/VFS open handoff、Event/wait-core/Latch API、pipe ABI与 errno、
signal/partial-progress、poll readiness、packet mode、dynamic backing、per-user accounting与 splice
能力均未改变；没有新增 probe、generation、cached readiness、通用 wait abstraction或第二套
listener registry。

## Validation

- `just fmt kernel --check` 与 `git diff --check`通过；残余 source搜索确认 `fs::pipe` 不再引用
  `yield_now()`、裸 `schedule()`、wait-core private type或 `Latch`。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`通过。sandbox内 lwext4
  C compile命中 `Bad system call`；完全相同命令在 sandbox外通过，归类为 host seccomp限制。
- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`通过；双架构 build串行
  执行，未把共享 `build/generated/device-tree/platform.dtb` 的并行结果作为证据。
- RV64 implementation candidate经仓库端到端 wrapper运行 focused pipe/iomux/epoll profile：kernel
  334项 KUnit全部通过；glibc为39项中38项通过，musl为38项中37项通过、1项 skipped，合计
  `attempted=77 passed=75 failed=2 infra_failed=0 skipped=1`。两个失败均为既有 `pipe15`，原因是
  缺少 `/proc/sys/fs/pipe-user-pages-soft`；两套 libc 的 iomux均9/9，epoll无失败，也未出现
  timeout、hang或panic。
- runtime之后只做了注释修正和删除 zero-length kernel-buffer read的无意义 writer hint；最终
  双架构 build已重跑，focused runtime未重复执行。LA64 runtime、专门的多reader/不同长度writer
  stress与 hardware均 Not Run。
- 一位独立 agent只读 change review检查 Event live semantics、三条 wait predicate、read operation
  gate、endpoint/capacity notification与 IOMUX current contract，未发现 Apollyon、Keter、Euclid
  或需要记录的 Safe finding；review未重新运行 build或QEMU。

`pipe2_04` 所需的 `/proc/<pid>/stat` sleeping-state oracle仍缺失，因此“不再 runnable busy loop”
由三条实际 Event call path、残余搜索和锁序审计证明，不外推 CPU utilization、调度 fairness或
task-state runtime结果。

## Remaining Risk / Links

- wake-all可能带来有限 thundering herd，但保持不同 request predicate与 owner recheck最直接；若
  将来基于性能证据改为 exclusive wake，需要先证明剩余-ready级联，不能让 quota成为 truth。
- 稀有的多reader/不同长度writer调度交错尚无专门 stress；当前 correctness依赖 Event的
  register-then-recheck和 `PipeInner` lock下的最终 admission。
- named FIFO需要独立处理 per-inode live session、blocking open、signal rollback与 unlink/reopen
  lifecycle，不属于本记录的后续自动 gate。
- Current contract dependency：[I/O Multiplexing poll wait](../../contracts/iomux/poll-wait.md)；本轮
  不 Introduce、Refine、Replace或Remove current contract ID。
- Register / limitations：[pipe procfs knobs](../../register/current-limitations.md#ane-20260528-pipe-procfs-knobs-stage1)、
  [splice family copy-backed stage 1](../../register/current-limitations.md#ane-20260617-splice-family-copy-backed-stage1)。
- RFC / transaction / external source：None
- Runtime log：`build/pipe-event-wait-rv64.log`（build-local，未入库）
- Issue / PR / commit：this change's Git commit
