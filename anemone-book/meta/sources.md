# Anemone Book 材料入口

本文记录 `anemone-book` 可引用的材料入口。它不是进度表，也不是事实源；
事实仍以代码、RFC、devlog、register / current limitations 和外部原始来源为准。

## 仓库内材料

### RFC

待补：按章节列出需要引用的 RFC，例如 scheduler wait、sched-latch、mount tree、
fanotify、ioctl-loop、kthread、threaded timer、fileops seek / char ioctl 等。

第 5 章 VFS、命名空间与 pseudo filesystem：

- `docs/src/rfcs/mount-tree-legacy-api/index.md`、`invariants.md`、`implementation.md`：`PathRef = Mount + Dentry`、`MountTree` topology owner、per-mount readonly、legacy mount API 和 accepted boundary。
- `docs/src/rfcs/proc-tgid-fd/index.md`、`implementation.md`：`/proc/<tgid>/fd` 作为 fd table 观察面，不缓存 `Arc<FileDesc>`，按操作重新验证当前 fd。

第 6 章设备驱动模型与 I/O 对象：

- `docs/src/rfcs/ioctl-loop/index.md`、`invariants.md`：`sys_ioctl()`、`IoctlCtx`、block devfs、loop 和 mount source 的 owner boundary。
- `docs/src/rfcs/fileops-seek-char-ioctl/index.md`、`invariants.md`：`FileOps::seek` / positioned I/O、`BackingFileHandle`、`CharDev` seek / ioctl narrow ctx。

第 3 章任务、进程与执行上下文：

- `docs/src/rfcs/cred-merge/index.md`：task credentials、exec credential commit、VFS/exec/syscall ABI 合并边界。

第 4 章调度、等待与时间：

- `docs/src/rfcs/sched-wait-refactor/index.md`：wait identity、single completion transaction、stale-safe wake placement 和 Event / timeout / signal 边界。
- `docs/src/rfcs/sched-latch/index.md`：`Latch`、`LatchTrigger`、poll/select OR wait、typed source register 和 final readiness scan 边界。
- `docs/src/rfcs/threaded-timer-event/index.md`：IRQ timer lane、threaded timer lane、timerfd / ITIMER_REAL 迁移与 wait-core timeout 非目标边界。

第 8 章体系结构、Trap 与平台边界：

- `docs/src/rfcs/signal-temp-mask-restore/implementation.md`：trap-return signal delivery 与 temporary mask restore 的边界；cleanup 责任保持在 signal 模块内部，不下沉到各 arch trap-return 层。
- `docs/src/rfcs/threaded-timer-event/implementation.md`：timer interrupt lane、threaded timer handoff 与 IRQ lane 的 source-audit / validation 边界。

### Devlog / Transaction Devlog

待补：按章节列出需要引用的 transaction devlog、small change record 和双周
devlog 入口。

第 5 章 VFS、命名空间与 pseudo filesystem：

- `docs/src/devlog/transactions/2026-06-18-mount-tree-legacy-api.md`：mount tree legacy API staged implementation、review gate、accepted limitation closeout。
- `docs/src/devlog/transactions/2026-06-04-proc-tgid-fd.md`：`/proc/<tgid>/fd` stage-1 directory、readlink 和 residual limitations。
- `docs/src/devlog/changes/2026-06-14-procfs-sysctl-pde-tree.md`：procfs PDE static tree 与第一批只读 `/proc/sys/kernel` 节点。
- `docs/src/devlog/changes/2026-06-10-fileops-status-ctx.md`、`docs/src/devlog/changes/2026-06-13-vfs-stream-file-mode.md`：opened-description status snapshot 与 stream file mode 边界。

第 6 章设备驱动模型与 I/O 对象：

- `docs/src/devlog/transactions/2026-06-04-ioctl-loop.md`：VFS ioctl 分发、block devfs `BLK*`、loop device pool 和 loop private ioctl。
- `docs/src/devlog/transactions/2026-06-05-fileops-seek-char-ioctl.md`：`FileOps` seek / positioned I/O、loop backing handle、char device seek / ioctl hook。
- `docs/src/devlog/changes/2026-06-05-block-byte-io-loop-mkfs.md`：block byte I/O helper 与 loop / mkfs 相关小迭代证据。

