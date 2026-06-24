# The Anemone Book Outline

本文是 `anemone-book` 的章节结构和覆盖检查表。它不是进度账本，也不是
Anemone 的事实源；正文事实仍以代码、RFC、devlog、register / current
limitations 和外部原始来源为准。

## 章节树

### 前言：为什么是 Anemone

**核心论点：** Anemone 的价值不在于复刻某个既有内核，而在于在 Linux ABI
兼容、Rust 约束和工程纪律之间走出一条可维护的系统设计路线。

**覆盖范围：**

- 本书的读者契约和叙述边界。
- Anemone 不是 Linux 的 Rust 复刻，也不是 syscall patch collection。
- Linux、Zircon / Fuchsia、Rust、开源社区和经典 OS 设计资料的影响与致谢。
- 本书不是手册，而是设计叙述快照。

**代表路径 / case study：** 不设置技术路径，保持为人味、背景和读者契约。

**图候选：** 暂无。前言不强行放图。

**代码片段候选：** 无。

**边界提示：** 不进入内核结构细节；不讨论比赛得分或测例矩阵。

### 1. 设计理念与系统地图

**核心论点：** Anemone 的外部约束包括 Linux ABI compatibility surface 和
Anemone-native UAPI surface，内部组织原则是 native object model、owner
boundary 和 Rust as a design constraint；全书后续章节都是这张地图的展开。

**覆盖范围：**

- 目标与非目标。
- Linux ABI surface、Anemone-native UAPI surface 与 Anemone native internal contract。
- Anemone 不是 Linux 的 Rust 复刻。
- Rust as a Design Constraint。
- 子系统地图。
- 重要对象和 owner boundary 总览。
- single source of truth、stage-aware compatibility、observability。
- 后续章节阅读方式。

**代表路径 / case study：**

- 一个 Linux-visible syscall 如何穿过 adapter，落到内部对象和 owner。

**图候选：**

- `Linux ABI、native UAPI 与 internal contract 共享同一组内核对象`。
- `Anemone 的主要子系统围绕对象 owner 而不是 Linux 源码目录组织`。
- `Rust 类型边界把 owner、handle、ctx 和 snapshot 分开`。

**代码片段候选：**

- 一个简短的 typed handle / ctx / owner API 形状。
- 一个 `assert!` 表达局部 correctness invariant 的例子。

**边界提示：**

- 不承诺完整 Linux kernel 语义。
- 不把 Rust 写成自动安全叙事。

### 2. ABI 边界与系统调用层

**核心论点：** Anemone 同时管理 Linux-visible syscall surface 和
Anemone-native syscall/UAPI surface；syscall adapter 把 UAPI、flag、errno、
结构体解析和 native 控制面限制在边界层，避免用户可见协议污染内部 owner。

**覆盖范围：**

- syscall dispatch。
- Linux UAPI、Anemone-native UAPI 与内部 API 的分层。
- errno、flag、struct compatibility。
- stage-aware compatibility。
- silent compatibility 的日志、注释和退出条件边界。
- native syscall/UAPI 作为受控扩展面，不伪装成 Linux ABI。
- native internal contract。
- 链接段注册表分发机制。
- Rust 过程宏如何收束 syscall metadata、分发表、`preparse`、validator 和
  `TryFromSyscallArg` 边界样板。

**代表路径 / case study：**

- `openat`：flag parsing、file status、VFS 边界。
- `ioctl`：Linux 控制面如何路由到真实 owner。
- Anemone-native syscall：调试输出、关机控制等能力如何形成受控扩展面。
- syscall macro / link-section registry：分散定义如何生成统一分发入口和参数解析样板。

**图候选：**

- `链接段注册表把分散 syscall 定义收束为统一分发表`。
- `syscall adapter 只翻译 Linux ABI，不拥有内部对象语义`。

**代码片段候选：**

- syscall registration macro 的使用形状。
- `preparse`、`#[validate_with(...)]` 和 `TryFromSyscallArg` 的短例子。
- syscall wrapper 调用内部 typed API 的短例子。

