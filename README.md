![hitsz](./report/assets/school.jpg)

# Anemone

## 项目简介

Anemone 是一个使用 Rust 实现、支持 RISC-V64 与 LoongArch64 平台的操作系统内核。

在开发过程中，我们始终避免为了对特定测例进行特化适配而妥协系统设计。Anemone 的目标是：在 Linux ABI 兼容性、进程线程管理、虚拟内存、VFS 与文件系统、设备驱动模型、IPC、同步和体系结构适配等核心能力域上形成**可解释、可维护、可持续演进**的系统实现。

## 完成情况

### 初赛情况

截至初赛结束，Anemone 已经通过初赛测例的大部分测例，并通过了大量 LTP 测例点。

![leader-board-rank](./report/assets/rank.png)

### Anemone内核介绍

- **进程管理** 实现 task / thread group / process group 等执行实体管理，覆盖 fork / clone / exec / exit / wait 等生命周期路径。
- **调度** 围绕 scheduler、wait-core、signal interruption 形成阻塞、唤醒与可中断等待路径。
- **内存管理** 实现地址空间、页表、缺页处理、匿名页、VMO / backing object、file-backed mapping、共享内存与内存压力相关路径。
- **IPC** 覆盖 signal、pipe、System V IPC、event/timer 类文件对象、poll/select 等等待组合路径。
- **文件系统** 实现 VFS、路径查找、mount view、opened file object、procfs、devfs 和多类文件后端的统一接入。
- **设备驱动模型** 实现设备发布、字符/块设备、ioctl 分发和若干具体设备对象。
- **时间** 围绕 clock、tick、IRQ / threaded soft timer、timerfd 和 itimer 组织时间线、超时与定时通知。
- **架构抽象层** 支持 RISC-V64 与 LoongArch64 的启动、trap、中断、上下文保存和平台差异收束

<img src="./report/assets/anemone-architecture.png" alt="Anemone架构概览" width="1000"/>

### 文档