第 3 章任务、进程与执行上下文：

- `docs/src/devlog/transactions/2026-06-02-cred-merge.md`：credentials merge 的执行事实、reviewer P0/P1 和 task / exec / VFS 边界。
- `docs/src/devlog/changes/2026-06-14-waitid.md`：exited-child `waitid` bridge、字段级 `siginfo_t` 写回和非 exit wait state 边界。

第 4 章调度、等待与时间：

- `docs/src/devlog/transactions/2026-06-01-sched-wait-refactor.md`：scheduler wait refactor 阶段事实、`TaskStatus` observation-only 边界和 retained entry 分类。
- `docs/src/devlog/transactions/2026-06-03-sched-latch.md`：`ppoll` / `pselect6` latch migration、source register gate 和 resolved iomux limitation。
- `docs/src/devlog/transactions/2026-06-20-threaded-timer-event.md`：threaded timer event gate、timerfd / ITIMER_REAL validation 和 post-summary hang non-closure 边界。

第 8 章体系结构、Trap 与平台边界：

- `docs/src/devlog/transactions/2026-06-06-signal-temp-mask-restore.md`：signal handler frame commit、arch `prepare_trapframe_for_signal_handler()` 调用点、cleanup 未下沉到 riscv64 / loongarch64 trap-return 层的执行记录。
- `docs/src/devlog/transactions/2026-06-20-threaded-timer-event.md`：threaded timer event 的 IRQ lane / worker handoff 记录，供本章提及 IRQ-off tail 边界时核对。

### Register / Current Limitations

第 5 章 VFS、命名空间与 pseudo filesystem：

- `docs/src/register/current-limitations.md`：`ANE-20260522-OTMPFILE-STAGE1`、`ANE-20260523-TRUNCATE-MMAP-COHERENCY`、`ANE-20260528-OPATH-STAGE1-CAPABILITIES`、`ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING`、`ANE-20260529-MEMORY-LTP-PROCFS-DEVZERO-RLIMIT-STAGE1`、`ANE-20260529-SYSV-SHM-LTP-INFRA-STAGE1`、`ANE-20260528-ROFS-DIRECT-WRITE-STAGE1`、`ANE-20260619-MOUNT-PROPAGATION-STAGE1`、`ANE-20260619-MOUNT-FLAG-MATRIX-STAGE1`、`ANE-20260619-MOUNT-FSTYPE-ALIAS-BRIDGE`、`ANE-20260619-MOUNT-UNMOUNT-CLEANUP-STAGE1`。
- `docs/src/register/open-issues.md`：`ANE-20260528-EXEC-ETXTBSY-WRITER-ACCOUNTING` 作为 VFS/open-file accounting residual issue。

第 6 章设备驱动模型与 I/O 对象：

- `docs/src/register/current-limitations.md`：`ANE-20260524-DEVFS-STATIC-PUBLISH`、`ANE-20260524-DEVFS-BLOCK-DEFAULT-SEMANTICS`、`ANE-20260605-DEVFS-CHAR-SEEK-IOCTL-STAGE1`、`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`。
- `docs/src/register/open-issues.md`：`ANE-20260527-LTP-CHDIR01-DEVICE-POOL` 作为 device pool / user-test 环境 residual issue。

第 3 章任务、进程与执行上下文：

- `docs/src/register/current-limitations.md#ane-20260527-process-group-session-stage1`：process group / session / job-control stage-1 边界。
- `docs/src/register/current-limitations.md#ane-20260602-clone3-stage1-adapter`：`clone3` pidfd / cgroup / pid namespace / set_tid 非目标边界。
- `docs/src/register/current-limitations.md#ane-20260607-signal-ltp-infra-stage1` 与 `docs/src/register/open-issues.md#ane-20260607-signal-ltp-remaining-semantics`：signal ABI / scheduler / setup observability 边界。

第 4 章调度、等待与时间：

