# Anemone 代码代理指引
- Anemone是一个整体遵循宏内核架构的、主要使用Rust开发的操作系统，支持多架构（当前主要是RiscV64，后续计划支持LoongArch64和x86_64）。
- **Anemone的核心目标是在不失架构灵活性、高度可扩展性与性能的前提下，尽可能提供对Linux ABI的支持，同时相比于Linux，引入和借鉴更多现代内核的设计。**

## 仓库组织
- 主内核在 `anemone-kernel/`，ABI 在 `anemone-abi/`，构建编排在 `scripts/xtask/`。
- 用户态相关仓库内联：`anemone-libc/` 与 `anemone-rs/`；符号表工具在 `symtab/`。
- 根 `Justfile` 只是薄封装，真实逻辑在 `xtask` 子命令（`build/qemu/clean/mrproper`）。
- 内核的架构无关入口是 `anemone-kernel/src/main.rs` 的 `kernel_main()`；最底层启动代码位于`anemone-kernel/src/arch/<arch>/bootstrap.rs`（如 `riscv64/bootstrap.rs`）。
- 配置文件在 `conf/`，分为平台（`platforms/*.toml`）和架构（`arch/<arch>/kernel.lds.in`, `arch/<arch>/*-unknown-anemone-elf.json`）两类，构建时会生成对应的 Rust 定义和链接脚本。

## 关键架构边界
- 平台与构建配置分离：
	- `kconfig`/`conf/.defconfig` 定义 profile、features、parameters。
	- `conf/platforms/*.toml` 定义架构常量与 QEMU 参数。
	- `conf/arch/<arch>/kernel.lds.in` 与 `conf/arch/<arch>/*-unknown-anemone-elf.json` 是架构模板与目标配置来源。
- `xtask build` 会**生成**并覆盖：
	- `anemone-kernel/src/kconfig_defs.rs`
	- `anemone-kernel/src/platform_defs.rs`
	- `build/generated/kernel.lds`
	不要手改这些生成文件。
- 架构层通过 `*Arch` 访问（`CpuArch`, `IntrArch`, `TrapArch`, `PagingArch`, `TimeArch`, `KernelLayout`），入口集中在 `anemone-kernel/src/arch/mod.rs`。
- 设备发现目前提供 Open Firmware/DTB 路线，实现在 `anemone-kernel/src/device/discovery/open_firmware.rs`。
- 设备驱动模型（DDM）目前以 `platform bus` 为主线：
	- 基础对象 `KObject`/`KSet`，抽象为 `BusType`、`Device`、`Driver` 三层。
	- 注册路径在 `anemone-kernel/src/device/bus/`：注册 `device/driver` 时会立即尝试匹配并执行 `probe`；匹配由具体总线实现（当前核心是 `PlatformBusType::matches`，按 compatible 表匹配）。
	- 设备树根仍围绕 `/sys/devices/platform` 的 `ROOT_BUS` 组织，后续再扩展更完整的 `/sys/bus` 视图。
- fs / VFS 当前仍是“先把 namespace 跑通”的阶段：
	- `anemone-kernel/src/fs/` 里仍以 big-lock 为主，`SuperBlock`/`Dentry`/`Mount` 的共享状态优先靠大锁保护，先避免死锁和复杂交叉锁顺序。
	- 现在的 namespace 与存储层仍然分离：`PathRef` 只是 `mount + dentry`，还没有完整的 dcache、negative dentry，也没有通用的按路径 open/create 前端或 fd 表系统调用。
	- `namei.rs` 的路径解析仍在演进中，处理绝对路径、空路径和父目录回退时要格外小心，不要假设它已经覆盖 Linux 级别的全部语义。
	- `ramfs` 是当前最完整的轻量后端，`ext4` 主要是 `lwext4-rust` 的包装层；碰 `ext4` 时要留意属性缓存与卸载/evict 路径仍有未收口的行为。
- IRQ 子系统分层：
	- 架构本地中断开关与 IRQ flags 抽象在 `anemone-kernel/src/exception/intr/hal.rs`（`IntrArchTrait`、`IrqFlags`）。
	- 通用 IRQ 逻辑在 `anemone-kernel/src/exception/intr/irq.rs`：`IrqDomain`、`IrqChip`、`CoreIrqChip`、`request_irq`、`handle_irq` 等。
	- 当前 `irq` 子系统主要处理设备外部中断；timer 与 IPI 仍在 `anemone-kernel/src/exception/` 下分别处理。
