# ANE-CHG-20260713-riscv-ad-bits-svade

**Type:** Bugfix / Architecture Compatibility
**Status:** Completed
**Date:** 2026-07-13
**Authors:** EDGW_, Codex
**Area:** mm / RISC-V / paging / Svade

## Problem

RISC-V PTE 定义了 Accessed（A）和 Dirty（D）位，但架构无关的 `PteFlags` 不暴露这两个
状态。此前只有 bootstrap 页表通过 `BOOTSTRAP_KERNEL` / `BOOTSTRAP_RAM` 显式预置 A/D；
运行期内核映射和用户映射从通用 `PteFlags` 转换为 `RiscV64PteFlags` 时只写入
V/R/W/X/U/G，因此新建叶子 PTE 的 A/D 初始值都是 0。

当前 QEMU virt CPU 提供 Svadu，硬件可以自动更新 A/D，所以这一路径通常不会暴露问题。
在实现 Svade 或令 `menvcfg.ADUE=0` 的平台上，访问 A=0 的叶子或写入 D=0 的可写叶子会触发
页故障。现有用户页故障路径会按 VMA 重新安装同样 A/D=0 的 PTE，内核页故障路径则是致命
错误，因此内核并不具备软件维护 A/D 的兼容路径。

这个问题只涉及 RISC-V PTE 编码策略，不改变架构无关 MM contract，也不需要跨子系统阶段
gate，因此按小迭代处理，不升级为 RFC。

## Scope

本轮只调整 RISC-V 通用页表项编码：

- 所有带 R/W/X 权限的叶子 PTE 在创建和改权时强制设置 `A=1,D=1`；
- 分支 PTE 的 A/D 保持为 0，因为这些位在非叶 PTE 中为保留位；
- bootstrap 页表继续使用已有的 `BOOTSTRAP_KERNEL` / `BOOTSTRAP_RAM` 组合位。

本轮不把 A/D 加入架构无关 `PteFlags`，不实现 accessed/dirty accounting、页面回收或文件
脏页跟踪，不新增 Svadu/Svade 能力探测或 `ADUE` 配置，也不修改 LoongArch 页表语义。

## Solution

在 `From<PteFlags> for RiscV64PteFlags` 的架构转换边界检查 `flags.is_leaf()`；只要通用
权限包含 R/W/X，就在结果中补上 `RiscV64PteFlags::ACCESSED | DIRTY`。

`RiscV64Pte::new()` 和 `RiscV64Pte::set_flags()` 都复用这条转换，因此该规则同时覆盖首次
映射、`mprotect`、fork/COW 改权和其它通用 PTE flag 更新。相比在各个 mapper caller 中
分别补位，这个方案让 RISC-V PTE 编码保持单一入口；相比处理 Svade page fault，它也避免
引入当前 MM 不消费的 A/D 状态机。

预置 D 位意味着当前内核不会从 RISC-V PTE 恢复真实的首次写入或 dirty 信息。这与修改前
架构无关 MM 已经不暴露 A/D 的 contract 一致；如果后续需要页访问统计或 PTE dirty
tracking，应重新设计架构无关状态所有权，而不是移除本兼容规则后隐式依赖 fault。

## Change

- `anemone-kernel/src/arch/riscv64/mm/generic.rs`
  - RISC-V 叶子 PTE 的通用 flag 转换统一补齐 A/D；
  - 注释记录 Svade / hardware A/D disabled 的兼容原因，以及分支 PTE 必须保持 A/D 清零的
    边界。

## Validation

Agent-run validation:

- 源码审计确认 `RiscV64Pte::new()` 和 `set_flags()` 都经过修改后的转换；
- 源码审计确认直接调用 `RiscV64Pte::arch_new()` 的位置仅用于 bootstrap 页表，且已有
  `BOOTSTRAP_KERNEL` / `BOOTSTRAP_RAM` 预置 A/D；
- `git diff --check -- anemone-kernel/src/arch/riscv64/mm/generic.rs` 通过；
- 当前 `visionfive2-rv64` dev 配置下 `just build` 通过，生成 `build/anemone.elf` 和
  `build/anemoneImage-rv64`；仅保留现有 `sbi_rt::legacy::console_putchar` deprecated warning；
- `just fmt kernel --check` 已运行，但被本轮 write set 外的既有格式差异阻塞；目标
  `generic.rs` 不在 formatter diff 中。

Agent 未运行 Svade / `ADUE=0` 的 QEMU runtime。当前构建目标是 VisionFive 2，其物理内存
布局不能直接作为 QEMU virt 镜像使用。

## Tracking Issues

### CHG-001 - ADUE=0 runtime validation

**Status:** Deferred
**Severity:** Euclid

**Issue:** 源码路径和 RISC-V 构建已经闭合，但尚未在关闭 Svadu hardware update 的 QEMU
CPU 上执行启动、用户映射、写入和 PTE 改权验证。

**Resolution:** 保留为本小迭代的验证缺口，不新增 register 条目。后续应使用
qemu-virt-rv64 配置构建匹配镜像，以关闭硬件 A/D update 的 CPU 模式至少覆盖内核正式页表
切换、用户匿名页读写和一次 `mprotect` / COW 改权路径；若仍出现页故障，再按 trap 类型和
原始 PTE 位值重新归因。

## Risk / Follow-up

- 该策略兼容 Svadu 和 Svade：支持硬件更新的平台看到 A/D 已置位，不支持硬件更新的平台
  不会因 A/D 为 0 触发软件维护 fault。
- 未来若引入 accessed/dirty accounting，必须先明确 A/D 状态由架构页表层还是通用 MM
  owner 持有，并重新评估“叶子永远置 1”的兼容策略。
- 当前没有仍然生效的缺陷或接受限制需要写入 register；未运行的 ADUE=0 runtime 证据保留
  在本记录的 Tracking Issues 中。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- Register / limitations: None.