- `docs/src/register/open-issues.md#ane-20260606-rt-sigtimedwait-async-waited-signal-eintr`：同步 signal wait completion 后结果分类缺口。
- `docs/src/register/open-issues.md#ane-20260616-ltp-post-summary-hang`：task exit / wait-core / timer / cleanup hang 的未归因边界。
- `docs/src/register/open-issues.md#ane-20260622-irq-off-heap-allocation`：IRQ-off / timer / scheduler path allocation audit 边界。

第 7 章内存管理与 memory object：

- `docs/src/register/current-limitations.md`：`ANE-20260523-TRUNCATE-MMAP-COHERENCY`、`ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION`、`ANE-20260529-FILE-BACKED-MMAP-FAULT-STAGE1`、`ANE-20260525-SYSV-SHM-MUNMAP-DETACH`、`ANE-20260525-SYSV-SHM-LOCK-RESIDENCY-STAGE1`、`ANE-20260525-SYSV-SHM-PERMISSIONS-PENDING-CREDENTIALS`、`ANE-20260529-SYSV-SHM-LTP-INFRA-STAGE1`、`ANE-20260527-MREMAP-ANON-ONLY`、`ANE-20260529-MEMORY-LTP-PROCFS-DEVZERO-RLIMIT-STAGE1`、`ANE-20260528-ROFS-DIRECT-WRITE-STAGE1`。
- `docs/src/register/open-issues.md`：`ANE-20260527-MMAP-MPROTECT-HEAP-FASTPATH-PERSISTENCE`、`ANE-20260527-MADVISE-DONTNEED-LOCKED-SHARED`、`ANE-20260602-SHMAT1-SIGILL-MASKS-SEGV-HANG-REVALIDATION`。

第 8 章体系结构、Trap 与平台边界：

- `docs/src/register/open-issues.md`：`ANE-20260608-RISCV-FPU-TRAP-RETURN-UNSAFE-BOUNDARY` 记录 rv64 trapframe alignment / FPU lazy-enable 的修复已落地但仍需 revalidation；`ANE-20260616-LTP-POST-SUMMARY-HANG` 的 IRQ-off allocation audit 相关段落记录 hard IRQ / trap return tail 的待审边界。

### 代码模块

第 5 章 VFS、命名空间与 pseudo filesystem：

- `anemone-kernel/src/task/files.rs`：`FileDesc`、opened file description、fd-local flags、file status flags 与 ioctl/fcntl access snapshot。
- `anemone-kernel/src/fs/file.rs`：`File`、`FileOps`、`FileIoCtx`、`FileOpStatusFlags`、`IoctlCtx`、`BackingFileHandle`。
- `anemone-kernel/src/fs/path.rs`、`anemone-kernel/src/fs/mount/{tree.rs,view.rs,flags.rs}`、`anemone-kernel/src/fs/namei.rs`：`PathRef`、`Mount`、`MountTree`、mount attrs 与 lookup generation retry。
- `anemone-kernel/src/fs/{inode.rs,dentry.rs,filesystem.rs}`：filesystem backend、inode identity 与 dentry boundary。
- `anemone-kernel/src/fs/proc/**`：procfs singleton superblock、PDE static tree、tgid dynamic tree、`fd`、`mounts` 和 sysctl nodes。
- `anemone-kernel/src/fs/devfs/**`：本章只作为 pseudo fs bridge 原则来源；设备模型细节归第 6 章。

第 6 章设备驱动模型与 I/O 对象：

- `anemone-kernel/src/device/{mod.rs,kobject.rs,bus/**}`、`anemone-kernel/src/driver/**`：device / driver / bus 基础形状、platform / virtio / PCIe matching 与 probe。
- `anemone-kernel/src/utils/any_opaque.rs`：`Opaque` / `AnyOpaque` type-erased private state，供 driver-private state 和 IRQ private data 等路径核对。
- `anemone-kernel/src/device/char/{mod.rs,devfs.rs,null.rs,zero.rs,full.rs,urandom.rs}`：`CharDev` registry、devfs publish helper、memory char device seek / ioctl boundary。
- `anemone-kernel/src/device/block/{mod.rs,devfs.rs,loop.rs,ramdisk.rs}`：`BlockDev` registry、block devfs byte I/O / `BLK*` / private ioctl、loop backing handle。
- `anemone-kernel/src/fs/devfs/**`：`DevfsPublish`、`DevfsNodeOps`、devfs singleton publish registry 和 stable inode identity。
- `anemone-kernel/src/fs/api/ioctl.rs`、`anemone-kernel/src/fs/file.rs`：`sys_ioctl()` 到 `FileOps::ioctl` 的 VFS 控制面路径。

