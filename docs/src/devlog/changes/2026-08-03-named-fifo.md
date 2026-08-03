# ANE-CHG-20260803-named-fifo

**Type:** Small feature / contract-bearing local cutover
**Status:** Completed
**Date:** 2026-08-03
**Authors:** doruche, Codex
**Area:** VFS / inode / pipe / openat / iomux / LTP

## Problem / Context

ext4 与 ramfs 已能创建、保存和重新加载 `S_IFIFO` inode，但 filesystem-backed FIFO 的
`open(2)` 仍返回 `EOPNOTSUPP`。匿名 pipe 已拥有 bounded byte buffer、`PIPE_BUF` admission、
blocking/nonblocking I/O、`SIGPIPE` / `EPIPE`、poll route、`FIONREAD` 与动态容量；为 named FIFO
复制第二套 data plane 会让 buffer、participant、readiness 和 teardown 出现并列 truth。

另一个前置问题是旧 `openat` 顺序先调用 backend `InodeOps::open`，再完成 final type、DAC、status
与 mount admission。若 backend open 直接加入 FIFO session，被最终拒绝的 open 也可能创建 session、
改变 participant count 或唤醒对端。

## Decision

resident `Inode` 以 private、non-`Clone` 的 `InodeKind` 保存 immutable kind；纯 `Copy` 的
`InodeType` 继续作为外部 kind projection。FIFO variant 唯一持有 typed weak `FifoAnchor`，它只负责
把同一 resident inode 的并发 open 会合到 live Pipe session，不拥有 buffer、participant、generation、
waiter 或 readiness。session 仅由 endpoint、pending admission 和 in-flight capability 强持。

VFS open orchestration 先完成 pathname、type、DAC、`NOATIME` 与 status admission，再把 normalized
`FifoOpenContext { access, nonblock }` 交给 Pipe owner。`O_PATH` 不进入 session；普通 inode 继续使用
backend open。`O_RDONLY` / `O_WRONLY` blocking open 以 pending participation 等待 partner，
`O_NONBLOCK | O_WRONLY` 在没有 reader 时原子返回 `ENXIO`，`O_RDWR` 立即加入两侧。

Pipe session 统一拥有 anonymous 与 named FIFO 的 bytes、capacity、reader/writer count、partner
generation、read transaction gate、Event recheck hint 和 poll route。pending admission 的 generation
只在真实参与被接受时推进；取消精确撤销本次 count。blocking open 被 signal 中断时返回现有
`RestartSyscall::Idempotent` 分类：本轮 admission 完整回滚，若 signal framework 选择重放，则重新开始
一个 partner round；本轮不承诺跨 signal frame 保留旧 partner identity，也不扩张 shared restart framework。

read/write/read-write endpoint 各只贡献一次对应 participation。dup、fork 与 `CLONE_FILES` 继续共享
同一 opened description。最后一个 session capability 消失后，weak anchor 不阻止 Pipe 释放；后续 open
建立 empty、default-capacity 的 fresh session。hard link 与 rename 共享 resident inode/session；unlink 后
旧 endpoint 继续工作，同名新 FIFO 使用新的 inode/anchor/session。

## Change

- `openat` 改为 resolve/admit/activate/publish：FIFO activation 发生在 permission/status rejection 之后、
  FAN_OPEN 与 fd publication 之前；anonymous pipe 和 named FIFO 使用同一 Pipe `FileOps` 与 direct-user
  read transaction。
- Pipe endpoint 扩展为 read/write/read-write capability；session-wide read mutex 防止多个独立 reader
  对同一 staged prefix 重复 copyout/commit。participant generation 记录已经与 waiter 重叠过的真实 partner，
  但 failed nonblocking writer 不发布 count、generation 或 wake。
- nonblocking reader 在尚未出现 writer 时保存 endpoint-local generation snapshot，只抑制该 Linux FIFO
  初始窗口的 READABLE/HUP projection；Pipe writer generation 仍是唯一 truth。writer 出现过后，buffered
  bytes、EOF、HUP 与 writer-side ERR 都由 current Pipe facts 投影。
- duplex poll subscription 在 Pipe lock 下发布前完成两侧全部 fallible replacement preparation；任何
  allocation failure 都不会留下 half-subscribed route。predicate update、route snapshot 与 registry swap
  在 Pipe owner lock 下完成，Event/poll notify 和旧 registry drop 均在锁外执行。
- 增加 `fcntl-test named-fifo`，覆盖 ext4/ramfs 的 open matrix、capacity/FIONREAD、poll/HUP/ERR、
  distinct-reader exact consumption、dup/fork/final close、rename/link/unlink/recreate、signal interruption 与
  SIGPIPE；增加 glibc/musl focused LTP group：`open06`、`read03`、`write04`、`select01`、`fcntl07`、
  `dup05`、`unlink05`。`anemone-rs` 只增加该真实 consumer 使用的窄 `renameat2` wrapper。