- MM 子系统当前结构：
	- 顶层在 `anemone-kernel/src/mm/mod.rs`，核心模块包括 `frame`、`paging`、`kmalloc`、`remap`、`zone`、`kptable`。
	- `zone.rs` 维护可用/保留内存区（`SysMemZones`）；`frame` 的 PMM 初始化会基于可用 zone 建立页帧分配器。
	- `paging` 采用 higher-half 布局抽象，布局约束由 `KernelLayout` 与架构分页 HAL 共同确定。
- 进程 / 调度目前以 `anemone-kernel/src/sched/` 为主：
	- `sched/proc.rs` 持有每 CPU 的 `ProcessorInfo`，其中 `MonoFlow` 保护的 `ProcessorInner` 只允许单线程、非重入访问；相关 accessor 默认要求已经满足 IRQ / preempt 纪律。
	- `schedule()` 与 `task_exit()` 都假设调用点已经关中断；`task_exit()` 会主动关本地中断后再切换，不能在未理解当前 IRQ 状态时直接调用。
	- idle task 是 per-CPU 的，调度回落到 idle 时要保持内核对 IRQ 状态的预期，避免把 CPU 留在关中断状态。
	- 新增或修改任务/进程路径时，优先检查 `sched/proc.rs`、`sched/idle.rs` 和 `task/task.rs`，不要只盯着 `sched/hal.rs`。

## 开发工作流（默认用这些命令）
- 初始化配置：`just defconfig`（复制 `conf/.defconfig` 到仓库根 `kconfig`）。
- 构建：`just build`（等价 `just xtask build`）。
- 运行 QEMU：`just xtask qemu --platform qemu-virt-rv64 --image build/anemone.elf`。  
- **网络验收**：`-netdev user` 下**不要**用宿主机 `ping 192.168.100.2` 作为默认标准；串口日志、kunit、客户机内 ping 网关、或 TAP 平台见 [`anemone-kernel/docs/NETWORK_ROADMAP.md`](anemone-kernel/docs/NETWORK_ROADMAP.md) **§十**。可选 TAP 平台：`qemu-virt-rv64-tap`（需先在 Linux 宿主机创建 `anetap0` 并配置 `192.168.100.1/24`）。
- 清理：`just clean`；彻底清理（含配置/生成文件）：`just mrproper`。
- 调试参考：`scripts/qemu-virt-rv64-dbg.just`（`qemu-server` + `gdb-client`）。
- `xtask` 命令结构（概要）：`conf`（`list/switch`）、`build`、`qemu`、`clean`、`mrproper`。
	- `build` 过程会先清理并重建 `build/generated/`，再生成 defs/lds，最后调用内核 `cargo build` 并产出 `build/anemone.elf`。
	- `clean` 清理构建产物；`mrproper` 额外删除 `kconfig` 与生成的 `kconfig_defs.rs/platform_defs.rs`。

## 代码约定（本仓库特有）
- 常用导入统一走 `anemone-kernel/src/prelude.rs`；新模块优先 `use crate::prelude::*;` 保持风格一致。
- 日志使用内核宏（`kdebugln!`, `kinfoln!`, `kerrln!` 等），实现位于 `anemone-kernel/src/debug/printk/mod.rs`。
- 内核提供了基本测试框架`kunit`，实现位于`anemone-kernel/src/debug/kunit/`，在内核子系统初始化后、调度开始前运行。
- 架构相关实现放在 `anemone-kernel/src/arch/<arch>/`，并通过 `Cur*Arch` 别名接入（见 `arch/mod.rs`）。
- 内核子 crate 放在 `anemone-kernel/crates/`，优先在这里扩展通用能力，再由内核主 crate 引用。
- `prelude` 统一 re-export：架构别名、内存地址/分页类型、错误、时间/调度 HAL、锁与常用宏，新增模块尽量复用已有导出，避免重复 `use`。

## 集成点与改动提示
- 新增系统调用/错误码时，优先同步 `anemone-abi/src/*`（如 `errno.rs`, `syscall.rs`），内核侧通过 `AsErrno` 映射。
- `anemone-abi/build.rs` 会用 `cbindgen` 生成 `anemone-abi/bindings.h`；改 ABI 后注意检查 C 头同步结果。
- 构建产物关键路径：`build/anemone.elf`、`build/anemone.disasm`、`build/kernel.map`，定位链接/启动问题优先看这三处。
- 平台常量（如 `kernel_va_base`, `phys_ram_start`）在 `conf/platforms/*.toml`，不要在内核代码里硬编码重复值。
