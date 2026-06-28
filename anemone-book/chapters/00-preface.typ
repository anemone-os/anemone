#import "../template/components.typ": *

= 前言

#epigraph(attribution: [Harold Abelson and Gerald Jay Sussman, @abelson1996sicp])[
  Programs must be written for people to read, and only incidentally for machines to execute.
]

#thesis[
  Anemone 的价值不在于把 Linux 源码目录翻译成 Rust，也不在于把一个个 syscall 修到测例通过。它更值得被记录的地方，是一个面向 Linux ABI compatibility 的教学/竞赛内核，如何在 Rust 类型约束、native object model、owner boundary 和持续验证之间，形成一条可以审查、可以继续演进的设计路线。
]

Anemone 最初面对的是很具体的外部世界：Linux ABI、现有用户态、测试程序、文件系统镜像、两套架构、有限时间和不断变化的工程现实。这些约束很容易把一个内核项目推向最短路径：哪里失败修哪里，哪个 flag 不认识就补哪个 flag，哪个测试卡住就临时绕过去。这样的路线可以堆出功能，却很难留下一个后来者还能理解和维护的系统。

本书选择从另一个角度讲 Anemone。它承认 Linux ABI compatibility 是必须认真面对的外部 surface，但不把内部结构写成 Linux 的缩小版。它承认 Rust 能帮助压缩生命周期和状态所有权错误，但不把 Rust 当成自动正确性的护身符。它也承认测试和真实负载是重要反馈，但不把测例矩阵、得分策略或一次次修补过程当成正文主线。Anemone 更核心的问题是：当用户可见协议、内部对象、架构边界和阶段化实现互相拉扯时，哪些对象拥有状态，哪些接口只表达能力，哪些兼容桥必须带着退出条件，哪些限制必须如实留在 register / current limitations 里。

因此，这不是一本 syscall 手册，也不是源码逐文件导览。读者不会在这里找到完整 errno matrix、全部 LTP case 解释或每个模块的实现清单。正文只挑那些能说明设计判断的路径：syscall adapter 如何隔离 Linux-visible ABI 和 native internal contract；task topology 为什么拥有进程关系；wait-core 为什么不能让 task 成为第二套等待真相源；VFS、procfs、devfs 和 device owner 如何分工；memory object 如何把匿名页、file-backed mapping 和 SysV shm 放进同一条 fault 叙事；arch / trap 层如何把 unsafe assembly、trapframe 和 generic kernel context 接起来。

本书也不是事实源。它只把已经存在的设计事实组织成一条可读路径；设计事实仍可由源码、已接受设计文档、执行记录、开放问题和已接受限制核对。#footnote[本书不替代这些 canonical 文档。代码决定当前行为，RFC 决定 accepted contract，transaction devlog 记录执行事实和实现反馈，register / current limitations 保存开放问题和接受限制。]

#non-goal[
  本书不以测例分数组织叙事，也不把开发过程描述成比赛环境的逐项应对。验证可以作为工程反馈闭环出现，但它服务设计判断，而不是替代设计判断。
]

Anemone 的设计当然不是从真空里长出来的。Linux 提供了最重要的 ABI 参照和大量工程现实提醒；Zircon / Fuchsia 的 object、handle、VMO 和 capability 风格影响了我们理解内部对象边界的方式；经典 OS 文献反复提醒系统设计要把接口、模块化和概念完整性放在功能堆叠之前；Rust 社区和开源系统软件生态则让“可审查的 unsafe boundary”变成一个实际工程问题，而不是口号。Anemone 的主要开发工作由 `doruche` 推进，`EDGW_` 也参与了系统建设；本书保留这两个名字，是为了让这份设计叙述仍然能指向真实的人和真实的工程判断。前言的这段致谢并不意味着 Anemone 复刻任何一个系统；它只是说明，本书讨论的是一个在多种约束和传统之间做取舍的内核，而不是孤立的练习题。

后续章节会从系统地图开始，依次展开 ABI 边界、执行实体、调度与等待、VFS、设备模型、内存、架构边界和工程反馈闭环。这个顺序不是实现顺序，而是读者理解系统的路径。如果某一章提到限制，它不是为了降低标准，而是为了把当前版本能承诺什么、不能承诺什么说清楚；如果某一章提到验证，它也不是为了展示过程，而是为了说明某个设计边界经受过怎样的反馈。

最后，读这本书时可以把 Anemone 当成一个仍在成长的系统，而不是一个已经封箱的产品。它的许多接口还会变，许多 accepted limitations 还会被关闭，许多章节在决赛版也应继续演进。但本书试图固定一个更重要的东西：当系统继续变化时，哪些设计问题不该被临时修补淹没，哪些 owner boundary 不该被便利路径偷走，哪些事实必须回到可以核对的地方。
