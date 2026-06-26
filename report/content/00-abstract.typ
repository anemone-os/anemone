#import "../components/figure.typ": report-figure

= 摘 要

Anemone 是一个使用 Rust 实现、支持 RISC-V64 与 LoongArch64 平台的操作系统内核。

在开发过程中，我们始终避免为了对特定测例进行特化适配而妥协系统设计。Anemone 的目标是：在 Linux ABI 兼容性、进程线程管理、虚拟内存、VFS 与文件系统、设备驱动模型、IPC、同步和体系结构适配等核心能力域上形成*可解释、可维护、可持续演进*的系统实现。

Anemone 的开发全程独立自主。在架构上，我们没有参考任何往届作品，而是抛弃了历史包袱，完全从零设计。我们还编写了新的 buddy 内存分配器、设备树解析库、LoongArch64 硬件支持库等，它们都独立于且被抽离了内核，形成单独的 crate，经过了 Miri、cargo-fuzz 等工具的验证，我们期望这些 crate 能为未来的开源社区贡献更多的基础设施。

截至本文档编写时，Anemone 已经通过初赛测例的大部分测例，并通过了大量 LTP 测例点。

#report-figure(
  image("../assets/rank.png", width: 90%),
  caption: [Anemone 当前榜单截图。],
)

Anemone 各个模块完成情况概览如下。

#figure(
  table(
    columns: (3.4cm, 10.8cm),
    align: (center, center),
    inset: 7pt,
    stroke: 0.8pt,
    [*模块*], [*完成情况*],
    [进程管理], [实现 task / thread group / process group 等执行实体管理，覆盖 fork / clone / exec / exit / wait 等生命周期路径。],
    [调度与时间], [围绕 scheduler、wait-core、signal interruption 和时间触发形成阻塞、唤醒与超时处理路径。],
    [内存管理], [实现地址空间、页表、缺页处理、匿名页、VMO / backing object、file-backed mapping、共享内存与内存压力相关路径。],
    [IPC], [覆盖 signal、pipe、System V IPC、event/timer 类文件对象、poll/select 等等待组合路径。],
    [文件系统], [实现 VFS、路径查找、mount view、opened file object、procfs、devfs 和多类文件后端的统一接入。],
    [设备驱动模型], [实现设备发布、字符/块设备、devfs bridge、ioctl 分发和若干具体设备对象。],
    [架构硬件抽象层], [支持 RISC-V64 与 LoongArch64 的启动、trap、中断、上下文保存和平台差异收束。],
    [ABI 兼容设计], [围绕 Linux syscall、UAPI 参数、flag、errno 和代表路径解释用户可见兼容语义。],
  ),
  caption: [Anemone 模块完成情况概览],
  kind: table,
)
