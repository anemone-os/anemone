= 术语表

本附录收束本书反复使用的核心术语。它不是 Anemone 的 ABI 规范，也不替代源码、RFC、devlog、register 或 current limitations；它只说明这些词在本书叙事里的默认含义。

== 设计边界

- ABI：application binary interface。本书主要指 Linux-visible syscall number、UAPI layout、flag、errno、signal frame、`ioctl` command 和用户态可观察行为。
- Linux-visible surface：用户态程序直接观察到的兼容面。它必须认真对齐，但不等于 Anemone 的内部对象模型。
- Anemone-native UAPI：Anemone 自己暴露的受控用户态入口，例如调试或关机控制能力。它不伪装成 Linux ABI，也不绕过内部 owner boundary。
- Native internal contract：内核内部 owner 之间的协议。它不是对用户态的 ABI 承诺，而是代码、RFC 和 review 可以核对的内部设计约束。
- Native object model：Anemone 内部对象和 ownership 组织方式，不等同于 Linux 源码目录结构。
- Owner boundary：状态转换权的边界。一个对象或子系统如果拥有某类状态，其他路径应通过窄接口、handle、token、ctx 或 snapshot 与它交互。
- Single source of truth：某个行为状态只有一个真相源。缓存、诊断字段或 snapshot 可以存在，但不能反向驱动状态机。
- Stage-aware compatibility：阶段化兼容策略。当前版本可以接受受限语义或临时桥，但边界、可见行为、日志和退出条件必须清楚。
- Accepted limitation：当前阶段明确接受的能力缺口，记录在 current limitations 中。它不是未知异常，也不是正文可以重新定义的未来承诺。

== 执行与等待

- Task：Anemone 中可被调度的执行实体。它提供身份、执行上下文和关联对象入口，但不应成为 scheduler、wait-core 或 topology 的第二套真相源。
- ThreadGroup：共享用户态资源和 Linux-visible TGID 关系的一组 task。它属于 task topology 叙事，而不是 scheduler 的内部状态。
- ProcessGroup / Session：Linux-visible process topology 的成员关系和控制边界。当前 job-control 能力仍按 register / current limitations 的 stage-1 边界理解。
- Task topology：维护 TID / TGID / PGID / SID membership、publish / unpublish 事务和 topology consistency 的中心 owner。
- Scheduler：拥有 runnable state、run queue 和 CPU placement 的子系统。它选择可运行对象，但不解释 syscall 意图或文件 I/O 语义。
- Wait-core：拥有 blocking protocol、wait identity 和 wake completion 分类的子系统。event source 发布 wake capability，但不直接拥有 task 的等待状态。
- Latch：用于 poll / select / pselect / ppoll 这类 OR wait 的一轮等待聚合。它把多个 readiness source 收束到一次 wait-core 协议，而不是让每个 source 自己调度 task。
- Wake token：完成一次等待或唤醒协议的能力对象。它表达 capability，不等同于 task owner。
- Snapshot：一次调用、一次观察或一次格式化输出中取得的状态副本。snapshot 可以用于诊断和稳定输出，不应反向驱动长期状态机。

== VFS、设备与 I/O

- FileDesc：fd table 中的文件描述符槽位及 fd-local flags。它指向 opened file description，但不等同于 `File` 本身。
- File：Anemone 的 opened file description，承载 file status flags、cursor 和 `FileOps` 分发边界。
- Inode：filesystem backend 暴露给 VFS 的对象身份和文件语义入口。它不是路径，也不是 fd。
- Dentry：目录项名字和 inode 的连接点。
- Mount：一次 filesystem tree 接入 namespace 的挂载实例，携带 mount-local 属性和 topology 关系。
- PathRef：`Mount + Dentry` 的位置对象，类似 Linux `struct path` 的角色。它表达“从哪个 mount view 看见哪个 dentry”，不是临时兼容结构。
- FileOps：opened object 的窄操作接口。VFS 通过它分发 read / write / seek / ioctl 等操作，但不应替具体 filesystem 或 device owner 解释私有状态。
- Pseudo filesystem：把内核对象接入 namespace 的桥，例如 procfs、devfs 和未来 sysfs。它可以是观察面或 control surface，但不拥有被暴露对象的核心状态。
- Devfs：设备模型到 VFS namespace 的桥。它发布设备节点和 inode，但设备身份、I/O 语义和私有控制面仍由 device owner 维护。
- CharDev / BlockDev：字符设备和块设备的 owner-facing 抽象。它们通过 devfs / `FileOps` 暴露给用户态，但不从属于 VFS。
- Ioctl ownership：`sys_ioctl` 和 VFS 只负责把控制请求送到合适 opened object；具体 command 的解释权属于对应 owner。

== Memory 与平台边界

- Address space：用户态虚拟地址空间及其 VMA 集合。
- VMA：虚拟内存区域，描述地址区间、权限和 backing 关系。
- Backing object：page fault 时提供页面来源的对象边界。匿名页、file-backed mapping、shadow object 和 SysV shm 都通过它进入 fault 叙事。
- Page cache：文件内容与 file-backed mapping 之间的共享驻留页层。当前 truncate / mmap 强一致性仍以 current limitations 为准。
- VMO：virtual memory object。本书用它描述 Zircon / Fuchsia 启发下的 memory object 思路；Anemone 不宣称完整实现 Zircon-style VMO。
- SysV shm：Linux-visible System V shared memory 能力。在本书中主要作为 shared backing、permission 和 lifecycle 的 memory object 例子。
- Trapframe：arch trap entry 保存的用户态寄存器和返回上下文。它跨越 unsafe assembly、Rust ABI 和 generic trap handling 边界。
- HAL ownership inversion：硬件抽象 trait 由使用该能力的 generic 模块定义，arch 模块实现并 re-export。这样接口贴近使用点，但硬件相关优化可能跨多个功能模块。
- Machine abstraction：启动期 machine descriptor、DTB / platform discovery、root IRQ/timer 初始化和多架构 early setup 的边界。它服务 boot 和 platform handoff，不替代 generic kernel object owner。