**边界提示：**

- 不展示宏展开后的长代码。
- 不列完整 syscall 表。
- 不展开完整 errno matrix。
- 不把 native syscall 写成绕过 Linux 兼容缺口的捷径。

### 3. 任务、进程与执行上下文

**核心论点：** Anemone 用 task 表达可运行的执行上下文，用中心化全局 task
topology 维护 thread group、process group 和 session 等 Linux-visible
关系；这把内存对象一致性和更高一级的拓扑一致性分开，使 task 不直接拥有
调度策略、等待协议或整套 process topology。

**覆盖范围：**

- task 与 TCB / user context。
- 全局 task topology 作为 TID / TGID / PGID / SID membership、publish /
  unpublish 事务和 topology consistency 的 owner。
- thread group、process、process group、session。
- memory object consistency 与 topology consistency 的区别。
- credentials 与权限身份。
- fork / clone / exec / exit 的高层生命周期。
- signal model 中和 task / process topology 相关的部分。
- user/kernel context 的 generic 表达。

**代表路径 / case study：**

- `clone` / `clone3` 到 task / thread group 建立。
- `TaskBinding::{UserLeader, Member, KThread}` 如何把不同执行实体发布到全局
  topology。
- `execve` 或 `exit` 如何改变执行上下文与生命周期。
- signal target selection 如何依赖 task / thread group topology。

**图候选：**

- `Task 提供身份和执行上下文，scheduler / wait-core 拥有 runnable 与 blocking 协议`。
- `全局 task topology 维护线程组、进程组和会话的一致性，而不是把拓扑关系缓存进每个 TCB`。
  占位建议：`assets/sources/ch03/global-task-topology.drawio` /
  `assets/figures/ch03/global-task-topology.png`，表达 object consistency 与
  topology consistency 的分层、`TOPOLOGY` 全局索引、`TaskBinding` publish
  事务和 topology lock order。

**代码片段候选：**

- task binding / thread group type 的简短 type shape。
- `TaskTopologyInner` 与 `TaskBinding` 的短片段，用来支撑中心化 topology owner。
- credentials snapshot / capability check 的接口形状。
- `Task` 字段形状中 `cred`、signal context、`sched_state` 与 observation-only `TaskStatus` 的边界。
- `CredentialSet` 的 uid/gid/groups/capability snapshot 形状。

**边界提示：**

- scheduler 和 wait-core 不放在本章作为主语。
- signal delivery 对 wait / trap-return 的影响放到第 4 章或第 8 章衔接。
- `clone3` 的 pidfd / cgroup / pid namespace / set_tid 仍是明确 deferred feature。
- process group / session 当前不宣称完整 POSIX job-control。

### 4. 调度、等待与时间

**核心论点：** scheduler 拥有 runnable state 和 CPU 选择，wait-core 拥有阻塞
协议，timer 提供时间触发；task 只是被调度和被唤醒的对象，不应成为第二套
等待 / 调度真相源。

**覆盖范围：**

- scheduler runnable state。
- CPU selection 与调度策略边界。
- wait-core、latch、event、poll / select OR wait。
- timeout、timer、itimer、timerfd 作为时间触发与 notification 例子。
- signal wake / interrupted wait / trap-return delivery 的交互。
- 不变量：先发布等待能力，再让出 CPU；wake 后由 owner 进行最终状态分类。

**代表路径 / case study：**

- sleep / wakeup 路径。
- `ppoll` / `pselect6` OR wait。
- signal interruption 如何影响 blocked syscall。

**图候选：**

- `Sleep / Wakeup 的关键不变量是先入队再让出 CPU`。
- `timer 提供时间触发，但不接管等待状态 owner`。
- `wait-core 拥有阻塞协议，event source 只发布 wake capability`。
- `ppoll / pselect6 的 OR wait 是一轮 latch，而不是多个 source 自己调度 task`。

**代码片段候选：**

