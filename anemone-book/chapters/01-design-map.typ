#import "../template/components.typ": *
#import "../template/figures.typ": *

= 设计理念与系统地图

#epigraph(attribution: [Fred Brooks, @brooks1995mythical])[
  Conceptual integrity is the most important consideration in system design.
]

#thesis[
  Anemone 的设计主线不是“用 Rust 写一个小 Linux”，也不是把能跑的 syscall 一层层堆起来。它面对的外部世界是 Linux ABI compatibility surface 和少量 Anemone-native UAPI；它在内部真正维护的是 task、file、inode、device、address space、wait state、trapframe 等对象的 owner boundary。理解 Anemone，要先看清用户可见协议在哪里被翻译，内部对象在哪里保持单一真相源，以及工程事实如何回到 RFC、devlog、register 和验证证据。
]

这一章是读者地图，不是全书摘要。后续章节会分别展开 ABI、task、scheduler、wait-core、VFS、设备模型、memory object、arch / trap 和工程方法；这里先给出判断框架：Anemone 为什么不是 Linux 的 Rust 复刻，哪些 surface 面向用户态，哪些边界属于内部对象，Rust 在设计里施加了什么约束，以及读者应如何把各章连接起来。

#tradeoff[
  Anemone 的路线不是纯粹性竞赛。它在 Linux ABI 兼容、Rust 类型边界、Zircon / Fuchsia 等系统的对象思想和阶段化工程现实之间取舍。取舍本身不可避免；关键是每个取舍都要有 owner、事实入口和可验证的退出条件。
]

== 不是 Linux 的 Rust 复刻

兼容 Linux ABI 是 Anemone 的外部约束，不是内部结构目标。用户态程序通过 syscall number、UAPI struct、flag、errno、`ioctl` command、`mmap` prot/flags、signal frame 和 trap-return 行为观察系统；这些协议必须被认真对待，因为它们决定已有用户态能否运行。但 ABI compatibility 并不要求内核内部复刻 Linux 的源码目录、锁层次或对象形状。

Anemone 更关心的是 translation boundary。`openat` 的 flag parsing、`clone3` 的 feature gate、`pselect6` 的 fdset 行为、SysV shm 的 id/permission/lifecycle、riscv64 和 loongarch64 的 trapframe 差异，都应该先在边界附近被翻译成内部对象可以理解的 typed intent。越过边界后，VFS 不应该再看到 raw syscall register，device owner 不应该由 VFS 替它解释私有控制面，wait-core 不应该从 Linux 进程拓扑里推导阻塞协议，MM 也不应该把每个 Linux VM corner case 都写成 VMA 里的散乱标志。

#boundary[
  Linux 是 Anemone 的重要 ABI 和工程参照，但不是内部对象模型的模板。正文提到 Linux 时，默认指用户可见语义、测试负载或设计对照；如果讨论内部结构，必须明确说明它只是启发，而不是 Anemone 的实现目标。
]

这条边界也解释了为什么本书不按 POSIX 功能分类成章。signal 的目标选择属于 task topology，signal interruption 属于 wait completion，signal delivery 又会在 trap-return path 上显形；SysV shm 属于 memory object 和 address space；pipe、eventfd、timerfd 只有在能说明 wait-core、file object 或 time owner 时才值得出现。功能名字是用户态入口，章节主语是对象 owner。

== 三层 Surface

可以把 Anemone 先分成三层 surface。第一层是 Linux-visible ABI surface：syscall number、UAPI layout、flag、errno 和用户态可观察行为都在这里被接住。第二层是 Anemone-native UAPI surface：调试输出、关机控制或其他受控扩展入口可以存在，但它们不能伪装成 Linux ABI，也不能绕过内部对象边界。第三层是 native internal contract：`Task`、`FileDesc`、`File`、`Inode`、`Mount`、`CharDev`、`BlockDev`、`VmObject`、`WaitState`、`TrapFrame` 等对象之间的协议。

这三层不是三套状态。Linux ABI 和 native UAPI 最终都会落到同一组内部对象上；差别在于入口如何解析、错误如何映射、哪些兼容性噪声被保留在边界附近。Anemone-native internal contract 也不是对外承诺，它是内核各 owner 之间可以互相审查的工程协议。一个 syscall handler 如果需要打开文件，应该调用 VFS 的 typed API；一个 timer callback 如果要完成 `timerfd`，应该回到 `TimerFdState` owner；一个 page fault 如果要找页面来源，应该进入 backing object，而不是由 trap 顶层猜测文件系统语义。

#rationale[
  把 surface 分开，是为了避免两个相反错误：把 Linux ABI 兼容写成内部结构复刻，或者把内部 native contract 当成可以随 syscall 便利随意改动的私有细节。前者会让 Anemone 失去自己的对象边界，后者会让兼容层反向污染核心状态。
]

