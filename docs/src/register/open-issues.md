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

## ANE-20260608-RISCV-FPU-TRAP-RETURN-UNSAFE-BOUNDARY

**Type:** Issue
**Status:** Fix landed; revalidation pending
**Area:** riscv64 / trap return / FPU / unsafe boundary

**Symptom / Trigger:** rv64 LTP `poll02`、`pselect01` 以及 `iozone -t 4` 等路径在 release 运行中会在用户态浮点指令附近收到 `SIGILL`，即使日志已经显示 lazy-FPU 路径曾为该 task 打印 `enabled fpu`。问题对插桩高度敏感：在 `utrap` 路径打日志会让原始 `SIGILL` 消失；普通 `let _ = trapframe.sstatus()` 不改变行为；`core::hint::black_box(trapframe.sstatus())` 又能让失败消失。2026-06-18 根因收敛为 riscv64 user-trap assembly 在 trapframe 入栈后直接调用 Rust，但 `RiscV64TrapFrame` 大小曾使 `$sp` 偏离 RISC-V C ABI 要求的 16 字节对齐；插桩只是改变了 UB 表面。修复已将 riscv64 trapframe 对齐、尺寸断言和 trap CSR 偏移护栏落地，并移除 `black_box` 止血；loongarch64 同步增加预防性 trapframe / FPU context 布局护栏。

**Impact:** 该现象说明 riscv64 用户返回、FPU lazy enable、trapframe 内存提交和 `sstatus.FS` CSR 恢复之间的 unsafe Rust / assembly 边界曾违反调用 ABI。`Task::fpu_used()` 只能证明 task 有 FPU 上下文，不能证明 trap entry 以满足编译器 ABI 假设的栈形态进入 Rust。若 hand-written assembly 破坏栈对齐，后端可基于合法 ABI 前提生成对该环境不安全的代码，从而把原本应继续执行的用户态浮点指令错误暴露为 `SIGILL`，并遮蔽 `poll02` / `pselect01` 以及其它 rv64 SIGILL 相关回归判断。

