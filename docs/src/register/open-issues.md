# 开放问题

## ANE-20260527-LTP-CHDIR01-DEVICE-POOL

**Type:** Issue
**Status:** Open
**Area:** user-test / LTP / device model

**Symptom / Trigger:** 在 rv64 白名单跑到 `chdir01` 时，`tst_device` 可能拿不到可用设备，随后测试以 `TBROK: Failed to acquire device` 结束。

**Impact:** 会把一次本应聚焦内核语义的白名单验证变成环境失败，遮蔽后续回归判断。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 白名单运行时稳定提供足够的可用设备，或者 `chdir01` 不再依赖当前这套设备池约束。
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Low
**Workaround:** 重新整理设备占用后再跑，或在专门的验证环境中执行该用例。

## ANE-20260527-MMAP-MPROTECT-HEAP-FASTPATH-PERSISTENCE

**Type:** Issue
**Status:** Open
**Area:** mm / uspace / mprotect

**Symptom / Trigger:** 当前 heap 上的 `mprotect` 快路径只会直接改写现有 PTE；如果后续再触发缺页，回填路径仍会按 VMA 的原始 `prot` 重新建页，保护属性不会稳定保留。

**Impact:** 这会让 heap 区间的保护变更在“已有页”与“未来 fault 页”之间出现分裂，语义和 Linux 的连续区间保护预期不一致。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 为 heap 保护变更补齐可持久化的范围级保护记录，或者在修改保护时把 heap 拆成可独立表达保护的 VMA，并重新验证 `mprotect` / fault 交互。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 避免把需要长期保留的权限切换依赖在当前 heap 快路径上。

## ANE-20260527-MADVISE-DONTNEED-LOCKED-SHARED

**Type:** Issue
**Status:** Open
**Area:** mm / madvise / mlock

**Symptom / Trigger:** `madvise(MADV_DONTNEED)` 目前只做 discard hint，不会区分页面是否已被 `mlock`，也不会按 shared / locked 约束返回 `EINVAL`；在 LTP `madvise02` 里，这会把本该拒绝的 locked/shared 场景放过去。

**Impact:** 白名单里 `madvise02` 的锁页/共享页拒绝语义仍然不对，和 Linux 预期有偏差。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 补齐锁页状态账本和共享页语义后，让 `MADV_DONTNEED` 按真实页面状态返回 `EINVAL` 或执行对应的回收逻辑，并重新跑 `madvise02`。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

**Severity:** Medium
**Workaround:** 暂时不要把 `madvise02` 这类锁页语义当成已收敛的能力。

## ANE-20260527-OPENAT-NOFOLLOW-OPATH-SYMLINK

**Type:** Issue
**Status:** Resolved
**Area:** fs / openat / readlinkat

**Symptom / Trigger:** `openat` 曾经没有真正支持 `O_NOFOLLOW`，`O_PATH | O_NOFOLLOW` 打开的 symlink 不能稳定保留“指向符号链接本体”的语义，导致 `readlinkat("", ...)` 这类空路径用例失败。

**Impact:** 已通过 fd/openat cleanup 收敛：final `O_NOFOLLOW` symlink 拒绝、`O_PATH | O_NOFOLLOW` symlink fd 保存和 `readlinkat(fd, "", ...)` 路径已经落地；剩余 `O_PATH` 后续能力按当前限制跟踪。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Exit Condition:** 已完成。focused rv64 LTP 中 `readlinkat01` 的 glibc / musl 空路径分支通过。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

**Severity:** Medium
**Workaround:** 无需针对该问题绕过；完整 `O_PATH` 能力仍见 `ANE-20260528-OPATH-STAGE1-CAPABILITIES`。

## ANE-20260527-LTP-MKNOD-LEGACY-READDIR

**Type:** Issue
**Status:** Open
**Area:** fs / syscall ABI / user-test

**Symptom / Trigger:** 老白名单里的 `read03` 需要 `mknod()` 生成 FIFO，而 `readdir21` 还直接依赖 legacy `__NR_readdir` 入口；当前链路里前者返回 `ENOSYS`，后者在该架构上也没有对应 syscall。

**Impact:** 这两项会继续把旧白名单的通过率卡在 syscall 入口层，和具体文件系统逻辑无关。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 补上 `mknod` / FIFO 创建的 syscall 路径，并决定是否为该架构提供 legacy `readdir` 兼容入口后，再重新跑 `read03` 和 `readdir21`。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 先把这两个用例从当前白名单里隔离出来，或者等 syscall 入口补齐后再回归。

## ANE-20260528-EXEC-ETXTBSY-WRITER-ACCOUNTING

**Type:** Issue
**Status:** Open
**Area:** fs / execve / open-file accounting

**Symptom / Trigger:** LTP `execve04` 让子进程以 `O_WRONLY` 打开 `execve_child`，父进程随后 `execve("execve_child", ...)`；Linux 期望返回 `ETXTBSY`，当前内核仍允许执行，导致 `execve_child` 运行并输出 `execve_child shouldn't be executed`。

