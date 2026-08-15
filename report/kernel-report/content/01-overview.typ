#import "../components/figure.typ": code-block, report-figure

= 概述

Anemone是一个多核、支持多架构的类Unix操作系统内核，目标是*在不引入大量历史包袱和污染内部实现的情况下，尽可能兼容Linux UAPI，从而支持现有的用户态应用程序和工具链*。Anemone具备强大的灵活性和扩展性——我们尽可能避免内核内部的硬编码，而是将各种常量、配置项转移到了可配置的Kconfig中，并提供了丰富的内核参数（例如内核抢占，各种文件系统驱动等）和调试接口，方便用户和开发者进行定制化和调试，同时也有利于我们轻松移植到新的架构或平台。目前，Anemone已经支持RISC-V64和LoongArch64两种架构，并适配了对应的QEMU虚拟平台。

== Anemone 整体架构

Anemone分成三层：最底层是架构与平台接入层，负责内核自举、trap/中断、上下文保存、时钟中断、机器适配、平台设备发现等；接着是基础设施层，这里包括内核的各个核心子系统：进程管理、调度、时间、内存管理、文件系统、网络、IPC、设备驱动模型等；最后是面向用户的系统调用层，在这里，我们兼容Linux的UAPI，并将其映射到Anemone自己的内部对象，避免Linux的语义侵入我们的内核，从而造成污染。

#report-figure(
  image("../assets/anemone-architecture.png", width: 100%),
  caption: [Anemone 整体架构图。],
)

== 项目结构

Anemone采用单仓库组织内核、共享ABI、用户态组件、构建配置和工程文档。仓库根目录的
`Justfile`提供统一入口，复杂的配置解析、构建和运行流程由`scripts/xtask`承载；内核与用户态则通过
`anemone-abi`中的共享类型和系统调用编号保持一致。重要目录如下所示。

#code-block(
  ```text
  .
  ├── anemone-kernel/             # 内核主体与内核侧独立 crate
  ├── anemone-abi/                # 内核和用户态共享的 ABI 定义
  ├── anemone-rs/                 # Rust 用户态运行时与系统接口
  ├── anemone-libc/               # C 用户态支持库
  ├── anemone-apps/               # init、测试、工具和示例程序
  ├── conf/
  │   ├── system-targets/         # 系统镜像与启动目标组合
  │   ├── platforms/              # 机器、设备树和 QEMU 平台描述
  │   ├── kconfs/                 # 可复用的内核 Kconfig 配置
  │   ├── build-presets/          # 目标、Kconfig 与构建 profile 组合
  │   └── rootfs/                 # 根文件系统清单
  ├── scripts/xtask/              # 配置、构建、rootfs 与 QEMU 编排
  ├── scripts/                    # 端到端测试和专项验证脚本
  ├── symtab/                     # 内核符号表生成工具
  ├── docs/                       # RFC、当前契约、开发记录与问题登记
  ├── report/                     # 技术报告与展示材料
  ├── xref/                       # 固定版本的外部实现参考
  ├── Cargo.toml                  # 内核工作区及内部 crate 组成
  └── Justfile                    # 仓库统一操作入口
  ```.text,
  caption: [Anemone 顶层目录],
)

`anemone-kernel/src`按内核职责拆分。系统调用层负责Linux ABI的解析、用户指针访问和参数转换，
核心子系统拥有内部状态与生命周期，架构及驱动代码负责把这些机制接到具体CPU和设备。主要模块如下。

#code-block(
  ```text
  anemone-kernel/
  ├── src/
  │   ├── arch/                   # RISC-V64、LoongArch64 架构实现
  │   ├── exception/              # trap、异常、中断与 IPI 入口
  │   ├── syscall/                # syscall 注册、分发和用户访问边界
  │   ├── task/                   # 任务、线程组、信号、凭据与资源
  │   ├── sched/                  # 调度类、运行队列、等待与负载均衡
  │   ├── time/                   # clock、tick、timer 与 POSIX 时间机制
  │   ├── mm/                     # 物理页、页表、地址空间与缺页处理
  │   ├── fs/                     # VFS、mount、伪文件系统与文件后端
  │   ├── net/                    # 网络协议栈接入和网络工作线程
  │   ├── device/                 # 设备、总线、发现流程与 I/O 对象
  │   ├── driver/                 # 中断控制器、存储、串口、网卡等驱动
  │   ├── sync/                   # 锁、事件和内核同步原语
  │   ├── debug/                  # 日志、KUnit 与性能观测设施
  │   └── uts/                    # 系统身份与 UTS namespace
  └── crates/
      ├── anemone-net-api         # 内核与协议栈之间的窄接口
      ├── anemone-smoltcp-stack   # smoltcp 网络栈适配层
      ├── buddy-system            # 伙伴页分配器
      ├── device-tree             # 设备树解析
      ├── idalloc                 # 标识符分配
      ├── range-allocator         # 区间分配
      ├── kernel-macros           # 内核过程宏
      ├── la-insc                 # LoongArch 指令支持
      └── anemos/                 # 内核使用的外部 crate 适配版本
  ```.text,
  caption: [Anemone 内核主体目录],
)

== 开发人员分工

- 张正翰：进程管理，内存管理，文件系统，IPC，网络栈，设备驱动，RISC-V架构适配，时间管理，syscall实现，文档撰写，测例支持。

- 陈函申：PCIe总线，进程管理，内存管理，LoongArch架构适配，物理开发版移植，测例支持。
