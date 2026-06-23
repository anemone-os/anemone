#import "../template/components.typ": *

= 设计理念与系统地图

#thesis[
  这只是 `§1` 的 seed。它先提供全书地图：Anemone 的外部约束是 Linux ABI compatibility surface，内部组织原则是 native object model、owner boundary 和 Rust as a Design Constraint。最终版要等模块章节成型后再压缩和重写。
]

Anemone 的基本路线是折中，而不是纯粹复刻。Linux ABI 提供用户可见的兼容边界；Zircon / Fuchsia 这类系统提供了 memory object 等设计空间的启发；Rust 则把许多状态所有权和生命周期问题提前暴露在类型与模块边界上。Anemone 的设计价值，正是在这些约束之间选择一条足够直接、足够可维护的路线。

== 三层 Surface

可以先把系统理解成三层 surface：

- Linux-visible ABI surface：syscall number、UAPI struct、flag、errno 和用户态可观察行为。
- Anemone-native object model：`Task`、`File`、`Inode`、address space、device owner、wait object 等内部对象。
- Engineering feedback surface：testsuits、真实负载、review、devlog 和 current limitations 组成的反馈闭环。

#boundary[
  Linux ABI compatibility 不意味着内部结构要照搬 Linux。ABI adapter 应尽量把 Linux-specific parsing、flag compatibility 和 errno 选择限制在边界层，内部对象仍按 Anemone 自己的 owner boundary 组织。
]

== Rust 作为设计约束

Rust 在这里不是“自动安全”的广告词。它更像一个设计约束：共享状态必须有 owner，生命周期必须穿过类型边界，临时 snapshot 和长期状态不能随便混用。好的 Rust 内核设计不是把 C 结构体翻译成 `struct`，而是借助类型系统阻止错误的依赖方向自然生长。

#rationale[
  本书会反复使用 owner、handle、ctx、snapshot 这些词。它们不是装饰性术语，而是为了区分谁拥有状态、谁只能观察状态、谁只是一次调用中的上下文。
]

== 阅读地图

`§2` 讨论 ABI 边界与 syscall 层，`§3` 讨论 task/process 等执行实体，`§4` 把 scheduler、wait-core 和 time 单独展开。`§5` 与 `§6` 分别讨论 VFS 和设备模型：devfs 是桥，不是设备模型本身。`§7` 讨论内存对象与 address space 的折中路线，`§8` 讨论 trap、architecture 和 platform boundary，`§9` 再回到工程反馈闭环。

这个 seed 版本有意保持短。等后文模块章完成 source pass 和 draft pass 后，`§1` 需要回来吸收真实的代表路径、图和术语，而不是让后文迎合这里的早期总论。