== Object Owner Map

Anemone 的主要子系统围绕对象 owner 组织，而不是围绕 Linux 源码目录组织。syscall 层拥有 ABI translation；task topology 拥有 TID/TGID/PGID/SID membership 和 publish / unpublish 事务；scheduler 拥有 runnable state 和 CPU placement；wait-core 拥有 blocking protocol 和 wait identity；VFS 拥有 pathname、mount view、opened file description 与 inode bridge；device model 拥有设备身份、probe、I/O 语义和私有控制面；MM 拥有 address space、VMA 和 backing object；arch / trap 层拥有硬件异常、trapframe ABI 和 unsafe assembly boundary。

#book-figure(
  "../assets/figures/ch01/system-owner-map.png",
  [Anemone 的主要子系统围绕对象 owner 组织；ABI surface 是入口，不是内部结构模板。],
  width: 100%,
)

这张图刻意不展开 §2 的 syscall adapter 细节，也不把所有模块都画成等价盒子。它表达的是方向：外部请求先经过 ABI / native UAPI 边界，再进入具体 owner；owner 之间通过窄接口协作，而不是互相缓存状态。`devfs` 是 VFS namespace 到 device owner 的桥，不把设备语义转移给 filesystem；file-backed mapping 让 VFS/page cache 与 MM fault path 相接，但页面来源仍由 backing object 解释；trap path 把硬件异常变成 generic kernel context，但 task 生命周期、地址空间 owner 和 signal policy 不下沉到 arch 层。

这种 owner map 不是静态组织架构图。它更像 review 时的第一轮问题表：这个字段是谁的真相源，是否只是 diagnostic snapshot；这个 helper 是否把 raw UAPI 泄漏进内部对象；这个 callback 是否在错误 context 做了对象析构；这个兼容桥有没有退出条件；这个 limitation 应该留在 register，还是已经有 accepted RFC 可以关闭。后续章节会把这些问题落到具体路径上。

== Rust as a Design Constraint

Rust 在这里不是“自动安全”的广告词。它更像一个设计约束：共享状态必须有 owner，生命周期必须穿过类型边界，临时 snapshot 和长期状态不能随便混用。好的 Rust 内核设计不是把 C 结构体翻译成 `struct`，而是借助类型系统阻止错误的依赖方向自然生长。

这并不意味着 Rust 会自动给内核带来正确性。裸汇编、trapframe layout、页表操作、用户指针、锁顺序、中断上下文和设备 DMA 仍然需要明确的 unsafe boundary。Rust 在 Anemone 中更实际的价值，是迫使 API 形状回答几个问题：调用者拿到的是 owner、handle、ctx、token 还是 snapshot；这个引用是否能跨越生命周期边界；这个 trait 是不是定义在真正的使用者附近；这个字段是协议状态还是诊断字段。

#design-note[
  本书反复使用 owner、handle、ctx、token 和 snapshot 这些词，不是为了制造术语密度。它们分别回答“谁拥有状态”“谁持有受限能力”“谁只在一次调用窗口内有效”“谁可以完成一次协议动作”“谁只是观察到的投影”。混用这些词，通常意味着状态所有权正在漂移。
]

因此，Rust as a Design Constraint 在第一章只定原则，不单独成章。真正的例子在后文：syscall macro 用类型和 validator 收束参数样板；task topology 不把成员关系缓存到每个 TCB；wait-core 用 `WaitState` 和 `WakeToken` 分离等待身份和唤醒能力；VFS 用 `FileOps` 和 `PathRef` 表达 opened object 与位置对象；device owner 通过 devfs / `FileOps` 暴露 I/O，但保持私有状态；MM 用 backing object 连接 anonymous、file-backed 和 SysV shm；arch HAL trait 由使用者定义，避免 `arch/` 目录反向拥有所有硬件抽象。

== 事实层与阅读路径

这本书并非 canonical source。代码决定当前行为，RFC 决定 accepted contract，transaction devlog 记录执行事实和实现反馈，register / current limitations 保存当前开放问题和接受限制。

这也影响阅读顺序。§2 讨论 ABI 边界与系统调用层，说明 Linux-visible surface 如何被翻译成内部 typed intent。§3 讨论 task、thread group、process group、session 和 credentials，建立执行实体与 topology owner。§4 把 scheduler、wait-core、latch、signal interruption 和 time trigger 放在一起，解释 runnable state 与 blocking protocol 为什么不能混成一套 task 状态。§5 讨论 VFS、mount view、procfs 和 pseudo filesystem，§6 专门展开独立的 device model 与 I/O owner。§7 讨论 address space、fault path、backing object、page cache 和 SysV shm。§8 收束 arch、trap、FPU、interrupt 和 machine abstraction 的 unsafe boundary。§9 则回到下一段路：结构比功能更难替换，未来演进必须沿这些边界继续收敛。