**Impact:** 缺少 executable-vs-writer 排斥语义，会让正在被写打开的文件仍可作为新程序映像执行，和 Linux 的 text file busy 语义不一致。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Exit Condition:** 为 VFS/open-file-description 或 inode 增加系统性的写打开/可执行打开账本，补齐 `execve` 与 writable open/truncate/write 之间的排斥规则，并重新验证 `execve04`。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 暂时不要把 `execve04` 视为 exec 主路径回归；等 VFS busy 账本系统性实现后再纳入通过项。

## ANE-20260529-MUSL-MEMORY-MADVISE01-SCHED-ASSERT

**Type:** Issue
**Status:** Open
**Area:** sched / task / user-test / LTP

**Symptom / Trigger:** 使用 `./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img build/ltp-debug.log` 复跑 memory profile 时，glibc memory 组已经完整结束；切到 musl memory 后，在 `madvise01` 执行到 `MADV_DOFORK` 附近触发 `anemone-kernel/src/sched/processor.rs:131` 的 `assertion failed: task.status() == TaskStatus::Runnable`。

**Impact:** musl memory 组无法完整跑完，导致本轮只能确认 glibc memory 组的 mmap / mremap errno 修复结果；后续 musl memory 的剩余失败矩阵会被这个调度断言遮蔽。

**Owner:** doruche
**Last Verified:** 2026-05-29
**Exit Condition:** 定位该断言对应的 task 状态转移竞态或错误唤醒路径，保证 musl memory 组至少能跑完整组并正常关机，再重新评估 musl 侧 mmap / madvise 失败项。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** High
**Workaround:** 当前先用 glibc memory 组验证 mmap / mremap errno 修复；musl memory 组需要等 scheduler 断言修复后再作为完整回归依据。

## ANE-20260531-SCHED-EVENT-WAKE-RUNNABLE-RACE

**Type:** Issue
**Status:** Open
**Area:** sched / event / task / user-test

**Symptom / Trigger:** 在 `Event::listen*()` 的等待循环与 `publish()` 唤醒交错时，`prepare_listener()` 会过早把当前 task 标成 `Waiting`，而 waker 侧 `try_to_wake_up()` 先把它切回 `Runnable` 再独立调用 `task_enqueue()`；如果 waiter 在这段窗口里完成一轮 `schedule()` 并进入下一轮等待，旧 wake 的尾巴就可能撞上新的 `Waiting` 状态，触发 `anemone-kernel/src/sched/processor.rs:255` 的 `assert!(task.status() == TaskStatus::Runnable)`。

**Impact:** 会让部分 user-test / LTP profile 随机 panic，遮蔽后续语义回归判断；这是 scheduler / event 的状态交错问题，不是平台硬件语义差异本身。

**Owner:** doruche
**Last Verified:** 2026-05-31
**Exit Condition:** 将 event 等待轮次、唤醒归属和 task 入队时序收口，确保 waiter 下一轮不会接到前一轮 wake 的尾巴，并在已知触发 profile 上不再出现该断言。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [RFC-20260601-sched-wait-refactor](../rfcs/sched-wait-refactor/index.md), [Sched Wait Refactor 事务日志](../devlog/transactions/2026-06-01-sched-wait-refactor.md)

**Severity:** High
**Workaround:** 继续用同一类 profile 复跑确认是否命中该竞态，但不要把它当成已收敛的功能缺口。

## ANE-20260602-SHMAT1-SIGILL-MASKS-SEGV-HANG-REVALIDATION

**Type:** Issue
**Status:** Open
**Area:** mm / signal / SysV shm / user-test / LTP

**Symptom / Trigger:** `shmat1` 曾在只读 attach 后写入触发缺页异常，内核投递 `SIGSEGV` 后进入线程组退出路径并卡住，`build/shmat01-stuck.log` 后段表现为大量重复的 event publish。修正同步 fault 的 `SIGSEGV` 应投递给 faulting task 而不是线程组后，最新 rv64 多次复跑未再复现该卡死；但最新 `build/user-test-rv64.log` 里的 glibc / musl `shmat1` 都更早以 illegal instruction 退出，状态码为 132。

**Impact:** 当前 rv64 日志只能说明原来的 `SIGSEGV -> exit_group` 卡死没有再次出现，不能证明原始 `NotMapped -> SIGSEGV -> task exit` 路径已经被完整覆盖；`SIGILL` 132 会遮蔽 `shmat1` 对只读映射 fault 语义和退出收敛性的回归判断。

**Owner:** doruche
**Last Verified:** 2026-06-02
**Exit Condition:** 先定位并消除 rv64 `shmat1` 的 illegal instruction 132，或构造一个定向用例稳定覆盖只读 `shmat` 写 fault；确认同步 `SIGSEGV` 只投递到 faulting task，线程组退出能收敛，且不再出现重复 event publish 卡死。

