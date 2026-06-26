#import "../components/figure.typ": code-block, report-figure

= 概述

Anemone是一个多核、支持多架构的类Unix操作系统内核，目标是*在不引入大量历史包袱和污染内部实现的情况下，尽可能兼容Linux UAPI，从而支持现有的用户态应用程序和工具链*。Anemone具备强大的灵活性和扩展性——我们尽可能避免内核内部的硬编码，而是将各种常量、配置项转移到了可配置的Kconfig中，并提供了丰富的内核参数（例如内核抢占，各种文件系统驱动等）和调试接口，方便用户和开发者进行定制化和调试，同时也有利于我们轻松移植到新的架构或平台。目前，Anemone已经支持RISC-V64和LoongArch64两种架构，并适配了对应的QEMU虚拟平台。

== Anemone 整体架构

Anemone分成三层：最底层是架构与平台接入层，负责内核自举、trap/中断、上下文保存、时钟中断、机器适配、平台设备发现等；接着是基础设施层，这里包括内核的各个核心子系统：进程管理、调度、时间、内存管理、文件按系统、IPC、设备驱动模型等；最后是面向用户的系统调用层，在这里，我们兼容Linux的UAPI，并将其映射到Anemone自己的内部对象，避免Linux的语义侵入我们的内核，从而造成污染。

#report-figure(
  image("../assets/anemone-architecture.png", width: 100%),
  caption: [Anemone 整体架构图。],
)

== 项目文件结构

目前，整个项目的顶层目录结构的重要部分如下所示。

#code-block(
  ```text
  .
  ├── Justfile                    # 构建、格式化、运行入口
  ├── kconfig                     # 内核配置文件
  ├── anemone-book                # 高层设计文档
  ├── anemone-kernel              # 内核主体
  ├── anemone-abi                 # 内核与用户态共享 ABI
  ├── anemone-rs                  # Rust 用户态支持库
  ├── anemone-libc                # 用户态 libc
  ├── anemone-apps                # 用户态应用
  │   ├── init
  │   └── user-test
  ├── conf                        # 架构、平台和 rootfs 配置
  │   ├── arch
  │   ├── platforms
  │   └── rootfs
  ├── symtab                      # 符号表辅助工具
  ├── scripts                     # 构建、运行和 QEMU 脚本
  ├── docs                        # RFC、devlog、register
  └── report                      # 比赛的开发报告
  ```.text,
  caption: [Anemone 顶层目录],
)

内核主体按子系统拆分如下。

#code-block(
  ```text
  arch       # RISC-V64 / LoongArch64 架构入口
  exception  # trap、异常和中断入口
  syscall    # Linux syscall 分发与 ABI 解析
  task       # task、线程组、进程拓扑、信号和资源
  sched      # 调度器、等待和运行队列
  time       # 时钟、timer、itimer 和时间 API
  mm         # 地址空间、页表、物理页和缺页路径
  fs         # VFS、mount、procfs、devfs 和文件系统后端
  device     # 设备模型、设备发现和 I/O class
  driver     # 块设备、串口、中断控制器、virtio 等驱动
  sync       # 内核同步原语
  crates     # 独立 crate
  ├── buddy-system
  ├── device-tree
  └── la-insc
  ```.text,
  caption: [Anemone 内核主体目录],
)

== 分工与贡献

本节由队伍在最终提交前补齐。建议按“成员 -> 负责模块 -> 代表性工作 -> 可答辩代码入口 -> 相关验证证据”的粒度填写，避免只列职责名。