**Owner:** doruche
**Last Verified:** 2026-06-18
**Exit Condition:** 在移除 `black_box` 止血后，release rv64 `iozone -t 4` 已由用户确认不再触发同类 SIGILL；仍需复跑并确认 `poll02` / `pselect01` 以及 rv64 full/user-test 相关 profile 不再在已启用 FPU 后因同类浮点指令收到 `SIGILL`。确认后移除此开放问题，并把长期历史保留在开发日志中。
**Related:** [SHMAT1 SIGILL revalidation](#ane-20260602-shmat1-sigill-masks-segv-hang-revalidation)

**Severity:** High
**Workaround:** 已移除 `core::hint::black_box(trapframe.sstatus())` 止血；后续不要重新引入插桩或 opaque read 作为稳定器。若同类 SIGILL 再现，优先检查 arch trap entry 是否保持 Rust/C ABI 调用边界、trapframe 布局断言和汇编偏移护栏，而不是先假设用户二进制非法。

## ANE-20260616-EXIT-WAIT-REAP-PARENT-TOPOLOGY-RACE

**Type:** Issue
**Status:** Open
**Area:** task topology / exit / wait / scheduler

**Symptom / Trigger:** 在线程组最后一个 task 退出时，`kernel_exit()` 先把当前 task 从 topology 中摘除，并在 child thread group 的 `members` 已为空后，把 thread-group 生命周期发布为 `ThreadGroupLifeCycle::Exited`。但同一段退出尾部在发布 `Exited` 之后，仍继续通过 `tg.get_parent()` 获取 parent `ThreadGroup`，并向 parent 递送终止信号、发布 parent 的 `child_exited` event，最后再唤醒 init reaper 和 `vfork_done`。这使 `Exited` 对 wait-family 可见的时间点早于退出发布协议真正完成的时间点。

相关代码在 `anemone-kernel/src/task/api/exit/mod.rs`：

```rust
let is_last = task.detach_from_topology();

// if we are the last thread in this thread group, we should do the cleanup
// work.

// a longer critical section must be held here to avoid races. TODO: explain
// why.
if is_last {
    let mut tg_inner = tg.inner.write();

    let xcode = match tg_inner.status.life_cycle {
        ThreadGroupLifeCycle::Alive => {
            // no one called exit_group before. all threads call exit... use our exit code.
            code
        },
        ThreadGroupLifeCycle::Exiting(existing_code) => {
            // someone already called exit_group before. use their exit code.
            existing_code
        },
        ThreadGroupLifeCycle::Exited(existing_code) => {
            panic!("thread group already exited with code {:?}", existing_code);
        },
    };

    // 1. reparent orphan children.
    // following operations are a bit tricky, but it's safe.
    //
    // TODO: but i think we'd better switch to a more reasonable and less
    // error-prone design later.
    drop(tg_inner);
    tg.reparent_orphan_children();
    tg_inner = tg.inner.write();

    // 2. set status to Exited, so that wait4 can reap this thread group.
    tg_inner.status.life_cycle = ThreadGroupLifeCycle::Exited(xcode);
    kerrln!(
        "[special_report] kernel_exit tg_exited tid={} tgid={} code={:?}",
        task.tid(),
        tg.tgid(),
        xcode
    );

    drop(tg_inner);

    let cpu_usage = tg.cpu_usage_snapshot();

    if let Some(terminate_signal) = tg.terminate_signal() {
        let uid = tg
            .leader()
            .map(|leader| leader.cred().uid.real)
            .unwrap_or_else(|| task.cred().uid.real);
        tg.get_parent().recv_signal(Signal::new(
            terminate_signal,
            SiCode::Kernel,
            SigInfoFields::Chld(SigChld {
                pid: tg.tgid(),
                uid,
                // TODO: this is false. we should look at si_code first.
                status: match xcode {
                    ExitCode::Exited(xcode) => xcode as i32,
                    ExitCode::Signaled(signo) => signo.as_usize() as i32,
                },
                utime: duration_to_ticks(cpu_usage.self_user() + cpu_usage.reaped_user()),
                stime: duration_to_ticks(
                    cpu_usage.self_kernel() + cpu_usage.reaped_kernel(),
                ),
            }),
        ));
    }

    // 3. publish child_exited event.
    kerrln!(
        "[special_report] kernel_exit child_exited_publish child_tgid={}",
        tg.tgid()
    );
    tg.get_parent().child_exited.publish(1, false);
    kerrln!(
        "[special_report] kernel_exit child_exited_publish_done child_tgid={}",
        tg.tgid()
    );

    // 4. orphan children reparented to init may contain zombie thread groups. let's
    //    publish that to init as well.
    // this hardcoding is a bit ugly. when we support subreapers, we should publish
    // this to the actual reaper.
    get_init_task()
        .get_thread_group()
        .child_exited
        .publish(1, false);

    task.vfork_done.publish(1, true);
}
```

**Mechanism:** wait-family core 只用 `ThreadGroupLifeCycle::Exited` 判断 exited child 是否命中。`sys_wait4()` 和未设置 `WNOWAIT` 的 `sys_waitid()` 都会以 `WaitDisposition::Reap` 进入 `wait_for_exited_child()`；命中后会调用 `ThreadGroup::try_reap_child()`。也就是说，一旦 `kernel_exit()` 写入 `Exited` 并释放 `tg_inner` 锁，parent 的 wait 路径就可以把该 child 作为可 reap zombie 处理，而不会等待 child 退出尾部完成 parent signal、`child_exited` publish 或 reaper wake。

相关代码在 `anemone-kernel/src/task/api/wait/mod.rs`：

```rust
fn scan_one(&mut self, tg: &Arc<ThreadGroup>) -> bool {
    let matched = match self.target {
        WaitTarget::AnyChild => true,
        WaitTarget::ChildWithTgid(tgid) => tg.tgid() == tgid,
        WaitTarget::AnyChildWithPgid(pgid) => tg.pgid() == pgid,
        WaitTarget::AnyChildWithCurrentPgid => tg.pgid() == self.current_pgid,
    };

    if !matched {
        return false;
    }

    self.matched_any = true;
    matches!(tg.status().life_cycle(), ThreadGroupLifeCycle::Exited(_))
}
```

```rust
if let Some(child) = tg.find_child(|child| scanner.scan_one(child)) {
    kdebugln!(
        "wait: found a child {} that satisfies the wait condition",
        child.tgid(),
    );
    let tgid = child.tgid();

    match disposition {
        WaitDisposition::Peek => {
            return Ok(Some(wait_outcome_from_child(&child)));
        },
        WaitDisposition::Reap => {
            drop(child);

            // If multiple threads are waiting for the same child, only
            // one of them can reap it, and the others will fail to find
            // the child in topology. This is fine, since they will just
            // loop and wait again.
            if let Some(child) = tg.try_reap_child(tgid) {
                fs::proc::try_unbind_thread_group(tgid);
                let outcome = wait_outcome_from_child(&child);
                kerrln!(
                    "[special_report] wait4 reap parent_tgid={} child_tgid={} exit_code={:?}",
                    tg.tgid(),
                    outcome.tgid,
                    outcome.exit_code
                );
                tg.on_reap(&child);

                return Ok(Some(outcome));
            }

            continue;
        },
    }
}
```

**Topology Removal:** `try_reap_child()` 会在持有 `TOPOLOGY` 写锁时直接从 `topology.thread_groups` 删除 child TG。它只要求 child 仍挂在 parent 的 `children_tgids` 中、生命周期已经是 `Exited`、`parent_tgid` 等于当前 wait caller 的 TGID，并且 `ntasks() == 0`。这些条件在 `kernel_exit()` 发布 `Exited` 后、退出尾部完成前都可能已经成立：`task.detach_from_topology()` 已经清空最后一个 member，`Exited` 已经写入，parent-child 链接也尚未被 reap 移除。

相关代码在 `anemone-kernel/src/task/topology/parent_child.rs`：

```rust
pub fn try_reap_child(&self, child_tgid: Tid) -> Option<Arc<ThreadGroup>> {
    let mut topology = TOPOLOGY.inner.write();

    // make sure this is indeed a child thread group of us.
    if !self.inner.read().children_tgids.contains(&child_tgid) {
        return None;
    }

    let child_tg = topology.thread_groups.remove(&child_tgid)?;

    assert!(
        matches!(
            child_tg.status().life_cycle(),
            ThreadGroupLifeCycle::Exited(_)
        ),
        "task topology: child thread group {} is not exited yet when reaping",
        child_tgid
    );
    assert!(
        child_tg.parent_tgid() == Some(self.tgid()),
        "task topology: child thread group {} has unexpected parent {:?} when reaping",
        child_tgid,
        child_tg.parent_tgid()
    );
    assert!(
        child_tg.ntasks() == 0,
        "task topology: child thread group {} is not empty when reaping",
        child_tgid
    );

    assert!(
        self.inner.write().children_tgids.remove(&child_tgid),
        "task topology: child thread group {} disappeared from parent {} when reaping",
        child_tgid,
        self.tgid()
    );

    Some(child_tg)
}
```

**Failure Mode:** 被 reap 的 child `ThreadGroup` 可能仍被正在执行 `kernel_exit()` 的栈帧持有，因此 child TG 对象尚未 drop，退出尾部仍会继续运行。此时 child TG 的 `inner.parent_tgid` 仍保存原 parent TGID；但 parent 可能已经在 reap child 后继续退出，并被自己的 parent 通过相同 wait/reap 路径从 `TOPOLOGY.thread_groups` 删除。于是 child 退出尾部再次调用 `get_parent()` 时，会先成功读到 `Some(parent_tgid)`，然后在全局 topology 中查不到该 parent TG 并 panic。

相关代码在 `anemone-kernel/src/task/topology/parent_child.rs`：

```rust
pub fn get_parent(&self) -> Arc<ThreadGroup> {
    let parent_tgid = self
        .inner
        .read()
        .parent_tgid
        .expect("task topology: parent thread group not found");

    let topology = TOPOLOGY.inner.read();

    let parent = topology
        .thread_groups
        .get(&parent_tgid)
        .expect("task topology: parent thread group not found")
        .clone();

    parent
}
```

**Impact:** 这是 `ThreadGroupLifeCycle::Exited` 的语义混合问题：它同时表示“exit code 已经确定”和“waiter 可以从 topology 删除该 thread group”，但 `kernel_exit()` 在发布这个状态后仍执行依赖 parent topology 的外部可见通知。该状态边界允许 wait/reap 观察并删除一个尚未完成退出发布协议的 child，破坏 parent-child topology ownership，并影响所有 fork/exit/wait 密集路径；一旦触发 panic，会遮蔽后续用户态 profile 和 syscall 语义判断。

**Owner:** EDGW_
**Last Verified:** 2026-06-16
**Exit Condition:** 重定义 thread-group 退出状态机，使 wait/reap 只能删除已经完成父通知、`child_exited` 发布、init reaper wake 和必要退出尾部的 zombie；或者在任何可 reap 状态暴露前稳定持有后续退出尾部所需的 parent/reaper 能力对象，并保证退出尾部不再通过 stale `parent_tgid` 回查全局 topology。修复应明确 `Exiting`、exit code 可见、parent 通知完成、zombie 可 reap 四个阶段的 owner、锁顺序和引用规则，并用定向测试覆盖 last-thread exit、parent 同时 wait/reap、parent 快速退出并被祖先进程 reap、以及 child 退出尾部继续执行的交错。

**Related:** [开发日志：2026-06-08 至 2026-06-21](../devlog/2026-06-08_to_2026-06-21.md), [当前限制：进程组与会话 stage-1](./current-limitations.md#ane-20260527-process-group-session-stage1)

**Severity:** High
**Workaround:** 在修复前，不要把这类 panic 归因到触发时正在运行的用户态 case 或单个 syscall；应按 task topology / exit-wait 生命周期竞态处理，并避免使用 panic 之后的测试结果判断后续功能语义。

## ANE-20260616-SIGNAL-SIGFRAME-EXIT-USPACE-LOCK-RECURSION

**Type:** Issue
**Status:** Open
**Area:** signal / exit / mm uspace / LTP

**Symptom / Trigger:** `etc/log-la.log` 跑到 `RUN LTP CASE crash01` 后，child task #232 连续制造用户态 `InvalidInstruction`、execute fault 和 read fault。日志最后显示 `kernel_exit enter tid=task #232 tgid=task #232 code=Signaled(SigNo(11))`，随后内核 panic 于 `anemone-kernel/src/task/api/exit/mod.rs:41:37`，消息为 `Mutex cannot be locked recursively`。这里 `crash01` 的非法指令和缺页只是触发用户信号路径；真正让内核停止的是 signal sigframe 写失败分支在仍持有 `UserSpace` mutex 时进入 `kernel_exit_group()`，而 `kernel_exit()` 又为 `clear_child_tid` 重锁同一个 `UserSpace`。

**Confirmed Evidence:** `etc/log-la.log` 末尾的 backtrace 地址符号化结果为：

```text
0xffffffff80324a24  Mutex<UserSpace>::lock
0xffffffff802921a8  task::api::exit::kernel_exit
0xffffffff80292b98  task::api::exit::kernel_exit_group
0xffffffff80342fe0  task::sig::handle_signals
0xffffffff8038e218  rust_utrap_entry
```

`0xffffffff802921a8` 对应 `kernel_exit()` 中 `clear_child_tid` 的 `usp.lock()` 之后；`0xffffffff80342fe0` 对应 `perform_signal_action()` 中 sigframe 写失败后调用 `kernel_exit_group(SIGSEGV)` 的下一条指令。也就是说，panic 时不是普通默认信号动作直接退出，而是走过了用户自定义 handler 的 sigframe 构造失败路径。

**Full Failure Path:**

1. LoongArch 用户异常入口识别异常。如果是未覆盖的异常，例如 `crash01` 里的非法指令，trap 代码先把 `SIGILL` pending 到当前 task；如果是用户页异常，则进入 `handle_user_page_fault()`，失败时也会转成用户信号。无论是哪一种，trap return 前都会调用 `handle_signals()`：

```rust
match reason {
    LA64Exception::Syscall => {
        restart_syscall = handle_syscall(trapframe);
    },
    LA64Exception::PageModified
    | LA64Exception::PageNotReadable
    | LA64Exception::PageNotExecutable
    | LA64Exception::PagePrivilegeIllegal
    | LA64Exception::PageInvalidFetch
    | LA64Exception::PageInvalidLoad
    | LA64Exception::PageInvalidStore => {
        handle_user_page_fault(PageFaultInfo::new(
            VirtAddr::new(trapframe.era),
            VirtAddr::new(trapframe.badv as u64),
            match reason {
                LA64Exception::PageInvalidFetch | LA64Exception::PageNotExecutable => {
                    PageFaultType::Execute
                },
                LA64Exception::PageInvalidLoad
                | LA64Exception::PageNotReadable
                | LA64Exception::PagePrivilegeIllegal => PageFaultType::Read,
                LA64Exception::PageModified | LA64Exception::PageInvalidStore => {
                    PageFaultType::Write
                },
                _ => unreachable!(),
            },
        ));
    },
    _ => {
        kerrln!(
            "({}) user {} aborted with unhandled exception: {:?}, pc: {:#x}, badv: {:#x}\n\ttask return value not implemented yet",
            cur_cpu_id(),
            current_task_id(),
            reason,
            trapframe.era,
            trapframe.badv
        );
        get_current_task().recv_signal(Signal::new(
            SigNo::SIGILL,
            SiCode::Kernel,
            SigInfoFields::Ill(SigFault {
                addr: VirtAddr::new(trapframe.era),
            }),
        ));
    },
}

assert!(IntrArch::local_intr_enabled());
handle_signals(
    trapframe,
    restart_syscall.map(|restart| (restart, syscall_ctx)),
);
```

2. `handle_signals()` 拉取 pending signal，并把每个 signal 交给 `perform_signal_action()`。只有用户自定义 handler 会返回 `true` 并停止循环；默认动作会直接调用默认终止函数。这次 backtrace 落在 `perform_signal_action()` 的 custom-handler 分支，所以问题集中在“准备用户态 signal frame”的失败处理：

```rust
pub fn handle_signals(
    trapframe: &mut TrapFrame,
    mut restart_syscall: Option<(RestartSyscall, SyscallCtx)>,
) {
    let mut committed_handler_frame = false;
    loop {
        if let Some(signal) = get_current_task().fetch_signal() {
            if perform_signal_action(signal, trapframe, &mut restart_syscall) {
                committed_handler_frame = true;
                break;
            }
        } else {
            break;
        }
    }

    if !committed_handler_frame {
        get_current_task().restore_temporary_sig_mask_if_pending();
    }
}
```

3. `perform_signal_action()` 的 custom-handler 分支会先修改当前 signal mask，选择普通用户栈或 altstack，编码 `ucontext`，然后算出 `sigframe_base` 并锁住当前 task 的 `UserSpace` 来写 `RtSigFrame`：

```rust
let frame = RtSigFrame {
    siginfo: signal.to_linux_siginfo(),
    ucontext,
};

let sigframe_base = VirtAddr::new(align_down_power_of_2!(
    init_sp - size_of::<RtSigFrame>() as u64,
    16
) as u64);
{
    let usp = task.clone_uspace_handle();
    let mut guard = usp.lock();
    match UserWritePtr::<RtSigFrame>::try_new(sigframe_base, &mut guard) {
        Err(e) => {
            knoticeln!(
                "perform_signal_action: failed to write sigframe to {} user stack: {:?}",
                task.tid(),
                e
            );
            kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
        },
        Ok(mut uptr) => {
            uptr.write(frame);
        },
    }
}
```

这里的 bug 是作用域和发散调用的组合：`guard` 是 `MutexGuard<UserSpace>`，只会在这个 block 结束时 drop；但 `kernel_exit_group()` 返回类型是 `!`，它不会返回到当前 block。因此进入 `kernel_exit_group()` 前，`guard` 仍然活着，`UserSpace` mutex 的 owner 仍是当前 task。这个路径的意图是“用户栈不可写，给进程发 `SIGSEGV` 终止”，但实际执行成了“持有 `UserSpace` 锁进入完整退出协议”。

4. `kernel_exit_group()` 更新 thread-group lifecycle 后，最终调用 `kernel_exit(code)`。它没有、也不应该知道调用方还握着 `UserSpace` 锁：

```rust
pub fn kernel_exit_group(code: ExitCode) -> ! {
    {
        let task = get_current_task();
        if task.tid() == Tid::INIT {
            panic!("init task shall not exit");
        }
        let tg = task.get_thread_group();
        let is_exiting = tg.update_life_cycle_with(|prev| match prev {
            ThreadGroupLifeCycle::Alive => (ThreadGroupLifeCycle::Exiting(code), false),
            ThreadGroupLifeCycle::Exiting(existing_code) => {
                (ThreadGroupLifeCycle::Exiting(*existing_code), true)
            },
            ThreadGroupLifeCycle::Exited(code) => {
                panic!("thread group already exited with code {:?}", code);
            },
        });

        if is_exiting {
            drop(tg);
            drop(task);

            kernel_exit(code)
        }

        tg.for_each_member(|member| {
            if member.tid() != task.tid() {
                member.recv_signal(Signal::new(
                    SigNo::SIGKILL,
                    SiCode::Kernel,
                    SigInfoFields::Kill(SigKill {
                        pid: task.tgid(),
                        uid: task.cred().uid.real,
                    }),
                ))
            }
        });
    }
    kernel_exit(code)
}
```

5. `kernel_exit()` 的前段会处理 Linux `set_tid_address()` / `CLONE_CHILD_CLEARTID` 语义：如果当前 task 有 `clear_child_tid`，退出时要把该用户地址写 0，并对同地址 futex wake。写用户地址需要再次锁同一个 `UserSpace`：

```rust
if let Some(addr) = task.get_clear_child_tid() {
    let usp = task.clone_uspace_handle();
    let cleard = {
        let mut guard = usp.lock();
        match UserWritePtr::<Tid>::try_new(addr, &mut guard) {
            Ok(mut uptr) => {
                uptr.write(Tid::new(0));
                true
            },
            Err(e) => {
                knoticeln!(
                    "failed to clear child tid for task {}: {:?} at address {:#x}",
                    task.tid(),
                    e,
                    addr.get()
                );
                false
            },
        }
    };
    if cleard {
        if let Err(e) = futex::wake_at(&task.clone_uspace_handle(), addr, 1) {
            // ...
        }
    }
}
```

因为第 3 步的 `guard` 还没释放，这里 `usp.lock()` 会在同一个 task 上递归锁同一个 mutex。

6. `Mutex<UserSpace>::lock()` 显式禁止同 task 递归加锁。它用 `locker` 记录当前 locker 的 task pointer，并在锁入口处断言当前 task 不是已有 locker：

```rust
pub fn lock(&self) -> MutexGuard<'_, T> {
    assert!(!in_hwirq(), "Mutex cannot be locked in hwirq context");
    assert!(
        IntrArch::local_intr_enabled(),
        "Mutex cannot be locked when interrupts are disabled"
    );
    assert!(
        allow_preempt(),
        "Mutex cannot be locked when preemption is disabled"
    );
    assert!(
        self.locker.load(Ordering::Acquire) != Arc::as_ptr(&get_current_task()) as usize,
        "Mutex cannot be locked recursively"
    );
    // ...
}
```

因此 panic 文本和源码完全吻合：不是死锁等待，也不是 page fault 本身损坏了内核状态，而是同一个 task 在 signal 错误路径持有 `UserSpace` guard 时进入 exit，exit 又重入需要 `UserSpace` 的清理动作。

**Why `crash01` Exposes It:** `crash01` 会反复制造用户异常，并安装/使用 handler 来验证进程 crash 行为。只要某次 signal delivery 需要把 `RtSigFrame` 写到一个不可写、未映射、越界或已经被测试故意破坏的用户栈/altstack，`UserWritePtr::<RtSigFrame>::try_new()` 就会失败。正确行为应该是放弃继续投递 handler，并让进程以 `SIGSEGV` 终止；当前行为是在失败分支马上进入 `kernel_exit_group()`，但忘了先释放 signal frame 写入时持有的 `UserSpace` mutex。

**Impact:** 这是 signal delivery failure path 与 exit cleanup 的锁生命周期 bug。它把一个用户态可恢复的测试失败表面升级成内核 panic，并截断整个 LTP profile。影响不局限于 `crash01`：任何会让 handler sigframe 写失败的路径都可能触发，包括损坏普通用户栈、损坏 altstack、handler 地址/栈组合异常、同步 fault 转 signal 后再次 fault，以及带 `clear_child_tid` 的线程在这些路径上退出。由于 `clear_child_tid` 是线程/clone 常见状态，这个 bug 会污染 signal、clone/futex exit、fault-to-signal 和 LTP crash 类用例的判断。

**Fix Direction:** `perform_signal_action()` 不能在持有 `UserSpace` mutex、用户指针 guard 或其它 exit 路径可能重入的睡眠锁时调用 `kernel_exit_group()` / `kernel_exit()`。最小修复是把 sigframe 写入结果先保存到局部变量，让 `usp`/`guard` 的 block 明确结束，再在 block 外根据失败结果调用 `kernel_exit_group(SIGSEGV)`。更系统的修复应检查所有 signal / `rt_sigreturn` / fault-to-signal 的发散退出调用，确认没有同类“持锁进入 exit”路径。

**Owner:** EDGW_
**Last Verified:** 2026-06-16
**Exit Condition:** sigframe 写失败、`rt_sigreturn` 坏 frame、fault-to-signal 失败等路径均不在持有 `UserSpace` mutex 或用户访问 guard 时进入 exit；`crash01` 不再以 `Mutex<UserSpace>::lock -> kernel_exit -> kernel_exit_group -> handle_signals` panic 截断；定向用例覆盖不可写 signal stack / altstack、带 `clear_child_tid` 的线程退出，以及用户异常风暴下 handler-frame 写失败的场景。

**Related:** [SHMAT1 SIGILL revalidation](#ane-20260602-shmat1-sigill-masks-segv-hang-revalidation), [Signal LTP remaining semantics](#ane-20260607-signal-ltp-remaining-semantics), [当前限制：Signal LTP infra](./current-limitations.md#ane-20260607-signal-ltp-infra-stage1)

**Severity:** High
**Workaround:** 修复前，不要把 `crash01` 里的大量 `InvalidInstruction` / page fault 日志当成内核崩溃根因；真正停止点是 sigframe 写失败后的 exit lock recursion。需要继续跑长 profile 时，可以暂时隔离 `crash01` 或其它会破坏 signal stack 的用例，避免该 panic 截断后续测试结果。

## ANE-20260616-RV64-FTEST-SHADOW-PARENT-CORRUPTION

**Type:** Issue
**Status:** Open / Unresolved
**Area:** mm / uspace / VMO / fork COW / LTP

**Symptom / Trigger:** `etc/log-rv-illegal-read02.log` 在 `ftest01` 失败结束后，刚打印 `RUN LTP CASE ftest02` 就触发 rv64 kernel page fault：`pc=VirtAddr(0xffffffff802bed48), addr=VirtAddr(0x10), type=Read`。这次定位必须以 `read02` 的地址为准；`etc/log-rv-illegal-read01.log` 也有同类 `addr=0x10` kernel read fault，但那份日志对应的内核已经和当前代码偏离，里面的 `pc=0xffffffff802bd970` 只能作为“同类形态”参考，不能再作为精确源码定位依据。

**Confirmed Evidence:** `read02` 的 panic 发生在 fork memory diagnostic 路径，而不是 `ftest02` 自身执行路径。`task/api/clone/mod.rs` 的 `report_fork_memory()` 会在 fork publish 后调用 parent / child 的 `UserSpace::memory_report()`；`UserSpace::memory_report()` 遍历 VMA，先通过 `vma.backing().memory_report_kind()` 取得 report kind，再调用 backing 的 `fill_memory_report()`。`read02` 的 `0xffffffff802bed48` 对应当前源码中 `anemone-kernel/src/mm/uspace/vmo/shadow.rs` 的 `ShadowObject::memory_report_kind()` 等价位置，即：

```rust
fn memory_report_kind(&self) -> Option<VmMemoryReportKind> {
    self.parent.memory_report_kind()
}
```

fault 地址是 `0x10`，这符合对损坏的 trait object / fat pointer 做动态派发时读取 vtable 槽位的形态。外层 `vma.backing()` 已经成功派发到了 `ShadowObject`，说明 `VMA.backing` 本身还能被当作 `ShadowObject` 调用；真正异常集中在 `ShadowObject` 对象体内部，尤其是 `parent: Arc<dyn VmObject>` 的 data/vtable word 可能已经被写坏、清零，或者该 `ShadowObject` 所在内存已经被释放后复用。

**LTP Context:** `read02` 中 `ftest01` 先启动并创建 5 个子进程。LTP 源码 `etc/testsuits-for-oskernel/ltp-full-20240524/testcases/kernel/fs/ftest/ftest01.c` 显示该用例默认让每个 child 操作不同文件，混合 `lseek`、`read`、`write`、`ftruncate`、`fsync`、`sync` 和 `fstat`。日志里的 `libftest.c:78: ft_dumpbits: Assertion '0 < (buf - bits)' failed` 是 `ft_dumpbits()` 诊断打印里的断言，说明测试已经检测到文件内容 bitmap / pattern 不一致后试图 dump bits；它不是当前已证明的内核对象损坏点。`ftest02` 源码会做目录和 inode 操作并 fork children，但本次 panic 在 `RUN LTP CASE ftest02` 之后立即发生，尚未看到 `ftest02` 自己的 child 操作日志，因此不能把根因归到 `ftest02` 的目录操作。

**Current Analysis:** 当前只确认 `ShadowObject.parent` 已呈现损坏或 UAF 症状，尚未确认“为何损坏”。`ftest01` 可能只是让 user-test 长 fork 链、ramfs 文件 I/O、truncate/fsync/sync 和退出/reap 交错达到触发条件；它不直接说明普通文件 `read/write` 把文件页缓存零拷贝映射给用户并写坏内核对象。`read02` 的 fork memory 报告还显示 user-test 父进程已有很深的 COW shadow 链，例如 stack shared 达到 1929 pages；这会放大 memory-report 对 shadow parent 链的遍历深度，但仅靠“链很长”还不能解释 parent fat pointer 被破坏。仍需重点排查 VMA/VMO 生命周期、fork COW 链所有权、deferred unmap 与 backing drop 顺序、truncate/discard 对已安装 PTE 的处理，以及 `ShadowObject` drop / allocation reuse 是否存在 UAF 窗口。

**Impact:** 该问题会把 LTP fs profile 截断在 `ftest01` / `ftest02` 附近，并且 panic 发生在内核诊断遍历 COW backing 时。由于表现是内核对象内部指针损坏，它比单个 LTP 用例失败更严重：后续任何 fork memory report、`/proc` memory accounting、OOM/debug 统计，或者其它遍历 VMA backing 链的路径都可能在同一类损坏后 panic。当前不能把 `ftest01` 的文件内容失败、`ftest02` 的启动点、或 `read01` 的旧地址单独作为根因。

**Owner:** EDGW_
**Last Verified:** 2026-06-16
**Exit Condition:** 为 `ShadowObject` 和 `Arc<dyn VmObject>` backing 链补充足够诊断，能够区分“对象体被覆盖”和“对象已经 drop 后被复用”；定位并修复实际破坏来源；确认 VMA/VMO drop、discard/truncate、deferred PTE unmap、fork COW 链和 memory-report 遍历之间没有 stale backing 或 stale PTE 窗口；使用包含 `ftest01`、`ftest02` 以及 `read01` 同类 fsx/fs profile 的 rv64 回归复跑，证明不再出现 `ShadowObject::memory_report_kind()` / `addr=0x10` kernel read panic，并重新分类 `ftest01` 的文件内容失败。

**Related:** [开发日志：2026-06-08 至 2026-06-21](../devlog/2026-06-08_to_2026-06-21.md), [当前限制：truncate mmap coherency](./current-limitations.md#ane-20260523-truncate-mmap-coherency), [当前限制：file-backed mmap fault stage-1](./current-limitations.md#ane-20260529-file-backed-mmap-fault-stage1), [当前限制：mremap anon-only](./current-limitations.md#ane-20260527-mremap-anon-only)

**Severity:** High
**Workaround:** 当前没有可信修复性绕过。为了继续跑长 profile，可以临时隔离 `ftest01` / `ftest02` 或关闭 fork memory diagnostic 来避免在 report 路径立即 panic，但这只会隐藏 `ShadowObject` backing 链损坏的观测点，不能证明内存生命周期问题已经消失。

## ANE-20260616-LTP-POST-SUMMARY-HANG

**Type:** Issue
**Status:** Open
**Area:** user-test / LTP / task exit / wait-core / timer / loop cleanup

**Symptom / Trigger:** rv64 长 profile 运行 LTP 时，小概率在单个 case 已经打印完 LTP `Summary` 后卡住，runner 没有继续打印 `PASS LTP CASE ...` 或 `FAIL LTP CASE ...`，也不会进入下一个 case。当前已知例子是 `build/ltp-all-rv.log` 在 `ioctl05` 结束 summary 后停住；单独运行 `ioctl05` 暂未复现。

**Impact:** 一个偶发 case 卡住会阻塞后续 profile，导致本轮 LTP 得分信号和失败矩阵都被截断。该现象目前不能简单归类为 `wait4` / `waitid` 错过唤醒：LTP harness 的 summary 输出发生在 testcase cleanup 和最终退出之前，仍需要确认 child 是否已经真正退出。

**Owner:** doruche
**Last Verified:** 2026-06-16
**Exit Condition:** 补充父/子进程状态与 cleanup 阶段观测，证明卡住时 child 是停在 LTP cleanup（例如 loop device detach、timer sleep 或退出路径）还是已经退出但父进程 wait/reap 状态没有收敛；随后按真实根因修复 loop cleanup / timer wait / task exit / wait-core 唤醒或 reaping 语义，并确认长 LTP profile 不再需要 runner timeout 才能推进。
**Related:** [User-test LTP Pgrp Isolation](../devlog/changes/2026-06-07-user-test-ltp-pgrp-isolation.md), [RFC-20260601-sched-wait-refactor](../rfcs/sched-wait-refactor/index.md), [当前限制：IOCTL LTP stage-1 gaps](./current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)

**Severity:** High
**Workaround:** `user-test` LTP runner 暂时使用 per-case timeout；超时后只对该 case 的独立进程组发 `SIGKILL`，把该 case 归为 runner 设施失败并继续执行后续 case。该绕过只保证 profile 能继续推进，不证明内核 wait / timer / cleanup 语义已修复。