**Related:** [Sched Wait Refactor 事务日志](../devlog/transactions/2026-06-01-sched-wait-refactor.md), [RFC-20260601-sched-wait-refactor](../rfcs/sched-wait-refactor/index.md), [当前限制：SysV shm LTP infra](./current-limitations.md#ane-20260529-sysv-shm-ltp-infra-stage1)

**Severity:** High
**Workaround:** 当前只把最新 rv64 结果视为“未复现卡死但被 SIGILL 遮蔽”；不要用旧 la64 日志判断本轮修复结果，也不要把该项标成已验证通过。

## ANE-20260606-RT-SIGTIMEDWAIT-ASYNC-WAITED-SIGNAL-EINTR

**Type:** Issue
**Status:** Open
**Area:** signal / wait-core / syscall ABI

**Symptom / Trigger:** `rt_sigtimedwait` 在 wait-core 返回 `Signal` 或 `Force` outcome 时，会先把结果分类为 interrupted，恢复旧 signal mask，然后返回 `EINTR`；该分支没有先尝试 dequeue waited set 中的 pending signal。如果 waited signal 在 syscall precheck 之后到达并完成当前 wait round，调用可能错误返回 `EINTR`，而不是消费该 signal 并返回 signal number / siginfo。

**Impact:** 这会破坏 `rt_sigtimedwait` 的同步等待语义，并影响 `sigtimedwait` / `sigwaitinfo` 以及依赖它同步收割信号的 libc、BusyBox 或 LTP 路径。该问题不是 `sigsuspend` delayed mask restore 的范围扩张理由：`rt_sigtimedwait` 仍应在 syscall body 内同步 dequeue waited signal 并恢复 mask，不应改造成 trap-return signal delivery / `rt_sigreturn()` 协议。

**Owner:** doruche
**Last Verified:** 2026-06-06
**Exit Condition:** `rt_sigtimedwait` 在 wait completion 后先按 waited set 尝试 dequeue matching signal，只有确认没有 waited signal 且存在其他未屏蔽 signal / force 条件时才返回 `EINTR` 或进入对应 fail-closed 路径；重新验证 waited signal 在 precheck 后到达的定向用例，以及 LTP `rt_sigtimedwait01` / `sigtimedwait01`。
**Related:** [Sched Wait Refactor 事务日志](../devlog/transactions/2026-06-01-sched-wait-refactor.md), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 不要把当前 `rt_sigtimedwait` 的 async wake `EINTR` 结果当成已收敛 ABI；需要验证同步信号等待语义时，应使用覆盖 precheck-after-arrival 窗口的定向用例重新确认。

## ANE-20260607-SIGNAL-LTP-REMAINING-SEMANTICS

**Type:** Issue
**Status:** Open
**Area:** signal / syscall ABI / scheduler / user-test / LTP

**Symptom / Trigger:** `build/user-test-rv64.log` 的 signal profile 中，`tgkill03` 和 `rt_sigqueueinfo01` 已有明确窄修；剩余仍有若干非设施型缺口需要单独收敛：`tgkill02` 在 `RLIMIT_SIGPENDING=0` 且 realtime signal 被阻塞时，Linux/LTP 期望 `tgkill()` 返回 `EAGAIN`，当前仍成功；`rt_sigaction01` / `rt_sigaction02` 在 signal 64 边界分别表现为期望成功却得到 `EINVAL`、坏用户指针期望 `EFAULT` 却先被 signal 编号校验拦成 `EINVAL`；`kill02` 在 child setup 阶段 timeout 并 `TBROK`，当前判断更像 LTP busy-poll/setup 与 scheduler/preemption 可观察性问题，不能作为 kill syscall errno 语义失败直接处理。

**Impact:** 这些残余会继续拉低 signal profile 得分，并且会把三类问题混在一起：realtime pending queue/resource accounting、signal number ABI 上界与参数校验顺序、以及 LTP setup 运行时可调度性。若不分开处理，后续容易为单个 TFAIL 写出过宽的 signal 子系统改动。

**Owner:** doruche
**Last Verified:** 2026-06-07
**Exit Condition:** 分别补齐并验证：realtime signal queue 与 `RLIMIT_SIGPENDING` 的 `EAGAIN` 语义；rt signal 编号上界 / `NSIG` 与 bad pointer 校验顺序策略；`kill02` setup timeout 的调度或 runner 根因。随后复跑 signal profile，确认 `tgkill02`、`rt_sigaction01`、`rt_sigaction02` 和 `kill02` 被重新归类或通过。
**Related:** [Signal LTP tgkill/sigqueueinfo 小迭代记录](../devlog/changes/2026-06-07-signal-ltp-tgkill-sigqueueinfo.md), [当前限制：Signal LTP infra](./current-limitations.md#ane-20260607-signal-ltp-infra-stage1), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

**Severity:** Medium
**Workaround:** 当前先不要把 `tgkill02` / `rt_sigaction01` / `rt_sigaction02` / `kill02` 当成同一个 signal delivery bug；优先按上述子问题分别构造或复跑定向用例。
