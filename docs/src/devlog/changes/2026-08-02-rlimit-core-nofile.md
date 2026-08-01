# ANE-CHG-20260802-rlimit-core-nofile

**Type:** Small Feature / Linux ABI correction
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** task / resource policy / file descriptor table / syscall ABI / user test

## Problem

Anemone 原先没有真实的 process resource-limit policy。`getrlimit()` 与
`prlimit64()` 各自返回固定值；`prlimit64(new_limit != NULL)` 打日志后仍成功，却不保存
新值，也不改变任何资源行为。`RLIMIT_NOFILE` 虽然报告
`MAX_FD_PER_PROCESS`，所有 fd allocation 仍只受整张 `FileTable` bitmap 容量约束。

这会同时造成 ABI 不诚实和 owner 混乱：syscall 层不能伪装 update 成功，rlimit policy
也不能复制一份 fd usage；真实 slot occupancy、reservation、publication 与 cleanup 已由
`FileTable` 拥有。

Linux 6.6.32 将 `prlimit64(pid, ...)` 的 pid 解析为 live task，在 shared process policy
上完成 read/update；跨 task 访问使用 caller real UID/GID 与 target
real/effective/saved IDs 或 `CAP_SYS_RESOURCE` 判断。fd allocator 则把当前
`RLIMIT_NOFILE` 作为 fd-number range 的 exclusive end。参考
`xref:linux-6.6.32:kernel/sys.c#do_prlimit`、
`xref:linux-6.6.32:kernel/sys.c#check_prlimit_permission`、
`xref:linux-6.6.32:kernel/sys.c#SYSCALL_DEFINE4(prlimit64)` 与
`xref:linux-6.6.32:fs/file.c#alloc_fd`。

## Decision

user `ThreadGroup` 是 process rlimit policy 的唯一 owner；`FileTable` 继续是 fd usage
的唯一 owner。二者只通过一次 allocation 使用的 crate-private `FdAllocCeiling`
snapshot 交接：

```text
getrlimit / prlimit64
        |
        v
user ThreadGroup: (soft, hard) policy
        |
        | FdAllocCeiling snapshot
        v
FileTable: bitmap / reservation / publication / cleanup
```

`RLIMIT_NOFILE.soft == N` 允许分配的 fd 范围是 `[0, N)`。降低 soft limit 不关闭、
不扫描既有 fd；已经取得的 reservation 可以按原协议 commit 或 rollback，commit 不二次
读取 policy。fork/vfork 创建新 `ThreadGroup` 时复制完整 pair，`CLONE_THREAD` 共享，
exec 保留；`CLONE_FILES` 只共享 fd table，不共享不同 `ThreadGroup` 的 policy。

初始 soft/hard 都从 `MAX_FD_PER_PROCESS` 派生。该 Kconfig 项只表示 build-time
`FileTable` capacity 与 system ceiling，运行期 syscall 不能改变它。当前只有
`RLIMIT_NOFILE` 可写并有 enforcement；已有诚实的固定 compatibility readback 保留，
其它 update 明确返回 `ENOSYS`。

`prlimit64` 接受任一 live user TID，并在 topology owner 的一次 read transaction 内取得
匹配的 `Task` 与 `ThreadGroup` capability；credential permission 使用被寻址 task 的 live
credential。RV64 继续暴露 legacy `getrlimit`，LA64 按 asm-generic 架构选择不暴露该
syscall；`anemone-rs` 对 `getrlimit` 使用 RV64 compile-time cfg，`prlimit64` 是两架构
共同接口。

## Change

- `ThreadGroup` 增加 user-only `UserResourceLimits`，构造形状由
  `ThreadGroupType` 断言；kthread 不携带伪造的 user policy。
- `getrlimit` 与 `prlimit64` 共用 resource readback owner；`prlimit64` 实现 target
  resolution、permission、copyin、pre-update snapshot、atomic pair update 与 commit 后
  copyout，copyout failure 不回滚 policy。
- `RLIMIT_NOFILE` 校验 `soft <= hard <= MAX_FD_PER_PROCESS`；提高 hard limit需要
  effective `CAP_SYS_RESOURCE`。
- normal open、description open、reservation、`dup`、`F_DUPFD*` 与 `dup3` 全部消费
  `FdAllocCeiling`；`dup3` 在 replacement cleanup 前拒绝超出 caller ceiling 的 target。