- wait token / latch / trigger 的短接口形状。
- scheduler 只接收 narrow wake capability 的例子。
- 普通 enqueue 与 wait completion `wake_enqueue` 的入口差异。
- `TaskSchedState` / `WakeToken` 的 wait identity 形状。
- `schedule_threaded_timer_event` 的 bounded completion lane 形状。

**边界提示：**

- 时间子系统当前可作为本章一节，不单独成大章。
- `timerfd` 只在能说明 wait-core / file object / clock boundary 时出现。
- wait-core timeout 当前仍留在 IRQ timer lane；迁移 threaded lane 需要独立 RFC 级证明。
- runtime accounting 可以作为调度观测面讲述，但不宣称完整 Linux CFS / EEVDF 策略。

### 5. VFS、命名空间与 pseudo filesystem

**核心论点：** VFS 负责路径、mount view、file object 和 inode 生命周期；
procfs、devfs、未来 sysfs 是 namespace bridge / control surface，不拥有被
暴露对象的核心状态。

**覆盖范围：**

- pathname lookup。
- mount tree / mount view。
- FileDesc / File / Inode / Dentry / Mount 的对象关系。
- VFS 与 filesystem backend 的边界。
- procfs 作为 task、sysctl、runtime state 的 namespace / control surface。
- devfs 在本章只作为 pseudo fs 桥的一般原则出现。
- readonly mount、bind、move、proc mounts 等作为边界或代表路径。

**代表路径 / case study：**

- path lookup + mount view。
- `/proc/<pid>/fd` 如何展示 file object，但不接管 opened file description。
- `/proc/<pid>/mounts` 如何展示 live mount view。

**图候选：**

- `FdTable、File、Inode 构成 VFS 的三层对象模型`。
- `pseudo filesystem 把内核对象接入 namespace，但不接管对象状态`。
- `mount view 决定路径可见性，而不是复制 filesystem object`。
- 占位建议：`assets/sources/ch05-vfs-object-model.drawio` / `assets/figures/ch05-vfs-object-model.svg`，表达 fd table、opened file description、`File`、`PathRef`、`Mount`、`Dentry`、`Inode` 和 backend 的 ownership 分层。
- 占位建议：`assets/sources/ch05-mount-view-visibility.drawio` / `assets/figures/ch05-mount-view-visibility.svg`，表达 bind / move / proc mounts view 只改变 mount topology，不复制 filesystem object。

**代码片段候选：**

- `FileOps` / VFS lookup ctx 的简短接口形状。
- procfs node callback 到真实 owner 的接口例子。
- `PathRef { mount, dentry }` 与 `FileOps` vtable 的短片段，用来支撑位置模型和 opened file object 边界。

**边界提示：**

- 设备模型细节放第 6 章。
- 不把 procfs / devfs / sysfs 写成只读观察面；它们也可以是 control surface。

### 6. 设备驱动模型与 I/O 对象

**核心论点：** Anemone 的设备模型独立于 VFS；devfs 只是把设备发布到
namespace 的桥，设备身份、I/O 语义和私有控制面由设备 owner 维护。

**覆盖范围：**

- device / driver / bus matching 与 probe。
- driver-private state 与 `Opaque` / `AnyOpaque` 的类型擦除边界。
- driver probe 之后的 I/O class publication。
- char device / block device 的共同边界。
- devfs bridge。
- `FileOps` 与设备 owner 的关系。
- `ioctl` 分发。
- loop、random、tty、block devfs 作为设计例子。
- 未来 sysfs 与设备 topology / control surface 的关系。

**代表路径 / case study：**

- discovery / driver registration -> bus match -> driver probe -> concrete device owner。
- devfs publish -> open -> FileOps read/write/ioctl -> device owner。
- loop / block ioctl 如何路由到 block / loop owner，而不污染 VFS。

**图候选：**

