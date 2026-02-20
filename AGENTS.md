# Anemone 代码代理指引

## 项目总览（先看这些）
- 这是一个 Rust `no_std` 内核工程，主内核在 `anemone-kernel/`，ABI 在 `anemone-abi/`，构建编排在 `scripts/xtask/`。
- 用户态相关仓库内联：`anemone-libc/` 与 `anemone-rs/`；符号表工具在 `symtab/`。
- 根 `Justfile` 只是薄封装，真实逻辑在 `xtask` 子命令（`build/qemu/clean/mrproper`）。
- 内核入口是 `anemone-kernel/src/main.rs` 的 `kernel_main()`；架构选择通过 `anemone-kernel/src/arch/mod.rs` 的 `arch_select!` 宏完成。

## 关键架构边界
- 平台与构建配置分离：
	- `kconfig`/`conf/.defconfig` 定义 profile、features、parameters。
	- `conf/platforms/*.toml` 定义架构常量与 QEMU 参数。
	- `conf/arch/<arch>/kernel.lds.in` 与 `target.json` 是架构模板与目标配置来源。
- `xtask build` 会**生成**并覆盖：
	- `anemone-kernel/src/kconfig_defs.rs`
	- `anemone-kernel/src/platform_defs.rs`
	- `build/generated/kernel.lds`
	不要手改这些生成文件。
- 架构层通过 `Cur*Arch` 访问（`CurCpuArch`, `CurExceptionArch`, `CurPagingArch`, `CurPowerArch`, `CurTimeArch`），入口集中在 `anemone-kernel/src/arch/mod.rs`。
- 设备发现走 Open Firmware/DTB 路线，内存区注册在 `anemone-kernel/src/device/discovery/open_firmware.rs`（`early_scan_and_register_memory`）。
- 异常/中断抽象在 `anemone-kernel/src/exception/hal.rs`（`ExceptionArch`, `IrqGuard`），时间抽象在 `anemone-kernel/src/time/hal.rs`（`TimeArch`）。硬件抽象层统一定义在上层模块而非架构模块，在大多数情况下，遵循依赖倒置原则。
- 低层调度抽象目前占位于 `anemone-kernel/src/sched/hal.rs`，新增架构调度原语先在这里落地。

## 开发工作流（默认用这些命令）
- 初始化配置：`just defconfig`（复制 `conf/.defconfig` 到仓库根 `kconfig`）。
- 构建：`just build`（等价 `just xtask build`）。
- 运行 QEMU：`just xtask qemu --platform qemu-virt-rv64 --image build/anemone.elf`。
- 清理：`just clean`；彻底清理（含配置/生成文件）：`just mrproper`。
- 调试参考：`scripts/qemu-virt-rv64-dbg.just`（`qemu-server` + `gdb-client`）。

## 代码约定（本仓库特有）
- 常用导入统一走 `anemone-kernel/src/prelude.rs`；新模块优先 `use crate::prelude::*;` 保持风格一致。
- 日志使用内核宏（`kdebugln!`, `kinfoln!`, `kerrln!` 等），实现位于 `anemone-kernel/src/debug/printk/mod.rs`。
- 架构相关实现放在 `anemone-kernel/src/arch/<arch>/`，并通过 `Cur*Arch` 别名接入（见 `arch/mod.rs`）。
- 内核子 crate 放在 `anemone-kernel/crates/`，优先在这里扩展通用能力，再由内核主 crate 引用。
- `prelude` 统一 re-export：架构别名、内存地址/分页类型、错误、时间/调度 HAL、锁与常用宏，新增模块尽量复用已有导出，避免重复 `use`。

## 集成点与改动提示
- 新增系统调用/错误码时，优先同步 `anemone-abi/src/*`（如 `errno.rs`, `syscall.rs`），内核侧通过 `AsErrno` 映射。
- `anemone-abi/build.rs` 会用 `cbindgen` 生成 `anemone-abi/bindings.h`；改 ABI 后注意检查 C 头同步结果。
- 构建产物关键路径：`build/anemone.elf`、`build/anemone.disasm`、`build/kernel.map`，定位链接/启动问题优先看这三处。
- 平台常量（如 `kernel_va_base`, `phys_ram_start`）在 `conf/platforms/*.toml`，不要在内核代码里硬编码重复值。