第 3 章任务、进程与执行上下文：

- `anemone-kernel/src/task/mod.rs`：`Task`、`TaskStatus`、`ThreadGroup`、`ProcessGroup`、`Session` 的核心形状。
- `anemone-kernel/src/task/topology/mod.rs`：全局 `TaskTopology`、`TaskTopologyInner`
  索引、`TaskBinding`、publish / unpublish 事务、`ThreadGroupType` shape assertion
  和 topology consistency 边界。
- `anemone-kernel/src/task/topology/thread_group.rs` 与 `anemone-kernel/src/task/topology/process_group.rs`：thread-group membership snapshot、topology lock 边界、process-group signal selector。
- `anemone-kernel/src/task/api/clone/clone3.rs`：`clone3` adapter、supported flags 和 deferred feature handling。
- `anemone-kernel/src/task/api/wait/`：`wait4` / `waitid` exited-child scan / reap helper 边界。
- `anemone-kernel/src/task/credentials/`：`CredentialSet`、uid/gid/capability snapshot 和权限身份边界。

第 4 章调度、等待与时间：

- `anemone-kernel/src/task/sched.rs`：`TaskStatus` observation-only snapshot、`TaskSchedState` helper 和 scheduler-state transaction 边界。
- `anemone-kernel/src/sched/wait.rs`：`WaitState`、`WakeToken`、`ActiveWait`、`WaitOutcome` 和 stale-safe wake placement。
- `anemone-kernel/src/sched/event.rs` 与 `anemone-kernel/src/sched/latch.rs`：Event adapter、`Latch` / `LatchTrigger` 和 producer capability 边界。
- `anemone-kernel/src/sched/processor.rs` 与 `anemone-kernel/src/task/cpu_usage.rs`：per-CPU run queue、placement API、runtime accounting 和 CPU usage snapshot。
- `anemone-kernel/src/fs/api/iomux/wait.rs`：`ppoll` / `pselect6` shared latch wait loop、register abort final scan 和 outcome mapping。
- `anemone-kernel/src/time/clock/`、`anemone-kernel/src/time/timer/`、`anemone-kernel/src/fs/timerfd.rs`、`anemone-kernel/src/task/itimer.rs`：clock table、IRQ/threaded timer lanes、timerfd state owner 和 `ITIMER_REAL` state owner。

第 7 章内存管理与 memory object：

- `anemone-kernel/src/mm/uspace/{mod.rs,vma.rs,mmap.rs,fault.rs}`：`UserSpace`、`VmArea`、VMA 编辑、page fault 和 heap / stack reservation。
- `anemone-kernel/src/mm/uspace/vmo/{mod.rs,anon.rs,shadow.rs,fixed.rs}`：`VmObject`、匿名页、copy-on-write shadow object、固定 ELF segment backing。
- `anemone-kernel/src/mm/uspace/api/{mmap.rs,brk.rs,mremap.rs,msync.rs,madvise.rs,mprotect.rs}`：Linux-visible mmap/brk/mremap/msync/madvise/mprotect syscall boundary。
- `anemone-kernel/src/fs/{inode.rs,cache_stats.rs}`、`anemone-kernel/src/fs/{ext4,ramfs}/file.rs`：inode mapping、resident backing file cache counter、ext4 / ramfs file-backed mapping 与 truncate/page-cache 交互。
- `anemone-kernel/src/mm/uspace/shm/{object.rs,segment.rs,registry.rs,permission.rs,api/}`：SysV shm segment registry、shared backing object、attach/detach、credentials hook 和 Linux-visible shmctl metadata。
- `anemone-kernel/src/mm/oom.rs`、`docs/src/rfcs/oom-killer/`、`docs/src/rfcs/inode-shrinker/`：内存压力下独占物理页 snapshot 与 inode/page-cache 回收边界。