- `bus owns matching and probe; driver creates the I/O owner`。
- `devfs 暴露设备节点，但设备语义仍由 driver owner 决定`。
- `FileOps 是 VFS 与设备 owner 之间的窄接口`。
- `ioctl 控制面穿过 VFS，但最终由设备 owner 解释`。
- 占位建议：`assets/sources/ch06/device-driver-bus.drawio` / `assets/figures/ch06/device-driver-bus.png`，表达 discovery source、`Device`、`BusType` devices/drivers collection、`Driver::probe()`、concrete owner 和可选 char / block registry 的关系。
- 占位建议：`assets/sources/ch06-devfs-device-bridge.drawio` / `assets/figures/ch06-devfs-device-bridge.svg`，表达 registry、publish record、devfs inode、VFS open 和 device owner 的桥接关系。
- 占位建议：`assets/sources/ch06-ioctl-owner-boundary.drawio` / `assets/figures/ch06-ioctl-owner-boundary.svg`，表达 `sys_ioctl`、`IoctlCtx`、`FileOps::ioctl`、`CharIoctlCtx` / `BlockIoctlCtx` 和 concrete device state 的解释权流向。

**代码片段候选：**

- `Device` / `DriverOps` / `BusType` 的短接口形状。
- `AnyOpaque` / `Opaque` 作为 driver-private state 的 design note。
- char / block device trait 或 hook 的短接口形状。
- device publish API 的短例子。
- `CharDev` / `BlockDev` trait、`IoctlCtx` / `CharIoctlCtx` / `BlockIoctlCtx` 的短片段，用来支撑 device owner 与 VFS ioctl 边界。

**边界提示：**

- 不把设备驱动模型写成 VFS 附属品。
- 不把 `/dev/null`、loop、random、tty 写成功能清单。

### 7. 内存管理与 memory object

**核心论点：** Anemone 在 Linux-visible `mmap` / `shm` / `brk` 语义和
Anemone-native backing object 边界之间走折中路线，用 address space、mapping、
page cache 和 fault owner 组织内存，而不是复刻 Linux VM 或完整采用 Zircon VMO。

**覆盖范围：**

- address space / VMA / page table。
- physical page allocation。
- page fault path。
- anonymous / file-backed mapping。
- page cache。
- SysV shm 作为 shared backing / permission / lifecycle 例子。
- memory object / backing object 思路与 Zircon VMO 的关系。
- truncate / mmap coherency、shared writable mmap 等作为边界自然提及。

**代表路径 / case study：**

- file-backed mmap fault。
- SysV shm attach -> fault -> detach。

**图候选：**

- `Page Fault 连接 trap handling、address space 和物理页分配`。建议文件名：`ch07-page-fault-owner-boundary.drawio` / `ch07-page-fault-owner-boundary.svg`。节点：arch trap、page fault handler、`Task` / `UserSpaceHandle`、`UserSpace` lock、`VmArea`、`VmObject::resolve_frame`、frame allocator、page table / TLB shootdown。箭头：fault info -> address-space lookup -> object page index -> frame resolution -> PTE install。技术结论：fault path 连接多个 subsystem，但 VMA/backing owner 决定页面来源。
- `mapping 通过 backing object 连接匿名页、文件页和共享内存`。建议文件名：`ch07-backing-object-map.drawio` / `ch07-backing-object-map.svg`。节点：Linux-visible `mmap` / `brk` / `shmat`、`AnonymousMapping`、`FileMapping`、`ObjectMapping`、`AnonObject`、`ShadowObject`、inode mapping、`ShmObject`。箭头：syscall adapter -> typed mapping -> `VmArea` -> backing object。技术结论：Anemone 使用 backing object 边界吸收不同来源，不承诺完整 Zircon-style VMO。
- `page cache 是文件 backing 与地址空间 fault 之间的共享层`。建议文件名：`ch07-file-backed-page-cache.drawio` / `ch07-file-backed-page-cache.svg`。节点：inode、ext4 / ramfs regular state、resident pages、shared/private mapping、`msync`、truncate。箭头：file read/write 与 mmap fault 共同访问 mapping；private mapping 通过 shadow object 分叉；truncate 只裁剪 resident cache，不主动失效 live user PTE。技术结论：page cache 已是共享层，但 truncate/mmap 强一致性仍是 accepted limitation。