- [初赛阶段文档](./report/build/anemone-report.pdf)
- [项目开发简介幻灯片](./report/Anemone初赛演示文稿.pptx)
- [演示视频](https://pan.baidu.com/s/1rhglWFYPBpUGX7G0ZbcY1A?pwd=kafu) 提取码：kafu

### 项目结构

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
```

内核主体按子系统拆分如下。

```text
arch       # RISC-V64 / LoongArch64 架构入口
exception  # trap、异常和中断入口
syscall    # Linux syscall 分发与 ABI 解析
task       # task、线程组、进程拓扑、信号和资源
sched      # 调度器、等待和运行队列
time       # 时钟、tick、timer、itimer 和时间 API
mm         # 地址空间、页表、物理页和缺页路径
fs         # VFS、mount、procfs、devfs 和文件系统后端
device     # 设备模型、设备发现和 I/O class
driver     # 块设备、串口、中断控制器、virtio 等驱动
sync       # 内核同步原语
crates     # 独立 crate
├── buddy-system
├── device-tree
└── la-insc
```

## 分支与复现方式

Anemone 的主线开发希望保持中性的项目门面。`main` 分支长期保留内核、通用构建系统和通用文档；比赛报告、提交材料、比赛环境适配和评测复现入口不长期进入 `main`。

本分支 `submit/prim` 是初赛提交和评审用的 orphan 快照分支。它保留了初赛报告、演示材料、公开的 pretest rootfs 配置和面向评委的运行说明。若只是希望在当前提交形态上测试 Anemone，不需要复现比赛平台日志，可以直接在 `submit/prim` 上按下面的开发方式准备镜像并运行。

`kako/bench` 是按赛方提交协议整理的分支，用于复现比赛日志。该分支假定自己运行在赛方提供的 Docker 环境内：开发者需要自行准备赛方根文件系统/测试盘镜像，进入赛方 Docker，执行 `make`，再使用赛方给出的 QEMU 命令运行。这个路径尽量贴近比赛平台的构建和启动方式；它和 `submit/prim` 的开发容器/`xtask` 路径不是同一个入口。

## 开发与构建

### 推荐开发环境

推荐的开发方式是使用 VS Code Dev Containers 直接进入仓库开发容器。安装 VS Code 的 Dev Containers 扩展后，在仓库根目录选择 `Reopen in Container`；`.devcontainer/devcontainer.json` 会基于仓库 `Dockerfile` 的 `fin_dev` 阶段构建开发环境。

`fin_dev` 阶段包含本项目需要的主要工具链和运行依赖，包括 Rust / cargo 工具、`just`、`cargo-binutils`、QEMU、`libguestfs-tools`、`libclang`、lwext4 交叉工具链等。Rust 具体版本和组件由仓库根目录的 `rust-toolchain.toml` 决定。

如果不使用开发容器，需要在本机安装与 `Dockerfile` 的 `fin_dev` 阶段等价的依赖。至少需要确保以下内容可用：

- `just` 与仓库 `rust-toolchain.toml` 指定的 Rust toolchain；
- `rust-objdump` / `rust-objcopy` 等 `cargo-binutils` 工具；
- `qemu-system-riscv64` 与 `qemu-system-loongarch64`；
- `virt-make-fs` 及其所需的 libguestfs / supermin 环境；
- `LWEXT4_TOOLCHAIN_RISCV64` 与 `LWEXT4_TOOLCHAIN_LOONGARCH64` 指向可用的 lwext4 交叉工具链。

### 构建入口与配置约定

Anemone 的构建、rootfs 生成、平台切换和 QEMU 运行都通过 `Justfile` 与 `scripts/xtask` 进入。不要直接在工作区或 `anemone-kernel` 中调用 `cargo build`，因为 `xtask` 会负责生成目标描述、内核链接脚本、`kconfig_defs.rs`、`platform_defs.rs` 以及 `build/` 下的输出文件。

仓库根目录的 `kconfig` 是当前开发者的本地内核构建配置。它不进入公共仓库，首次构建前可以用默认配置初始化：

```bash
just defconfig
```

`kconfig` 中最重要的构建字段是 `[build].platform`。它必须对应 `conf/platforms/*.toml` 中的某个平台配置；平台配置决定目标架构、内存布局、QEMU 参数和启动 rootfs 设备。常用命令如下：

```bash
just conf list
just conf switch qemu-virt-rv64-pretest
just conf switch qemu-virt-la64-pretest
```

`kconfig` 还承载内核 feature 开关和重要参数，例如日志等级、内核栈大小、进程数量上限、系统 tick 频率、设备数量等。重要常量一般都会放在 `kconfig`，而不是散落在源码中。面向开发者个人环境的 `kconfig`、本地 rootfs manifest、磁盘镜像和构建输出不应提交到公共仓库。

### rootfs manifest 与镜像输入

Anemone 当前有两类磁盘输入：

- 启动 rootfs 镜像：由 `just rootfs mkfs -c <rootfs-manifest>` 生成，输出位于 `build/rootfs/<name>/rootfs.img`。
- 测试盘镜像：由使用者自行提供，用于 `user-test` 挂载测试环境，通常来自赛方根文件系统/测试盘。

`conf/rootfs/*.toml` 是 rootfs manifest。manifest 描述如何生成启动 rootfs：

- `[fs].base` 指向一个基础目录，构建时会先复制进 rootfs staging tree；
- `[[dirs]]` 声明需要额外创建的目录，例如 `/dev` 和 `/mnt`；
- `[[apps]]` 声明需要构建并安装进 rootfs 的 Anemone 用户态应用；
- `[[files]]` 声明需要从宿主机复制进 rootfs 的文件，适合放置二进制 fixture。

`submit/prim` 提供了两套可直接使用的 pretest manifest 和基础目录：

| 架构        | rootfs manifest                 | 启动 rootfs 输出                       | 平台配置                 | 测试盘路径      |
| ----------- | ------------------------------- | -------------------------------------- | ------------------------ | --------------- |
| RISC-V64    | `conf/rootfs/pretest-rv64.toml` | `build/rootfs/pretest-rv64/rootfs.img` | `qemu-virt-rv64-pretest` | `sdcard-rv.img` |
| LoongArch64 | `conf/rootfs/pretest-la64.toml` | `build/rootfs/pretest-la64/rootfs.img` | `qemu-virt-la64-pretest` | `sdcard-la.img` |

对应的公开基础目录位于 `conf/rootfs/pretest-rv64-base` 和 `conf/rootfs/pretest-la64-base`。它们当前只包含启动所需的少量 loader 文件，但保留为 rootfs base，是为了展示构建系统对基础文件树的支持。

`sdcard-rv.img` 与 `sdcard-la.img` 不由仓库生成。若手动运行 QEMU，需要把对应测试盘镜像放在仓库根目录，并使用上表中的文件名。即使只是执行 `just build`，当前构建流程也会通过 QEMU 生成 DTB，因此平台 QEMU 参数引用的启动 rootfs 和测试盘路径可能需要提前存在；这是当前构建系统的一个已知粗糙点。

内核运行、`init` 和 `user-test` 都可能写入启动 rootfs 镜像和测试盘镜像；一次运行结束后，镜像内容不再保证是原始状态。建议把原版测试盘保存在仓库外或另一个文件名下，每次运行前复制成根目录的 `sdcard-rv.img` / `sdcard-la.img`，或者直接使用下面的端到端脚本由脚本完成复制。启动 rootfs 是 `build/` 下的生成物，需要恢复时重新执行 `just rootfs mkfs` 即可。

### 构建与运行

开发者若只想在 `submit/prim` 上运行当前 pretest 路径，推荐使用端到端 `user-test` 脚本。脚本会切换平台、使用公开 pretest manifest 重建启动 rootfs、把给定测试盘复制到平台要求的根目录文件名、构建内核并启动 QEMU：

```bash
./scripts/run-user-test-rv64.sh <sdcard-source-image> [log-file]
./scripts/run-user-test-la64.sh <sdcard-source-image> [log-file]
```

这里的 `<sdcard-source-image>` 建议指向仓库根目录之外的原版测试盘或副本，不要直接传根目录下的 `sdcard-rv.img` / `sdcard-la.img`；脚本会把它复制成平台要求的根目录文件名后运行。默认日志路径分别是 `build/user-test-rv64.log` 和 `build/user-test-la64.log`。

也可以手动执行同一条链路。RISC-V64：

```bash
just defconfig
just conf switch qemu-virt-rv64-pretest

cp <sdcard-source-image> sdcard-rv.img
just rootfs mkfs -c conf/rootfs/pretest-rv64.toml --sudo
just build
just xtask qemu --platform qemu-virt-rv64-pretest --image build/anemone.elf | tee build/user-test-rv64.log
```

LoongArch64：

```bash
just defconfig
just conf switch qemu-virt-la64-pretest

cp <sdcard-source-image> sdcard-la.img
just rootfs mkfs -c conf/rootfs/pretest-la64.toml --sudo
just build
just xtask qemu --platform qemu-virt-la64-pretest --image build/anemone.elf | tee build/user-test-la64.log
```

`just rootfs mkfs ... --sudo` 会在宿主侧调用镜像构建工具。若当前环境的 libguestfs / supermin 不需要提权，也可以去掉 `--sudo`。

### 调整 user-test / LTP 测例集合

`anemone-apps/user-test/ltp/profile.txt` 决定 `user-test` 当前运行哪些 LTP group。文件中每行写一个 group 名，空行和 `#` 注释会被忽略；写入 `all` 表示运行全部已登记 group。可用 group 位于 `anemone-apps/user-test/ltp/groups/`。

修改 `profile.txt` 或 group 文件后，需要重新构建 `user-test`，并重新生成包含它的启动 rootfs。最常见的方式是重新执行对应平台的 `just rootfs mkfs -c ...`，或者直接使用上面的 `run-user-test-*.sh` 脚本完成整条链路。

## 项目人员

哈尔滨工业大学（深圳）：
- 张正翰(doruche18@outlook.com)：进程管理，内存管理，文件系统，设备驱动，IPC，RISC-V架构适配，时间管理，syscall实现，文档撰写，测例支持。
- 陈函申(edgwunderline@outlook.com)：PCIe总线，进程管理，内存管理，LoongArch架构适配，测例支持。
- 指导老师：夏文，仇洁婷

## 参考

- [Linux](https://kernel.org) 设备驱动模型，MachineDesc，VFS，以及大量syscall
- [Zircon](https://fuchsia.dev) VMO架构
- [Chronix](https://github.com/PACTHEMAN123/Chronix) 用户态busybox安装