- `F_DUPFD*` 的 command-specific minimum 采用 Linux `(int)arg` 形状，negative 或
  `minimum >= soft` 返回 `EINVAL`，不复用 ordinary fd 的 `EBADF` parser。
- Kconfig 默认配置和 generator 注释改为 storage/system ceiling 语义，没有新增同值的
  runtime-default 配置别名。
- `anemone-rs` 增加 common `prlimit64` wrapper；legacy `getrlimit` wrapper 只在 RV64
  编译。
- 新增 `rlimit-test` app 与双架构 focused rootfs，覆盖 policy、permission、allocator、
  inheritance、shared-table independence 与架构接口差异。

## Validation

- owner-local KUnit 在 RV64 运行 327 项、LA64 运行 332 项，均打印
  `All tests passed!`。新增 coverage 包括 pair/system-ceiling 校验、pre-update/fork
  snapshot、所有 fd allocation primitive、reservation commit、replacement ordering 与
  `F_DUPFD*` minimum errno。
- RV64 `rlimit-test` 十项全部通过并打印 `RLIMITTEST:END:all cases passed`：readback/
  update/error、unprivileged hard raise与跨进程 permission、真实 `EMFILE`、lowering
  survival、fork isolation、cross-process update、nonleader-TID target、fcntl minimum
  errno、`CLONE_FILES` policy independence、exec preservation。完整日志为 agent-private
  `/tmp/rlimit-rv64-final.log`。
- LA64 运行同一十项 matrix并全部通过，且应用在编译期不包含 legacy `getrlimit`；common
  `prlimit64` read/update/enforcement 路径通过。完整日志为 agent-private
  `/tmp/rlimit-la64-final.log`。guest 完成 filesystem/network/device shutdown 后因当前
  LA64 无可用 poweroff handler 进入 halt；terminal PASS 已出现后由 host 退出 QEMU。
- `just app build rlimit-test --arch riscv64` 与
  `just app build rlimit-test --arch loongarch64` 通过。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=2G` 与 LA64
  对应命令通过。RV64 首次 sandbox build 在 lwext4 C 编译命中 host seccomp `SIGSYS`；
  完全相同命令在 sandbox 外通过，按环境限制归类。
- source audit确认所有 production fd allocator 都经 Task/files facade消费 typed ceiling，
  policy/table 不互存引用或 usage/policy cache；`MAX_FD_PER_PROCESS` 的 tracked 描述不再
  声称可由 syscall override。
- 一位独立 agent review 发现并推动修复 nonleader TID resolution、`F_DUPFD*` negative
  errno 与 task/TG 两次 topology lookup race；同一 reviewer最终复核无 blocking 或
  Euclid finding。
- focused LTP rlimit/getrlimit profile、完整 user-test profile、SMP runtime与 hardware
  均 Not Run；这里不从 focused QEMU 外推这些证据。

## Remaining Risk / Links

- `RLIMIT_NOFILE` 是本轮唯一可写、可 enforcement 的 resource。CPU、FSIZE、STACK、CORE、
  NPROC 的固定 readback 仍只是明确的 compatibility 能力；DATA、MEMLOCK、SIGPENDING、
  NICE、RTPRIO 等没有被顺带实现。
- user namespace、LSM hook、`/proc/<pid>/limits`、shell builtin与独立 initial soft/hard
  Kconfig 均不在本轮 target。出现真实需求时应重新判断 small iteration 或 RFC 边界。
- 相关 LTP infra限制继续保持 Active；本轮只更新其中已经变化的 rlimit事实，不把未复跑的
  signal、memory或 SysV shm profile写成通过。
- Register / limitations:
  [SysV shm LTP infra](../../register/current-limitations.md#ane-20260529-sysv-shm-ltp-infra-stage1)、
  [memory LTP procfs/devzero/rlimit](../../register/current-limitations.md#ane-20260529-memory-ltp-procfs-devzero-rlimit-stage1)、
  [signal LTP infra](../../register/current-limitations.md#ane-20260607-signal-ltp-infra-stage1)
- Current contract: None；本轮 owner、handoff、failure、cleanup 与 ABI 在一个原子
  small-change checkpoint 内闭合，没有提取新的仓库级 shared contract。
- RFC / transaction: None
- Issue / PR / commit: this change's Git commit