**代码片段候选：**

- VMA / backing object / fault handler 的短接口形状。
- page cache lookup / fill 的抽象边界。

**边界提示：**

- 不暗示已经完整实现 Zircon-style VMO。
- 不把 Linux VM 内部结构当作目标。
- 自然提及 stage-1 边界：file-backed fault 的洞页/EOF 错误域、truncate 与 live mmap coherency、SysV shm `munmap` detach、`SHM_LOCK` residency、mremap backing-aware grow / move、memory 组依赖的 procfs / rlimit / `/dev/zero` 可观察设施。
- 核对入口：`anemone-book/meta/sources.md` 第 7 章条目、`docs/src/register/current-limitations.md`、`docs/src/register/open-issues.md`、`docs/src/devlog/changes/2026-06-14-sysv-shm-cred-permissions.md`、`docs/src/rfcs/{oom-killer,inode-shrinker}/`、`anemone-kernel/src/mm/uspace/`、`anemone-kernel/src/fs/{ext4,ramfs}/file.rs`、`anemone-kernel/src/mm/uspace/shm/`。

### 8. 体系结构、Trap 与平台边界

**核心论点：** Anemone 把用户态、内核态和硬件异常之间的 unsafe boundary
收束在 arch / trap 层；多架构支持的核心不是复制代码，而是在共同内核对象
和架构特定上下文之间建立稳定边界。

**覆盖范围：**

- riscv64 / loongarch64 平台边界。
- 从 arch bootstrap 到 `bsp_kinit` / `ap_kinit` 的启动路径。
- trap entry / trap return。
- syscall dispatch 与 arch handoff。
- interrupt / exception。
- FPU / context。
- user/kernel boundary。
- unsafe assembly 与 Rust ABI。
- HAL ownership inversion：功能模块定义 arch trait，arch 实现并 re-export。
- arch-specific context 与 generic task / scheduler / MM / time / signal 的接口。
- machine abstraction：介于硬编码平台和完整设备树化之间的折中。

**代表路径 / case study：**

- `__nun` -> `rusty_nun` -> stage-1 bootstrap -> `Task::new_kernel*()` 创建 `kinit` -> scheduler。
- trap entry -> syscall / exception dispatch -> return。
- FPU lazy context / trapframe alignment 作为 unsafe boundary 工程例子。
- machine abstraction 如何承载平台初始化、设备发现和 arch glue。

**图候选：**

- `Bootstrap 把硬件初态收束成 generic kernel task`。
- `Trap entry 把硬件异常转换为 generic kernel path`。
- `Trapframe 是 Rust 内核和手写汇编之间的 ABI 合同`。
- `HAL trait 由使用它的功能模块定义，arch 只实现硬件事实`。
- `机器抽象层在硬编码平台和完全设备树化之间取得复杂度折中`。

**代码片段候选：**

- `arch/mod.rs` target selection / re-export 的短接口形状。
- `TrapArchTrait`、`SchedArchTrait`、`PagingArchTrait` 或 `TimeArchTrait` 展示 HAL owner 分散在功能模块的短接口形状。
- `MachineDesc` 的短接口形状。
- trapframe layout assertion 或 offset guard 的短例子。

**边界提示：**

- bootstrap 路径只讲到 normal scheduler / `kinit` handoff，不把 §8 写成启动源码逐行导览。
- `__nun` 命名可用脚注解释文化背景，但不能让命名故事抢过技术主线。
- HAL ownership inversion 的好处是接口更贴使用点、owner 更清晰；代价是内核硬件边界无法完全干净分离，硬件相关优化可能跨越多个功能模块。
- 说明 Linux ARM 早期 machine description 是启发来源，不是长期工业终局。
- 未来更完整 DTB / sysfs / device topology 可在 `TradeOff: ...` 收束段中讨论，不单独使用 `Beyond Anemone` 栏目。
- rv64 FPU / trap-return unsafe boundary 当前是“fix landed; revalidation pending”，不能写成所有同类 SIGILL 回归已经关闭。
- hard IRQ / IRQ-off return-tail allocation audit 仍是开放问题；本章只能表达目标边界和风险，不宣称现状已经完全 allocation-free。

