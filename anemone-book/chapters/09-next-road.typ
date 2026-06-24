#import "../template/components.typ": *

= 结语：Anemone 的下一段路

#epigraph(attribution: [Butler Lampson, @lampson1983hints])[
  There probably isn't a best way to build the system, or even any major part of it; much more important is to avoid choosing a terrible way, and to have clear division of responsibilities among the parts.
]

#thesis[
  Anemone 到当前阶段真正形成的成果，不是某个孤立 syscall、某个单点驱动或某组测例结果，而是一套还能继续演进的内核结构：Linux-visible ABI 被压在边界层，内部对象按 owner boundary 组织，unsafe 和硬件差异有可审查的局部入口，工程事实回到 RFC、devlog、register 和验证记录。下一段路的关键不是把未来功能列成清单，而是在这些边界内继续收敛。
]

一本设计叙述不应该替内核宣布终点。前面几章展示的也不是“Anemone 已经完成了一个 Linux kernel 的 Rust 版本”，而是另一个更具体的判断：当一个教学/竞赛内核必须面对真实 Linux 用户态、真实测试负载和多架构硬件边界时，它仍然可以拒绝把所有兼容性压力都塞进一团临时 glue。`openat`、`ioctl`、`clone3`、`ppoll`、`mmap`、`trap return` 这些入口看起来分散，但它们都在问同一个问题：用户可见协议应该在哪里结束，内部对象的真相源应该从哪里开始。

== 结构比功能更难替换

功能可以补，结构一旦带偏就很难退回。Anemone 因此把许多设计成本提前支付在边界上：syscall adapter 只翻译 Linux ABI，不拥有 VFS、task、device 或 MM 的对象语义；task topology 拥有 Linux-visible 线程组、进程组和会话关系，scheduler 和 wait-core 不把这些关系缓存成自己的第二套状态；VFS 负责名字、路径、opened file description 和 inode bridge，设备模型仍拥有设备身份、I/O 语义和私有控制面；address space、VMA 和 backing object 把虚拟地址可见性与页面来源分开；arch / trap 层把手写汇编、trapframe layout、FPU context 和 machine descriptor 收束成 generic kernel 可以审查的接口。

这些选择没有消灭复杂性，只是给复杂性分配了归属。Linux ABI 的细节仍然会出现，甚至会以很不体面的形式出现：某些 flag 需要静默兼容，某些 errno 只能先按阶段边界收束，某些测试暴露的行为不是核心语义错误，而是 procfs、rlimit、工具链或测试设施缺口。真正重要的是，这些压力不能反向定义内部对象。一个兼容桥如果必须存在，就应该有日志、注释、退出条件和事实入口；一个 accepted limitation 如果还没有关闭，就应该留在 register / current limitations，而不是被书稿写成已经解决。

#boundary[
  结语不是 backlog。这里提到的方向只说明 Anemone 下一阶段应继续沿哪些边界推进；尚未进入 accepted RFC、transaction devlog 或 current limitations closeout 的内容，不在本书中变成实现承诺。
]

== 当前阶段的诚实边界

Anemone 当前已经建立了几个可以继续工作的骨架：typed syscall metadata 和 link-section registry 让 ABI 入口分散定义但统一注册；task topology、wait-core、latch、timer lane 和 runtime accounting 让执行实体、阻塞协议和时间触发不再互相偷状态；VFS、procfs、devfs、device registry 和 `FileOps` / device owner bridge 让 namespace、control surface 和真实 I/O owner 有了分工；memory object 路线让匿名页、file-backed mapping、SysV shm 和 page cache 可以在同一条 fault path 上解释；arch HAL ownership inversion 让多架构差异贴着真实使用点被定义。

这些骨架并不等于完整 Linux 语义。文件映射的 truncate coherency、shared writable mmap、部分 signal / wait corner case、POLLPRI / exception readiness、完整 job-control、设备 topology、sysfs 语义、IRQ-off tail audit、FPU / trap-return revalidation 等问题，都需要在自己的事实层继续收敛。把这些问题自然说成“后续完善”太轻了；更准确的说法是，它们分别属于 VFS/page-cache owner、wait/signal owner、device model owner、arch unsafe boundary owner 或工程验证 owner。只有归属清楚，未来实现才不会靠一层临时兼容把问题抹平。

