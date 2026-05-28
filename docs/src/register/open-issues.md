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
**Status:** Open
**Area:** fs / openat / readlinkat

**Symptom / Trigger:** `openat` 目前还没有真正支持 `O_NOFOLLOW`，`O_PATH | O_NOFOLLOW` 打开的 symlink 不能稳定保留“指向符号链接本体”的语义，导致 `readlinkat("", ...)` 这类空路径用例仍然失败。

**Impact:** LTP `readlinkat01` 的空路径分支无法按 Linux 预期工作，`AT_EMPTY_PATH` 风格的后续接口也会受影响。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Exit Condition:** 为 `openat` 补上 `O_NOFOLLOW` / `O_PATH` 的路径解析与 fd 语义，并重新验证 `readlinkat01` 的空路径用例。

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

**Severity:** Medium
**Workaround:** 暂时避开依赖 `O_PATH | O_NOFOLLOW` 语义的空路径调用链。

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