第 8 章体系结构、Trap 与平台边界：

- `anemone-kernel/src/arch/{riscv64,loongarch64}/bootstrap.rs`、`anemone-kernel/src/main.rs`：`__nun` / `rusty_nun`、stage-1 bootstrap、`bsp_kinit` / `ap_kinit` handoff、rootfs mount 与 initial `execve` 路径。
- `anemone-kernel/src/arch/mod.rs`、`anemone-kernel/src/exception/trap/hal.rs`、`anemone-kernel/src/sched/hal.rs`、`anemone-kernel/src/mm/paging/hal.rs`、`anemone-kernel/src/time/hal.rs`、`anemone-kernel/src/task/sig/hal.rs`：HAL trait 由功能模块定义，arch 实现并 re-export 的依赖反转边界。
- `anemone-kernel/src/arch/mod.rs`、`anemone-kernel/src/exception/trap/hal.rs`、`anemone-kernel/src/syscall/{mod.rs,handler.rs}`：arch re-export、trapframe/syscall context trait、syscall table 与 arch handoff。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/{mod.rs,utrap.rs,signal.rs}`：trapframe layout、trap entry/return、syscall / exception / interrupt dispatch、signal trapframe setup。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/fpu.rs`：lazy FPU context、save/load assembly、layout guard 和 arch-specific enable bit。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/intr.rs`、`anemone-kernel/src/exception/intr/irq.rs`、`anemone-kernel/src/time/timer/irq.rs`：local interrupt control、IPI / timer / external IRQ dispatch、IRQ domain 和 IRQ lane boundary。
- `anemone-kernel/src/task/kthread/`、`anemone-kernel/src/initcall.rs`、`anemone-kernel/src/mm/uspace/fault.rs`：kthread bootstrap、late initcall window、generic fault path 与 arch handoff。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/machine/`、`anemone-kernel/src/device/bus/platform/`、`anemone-kernel/src/device/discovery/`：machine descriptor、DTB compatible matching、root IRQ/timer early init、platform bus bridge。

## 外部材料

### Linux

待补：记录 Linux ABI、man-pages、kernel docs 或源码参考入口。书稿引用 Linux
时应区分“用户可见 ABI 参考”和“内部实现启发”。

### Zircon / Fuchsia

待补：记录 VMO、object model、handle / capability 风格等相关参考入口。
书稿不得暗示 Anemone 已完整实现 Zircon-style VMO。

### Rust / OS 设计

待补：记录 Rust unsafe boundary、OS 设计、文件系统、调度、内存管理等可引用
资料。

### 引语候选

当前正文使用下列章首 epigraph。冻结前必须统一核对原文出处；无法核对稳定来源
的候选应改为转述或删除。

- §0 前言：Kent Beck, “Make it work, make it right, make it fast.”
- §1 设计理念与系统地图：Fred Brooks, “Conceptual integrity is the most important consideration in system design.”
- §2 ABI 边界与系统调用层：Jon Postel, “Be conservative in what you do, be liberal in what you accept from others.”
- §3 任务、进程与执行上下文：Butler Lampson, “Hints are often better than algorithms.”
- §4 调度、等待与时间：Edsger W. Dijkstra, “Simplicity is prerequisite for reliability.”
- §5 VFS、命名空间与 Pseudo Filesystems：David Wheeler, “All problems in computer science can be solved by another level of indirection.”
- §6 设备驱动模型与 I/O 对象：Rob Pike, “A little copying is better than a little dependency.”
- §7 内存管理与 memory object：Alan Kay, “Simple things should be simple, complex things should be possible.”
- §8 体系结构、Trap 与平台边界：C. A. R. Hoare, “There are two ways of constructing a software design: make it so simple that there are obviously no deficiencies, or make it so complicated that there are no obvious deficiencies.”