**source pass 核对入口：**

- `anemone-kernel/src/arch/{riscv64,loongarch64}/bootstrap.rs`、`anemone-kernel/src/main.rs`、`anemone-kernel/src/task/kthread/`、`anemone-kernel/src/initcall.rs`。
- `anemone-kernel/src/arch/mod.rs`、`anemone-kernel/src/exception/trap/hal.rs`、`anemone-kernel/src/sched/hal.rs`、`anemone-kernel/src/mm/paging/hal.rs`、`anemone-kernel/src/time/hal.rs`、`anemone-kernel/src/task/sig/hal.rs`、`anemone-kernel/src/syscall/{mod.rs,handler.rs}`。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/{mod.rs,utrap.rs}`。
- `anemone-kernel/src/arch/{riscv64,loongarch64}/fpu.rs`、`anemone-kernel/src/arch/{riscv64,loongarch64}/machine/`、`anemone-kernel/src/exception/intr/irq.rs`。
- `docs/src/register/open-issues.md` 中 `ANE-20260608-RISCV-FPU-TRAP-RETURN-UNSAFE-BOUNDARY` 与 IRQ-off allocation audit 条目。

### 9. 结语：Anemone 的下一段路

**核心论点：** Anemone 当前最重要的成果不是某个单点功能，而是一套能继续
演进的内核结构与工程方法；未来工作应沿着已建立的 owner boundary、ABI
边界和工程纪律继续收敛。

**覆盖范围：**

- 回顾全书主线。
- 点出当前阶段边界。
- 展望决赛和后续路线。
- 少量行业参照或未来设计空间，使用具体的 `TradeOff: ...` 标题承载。

**代表路径 / case study：** 无。作为全书收束。

**图候选：** 可选，一张轻量路线图；如果标题不能形成明确结论则不放。

**代码片段候选：** 无。

**边界提示：**

- 不写成 backlog。
- 不承诺尚未接受的设计。

## 附录

### 附录 A：术语表

**核心论点：** 统一 ABI、owner boundary、FileOps、wait-core、VMO、page
cache 等术语，降低跨章节阅读成本。

**候选术语：** ABI、native contract、owner boundary、single source of truth、
FileDesc、File、Inode、Mount、FileOps、wait-core、Latch、Task、ThreadGroup、
VMA、page cache、backing object、VMO、trapframe、machine abstraction。

### 附录 B：参考资料与致谢

**核心论点：** Anemone 的设计参考了 Linux、Zircon / Fuchsia、Rust、OS 经典
材料和开源社区，但不是任何单一系统的复刻。

**内容边界：**

- 正式 bibliography 由 `refs.bib` 生成。
- 致谢可解释设计来源，但不替代技术论证。

### 附录 C：Agentic Coding 与工程工作流

**核心论点：** Agentic coding 和 RFC 工作流是受约束的工程机制，不是设计责任
主体；所有设计事实仍回到 canonical docs、代码和验证。

**覆盖范围：**

- agentic coding / writing 的使用阶段和范围。
- RFC / implementation gate / transaction devlog。
- register / current limitations。
- 人工责任。
- agent 的限制。
- prompt / workflow / write set / review / validation 约束。
- 如何避免幻觉、越界和临时 hack 固化。

### 附录 D：版本说明

**核心论点：** 说明本 PDF 对应的 Anemone 版本、提交点、冻结时间和后续
决赛版维护方式。

**内容边界：**

- 记录版本快照，不写进度日志。
- 冻结后只修事实错误、错别字、引用错误和排版问题。

## 模块覆盖矩阵

| 模块 / 能力 | 主要落点 | 写法 | 备注 |
| --- | --- | --- | --- |
| syscall / ABI boundary | 第 2 章 | 正文重点 | Linux-visible surface 与 internal contract 分层。 |
| syscall link-section registry / proc macro | 第 2 章 | 正文亮点 | 展示 Anemone 独有的分发和元数据组织优势。 |
| task / process / thread group | 第 3 章 | 正文重点 | 身份、生命周期、执行上下文。 |
| credentials | 第 3 章 | 自然提及 | 权限身份和 capability check，可与 syscall / IPC 交叉。 |
| scheduler | 第 4 章 | 正文重点 | runnable state 和 CPU selection owner。 |
| wait-core / synchronization / timeout | 第 4 章 | 正文重点 | 阻塞协议和 wake invariant。 |
| signal | 第 3 / 4 章 | 正文重点 | topology 在第 3 章，wait / trap interaction 在第 4 章。 |
| timer / clock | 第 4 章 | 自然提及 | 不单独大章；作为 timeout / notification 例子。 |
| VFS / FileDesc / File / Inode / Dentry | 第 5 章 | 正文重点 | 三层对象模型和 opened file description 边界。 |
| path lookup / mount tree / namespace | 第 5 章 | 正文重点 | mount view 和 namespace 语义。 |
| procfs / sysctl | 第 5 章 | 正文重点 | namespace bridge / control surface。 |
| device model / driver publish | 第 6 章 | 正文重点 | 独立于 VFS 的设备 owner。 |
| devfs / future sysfs | 第 5 / 6 章 | 正文重点 | 通用 pseudo fs 原则在第 5 章，devfs 具体桥接在第 6 章。 |
| char device / block device / ioctl | 第 6 章 | 正文重点 | 设备 I/O 语义和私有控制面。 |
| loop / random / tty | 第 6 章 | 例子 | 只在支撑 device owner / ioctl 分发时出现。 |
| memory management / address space / page table | 第 7 章 | 正文重点 | address space、mapping 和 fault path。 |
| page allocator / page cache | 第 7 章 | 正文重点 | allocation 与 file-backed mapping 的连接点。 |
| mmap / file-backed mapping | 第 7 章 | 正文重点 | 代表路径候选。 |
| shared memory / SysV shm | 第 7 章 | 正文重点 | shared backing、permission、lifecycle。 |
| IPC primitives: pipe / eventfd / futex | 第 4 / 5 / 6 章 | 选择性提及 | 只有能支撑 wait-core、I/O object 或 ABI 分层时才写。 |
| arch / trap / interrupt / syscall handoff | 第 8 章 | 正文重点 | user/kernel boundary。 |
| FPU / context / unsafe boundary | 第 8 章 | 正文重点 | unsafe boundary 的代表例子。 |
| machine abstraction / platform init | 第 8 章 | 正文重点 | 复杂度折中的平台抽象。 |
| verification / accepted limitations | 各相关正文章 | 自然提及 | 只在支撑设计边界、trade-off 或版本边界时出现，不写测例分数。 |
| RFC / devlog / register / workflow | 附录 C | 附录 | 工程工作流集中放附录，不抢内核设计主线。 |
| agentic coding governance | 附录 C | 附录 | 讲 agent 使用披露、约束和人工责任。 |
| glossary / terminology | 附录 A | 附录 | 降低跨章节阅读成本。 |
| references / acknowledgements | 前言 / 附录 B | 自然提及 | 前言有致谢，附录 B 归档正式参考资料。 |
| version snapshot | 附录 D | 附录 | 版本说明，不做进度账本。 |

## 开放写作问题

- 每章 epigraph 的候选和来源需要单独核对。
- 第 2 章代表路径最终是否以 `openat + ioctl` 为主，还是加入一个 `clone3` 小例子。
- 第 4 章代表路径在 sleep / wakeup、`ppoll` / `pselect6` 和 signal interruption
  之间如何取舍。
- 第 5 章代表路径选择 path lookup + mount view，还是 `/proc/<pid>/fd`。
- 第 7 章代表路径选择 file-backed mmap fault，还是 SysV shm attach / detach。
- 哪些图需要先画 source，哪些可以在章节中内联。