本轮不修改 Unix pathname Socket namespace，不引入 pathname-keyed/global Pipe registry、generic inode
runtime attachment、backend-private Pipe state或持久化 session，不改变 anonymous pipe ABI、packet mode、
splice family、per-user pipe accounting或 legacy `readdir`。

## Contract Impact / Cutover

| Contract ID | 变化 | Cutover 前 effective baseline | 新 effective 规则 | 生效证据 |
| --- | --- | --- | --- | --- |
| `VFS-MAKE-NODE-001` | Refine | filesystem-backed FIFO node 可创建，但普通 open 返回 `EOPNOTSUPP` | make-node 仍只拥有 node publication；通过 VFS final admission 的 FIFO open 交给 Pipe owner | 本记录的 source audit、review、build 与 RV64 runtime |
| `PIPE-FIFO-OPEN-001` | Introduce | None | VFS admission 先于 Pipe participation；blocking/nonblocking/read-write open 与 interruption rollback 由一个 Pipe admission protocol 闭合 | [Named FIFO 当前契约](../../contracts/pipe/named-fifo.md#pipe-fifo-open-001--vfs-admission-先于-pipe-participation)及本记录证据 |
| `PIPE-FIFO-LIFECYCLE-001` | Introduce | None | resident inode weak anchor 会合 live session；Pipe 唯一拥有 data plane、participants、readiness 与 fresh-session teardown | [Named FIFO 当前契约](../../contracts/pipe/named-fifo.md#pipe-fifo-lifecycle-001--resident-inode-只弱会合-live-session)及本记录证据 |

代码与 current contract 在同一个 checkpoint 原子生效；失败时应保持旧 FIFO-open rejection，不能发布只有
anchor、只有 backend dispatch 或只有部分 access mode 的中间 surface。

## Validation

- `just fmt kernel`、`just fmt fcntl-test`、`just fmt user-test` 与 `git diff --check` 通过。
- `fcntl-test`、`user-test` 的 RV64/LA64 app build 通过；RV64/LA64 release kernel build 串行通过。
  RV64 sandbox 内 lwext4 的 `Bad system call` / `SIGSYS` 由完全相同命令在 sandbox 外成功复核为环境限制。
- canonical RV64 wrapper 使用 freshly materialized rootfs 运行：398/398 KUnit 通过；anonymous socket/pipe
  regression 通过；ext4 与 ramfs 的 named-FIFO focused suite 全部通过，包括 initial-no-writer poll suppression、
  HUP/ERR、两个独立 reader 各消费一个 distinct byte、signal interruption rollback、alias/unlink/recreate 与
  fresh-session capacity。guest 完成 filesystem、network、device shutdown并进入 PowerOff machine action。
- glibc 与 musl 各执行 7 个 focused LTP case，均为 `attempted=7 passed=7 failed=0 infra_failed=0 skipped=0`；
  合计 `attempted=14 passed=14 failed=0 infra_failed=0 skipped=0`。`select01` 每个 runtime 内 16 项 TPASS、
  3 项 architecture-unsupported TCONF；TCONF 不被伪装为已实现 syscall variant。
- 一位独立 agent 审查 owner、并发、allocation failure、read copy transaction、poll publication、open
  rollback、generation 与 Linux-visible readiness。首轮发现的四个 Apollyon 问题均已修复；同一 reviewer
  复审后没有剩余 Apollyon 或 Keter finding。最终 Architecture Friction Scan 未发现需要升级或阻断 cutover
  的第二 truth、owner 穿透、临时桥或 validation 降级。

## Remaining Risk / Links

- LA64 guest runtime、physical hardware、SMP>1、full LTP 与 final harness 均 Not Run；两架构 build和 RV64
  runtime 不替代这些边界。
- ext4 显式 sync/unmount/remount/reload 后首次 data-plane open、DAC rejection-before-participation、active-open
  mount-busy 与 crash consistency 未运行。当前 source owner 与 ext4/ramfs resident runtime验证不外推为这些证据。
- restart 只使用现有 idempotent syscall replay：取消的 admission 不跨 replay 保存 partner generation。若未来要求
  identity-preserving restart，必须由 shared signal/restart owner 另行设计，不能把第二份 truth 存入 FIFO waiter。
- Current contracts：[Pipe 当前契约](../../contracts/pipe/index.md)、
  [VFS make node](../../contracts/vfs/make-node.md)、
  [I/O Multiplexing poll wait](../../contracts/iomux/poll-wait.md)、
  [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)。
- Register：[legacy `readdir`](../../register/open-issues.md#ane-20260527-ltp-mknod-legacy-readdir)。
- Runtime log：`build/named-fifo-rv64.log`（build-local，未入库）
- RFC / transaction / external source：None
- Issue / PR / commit：this change's Git commit