这也是本书反复强调 artifact boundary 的原因。代码决定当前行为，RFC 决定 accepted contract，transaction devlog 记录执行事实和反馈，register / current limitations 保存开放问题和接受限制；The Anemone Book 只把这些事实组织成可读叙事。结语如果越过这些层次，反而会削弱前面几章建立的工程纪律。

== 下一段路

下一阶段最自然的路线，是继续把可见语义补到已有 owner boundary 上，而不是为了某个用例绕开边界。ABI 层应继续把 flag、errno、结构体和 silent compatibility 留在 syscall adapter 附近；task / signal / wait 路径应继续围绕 wait identity、topology owner 和 trap-return delivery 收束；VFS 和 device model 应继续保持 namespace bridge 与真实 I/O owner 的分离；MM 应继续让 page cache、file-backed mapping、SysV shm 和 memory pressure 路径回到 backing object 和 address-space contract；arch 层则需要把每个 unsafe ABI 前提、trapframe offset、IRQ context 和 FPU save/restore 路径做成能被局部审计的事实。

这条路线并不排斥更大的设计空间。更完整的 DTB 驱动发现、设备拓扑和 sysfs，可让设备模型有更稳定的可观察面；更强的 page-cache / mmap coherency，可让 file-backed mapping 更接近 Linux 用户态预期；更系统的 scheduler policy、resource accounting 和 OOM / shrinker 机制，可让内核在真实负载下更有弹性；更严格的 native UAPI，也可以让 Anemone 不只是被动兼容 Linux，而是在受控边界内表达自己的调试和管理能力。但这些方向都应先变成明确 contract，再变成代码，而不是先在实现里长出无法解释的公共接口。

#tradeoff[
  Anemone 的下一段路不是在“更像 Linux”和“更像某个现代微内核”之间二选一。Linux ABI compatibility 是用户态入口，Zircon / Fuchsia、Rust 和经典 OS 设计资料提供设计启发；Anemone 自己要守住的，是内部对象边界、事实层分工和可验证的阶段化承诺。
]

== 方法论的部分

Agentic coding 和 agentic writing 也应该放在这个框架里理解。它们不是 Anemone 的设计主体，也不是事实来源；它们更像一种放大器。放大器可以提高 source pass、草案生成、review checklist、局部实现和验证整理的速度，也会放大过度抽象、预铺接口、强行闭合和状态所有权漂移的风险。附录 C 已经详细讨论了这套 engineering harness；放在结语里，只需要留下一个方法论判断：速度只有在事实层、边界层和验证层都可审查时才有意义。

这也是为什么本书没有把测试分数、agent 对话或阶段性兴奋写成主线。真正能留下来的不是一次通过，而是下一位维护者能不能看懂为什么这里有一个 adapter、为什么那个字段只是 diagnostic-only、为什么某条 wait path 必须先发布能力再让出 CPU、为什么某个限制应该留在 register 而不是用一句“未来完善”带过。本章开篇的引语来自 Lampson 对系统设计的提醒：复杂系统未必有唯一最佳路线，但必须避免糟糕路线，并让各部分职责清楚。对内核来说，这里的“职责”包括未来 reviewer、debugger、porter 和不得不删除临时桥的人能否看清边界。

== 继续演进的内核

Anemone 的目标从来不是让一个小内核假装自己没有阶段边界。相反，它的价值在于承认阶段边界，同时让边界有名字、有 owner、有验证、有退出条件。一个系统如果能持续把兼容压力分类，把 unsafe boundary 局部化，把开放问题放回事实层，把实现反馈带回 contract，那么它就还有继续演进的空间。

所以，本书到这里不以“完成”收尾。更合适的结论是：Anemone 已经有了一套可以继续工作的方向感。它还会补功能、修语义、重写某些早期折中，也会在真实负载和审查中发现新的边界问题。但只要下一段路仍然围绕 owner boundary、ABI boundary、single source of truth 和可验证工程纪律推进，这些变化就不会只是功能堆叠，而会继续把 Anemone 推向一个更清晰、更能被维护的内核。
