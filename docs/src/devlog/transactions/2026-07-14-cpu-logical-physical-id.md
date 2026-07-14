# 2026-07-14 - CPU Logical / Physical ID

**Status:** Completed
**Owners:** EDGW_, Codex
**Area:** CPU discovery / bootstrap / per-CPU / scheduler / interrupt / architecture
**Canonical Plan:** [RFC-20260714-cpu-logical-physical-id](../../rfcs/cpu-logical-physical-id/index.md)
**Current Phase:** Closed

## Scope

本事务把原先同时表示软件下标和硬件标识的 `CpuId(usize)` 拆为逻辑 `CpuId` 与物理 `PhysCpuId`，建立 early-scan CPU registry，并迁移 per-CPU、scheduler、IPI、PLIC、架构启动和 scheduler stack 边界。

本事务不实现 CPU hotplug、任意高位稀疏物理 ID 的 bootstrap trampoline、PLIC `interrupts-extended` 解析或入口汇编栈协议重写。

## Invariants

- registry Vec 下标是连续逻辑 ID，元素是对应物理 ID。
- registry 是 mapping 和 CPU count 的唯一真相源，AP 启动前永久封存。
- 软件 owner 使用 `CpuId`，最终硬件边界使用 `PhysCpuId`。
- 反向映射是 bootstrap-only 的 O(CPU 数量) 操作。
- `STACK0` 和 guarded scheduler stack 按物理槽位归属；进入 scheduler 后由 `sched_ctx.sp` 维持。

完整定义见 [CPU Identity 不变量](../../rfcs/cpu-logical-physical-id/invariants.md)。

## Phase Log

### 2026-07-14 - Registry 与身份边界实现

**Phase:** 1-4 / implementation

**Change:** `device/cpu.rs` 新增锁保护的 `Vec<PhysCpuId>` registry，early scan 分配连续逻辑 ID 并封存；删除独立 `NCPUS` 和 `boxcar` 依赖。RISC-V、LoongArch 各自负责 CPU discovery 与注册，BSP/AP 只在启动时执行物理到逻辑反查。per-CPU、scheduler、task/kthread、timer worker、IPI queue 和 procfs 改用逻辑 ID；SBI、LoongArch IPI/mailbox、PLIC context 和架构 timer 改用物理 ID。

**Audit:** 源码搜索确认 `from_physical_id()` 只存在于两架构 BSP/AP setup；`GUARDED_STACK_TOPS` 只在两架构 `switch_to_guarded()` 查询。CPU-facing 旧 `.get()` 调用已按逻辑索引或物理硬件边界分类。`boxcar` 已从 manifests 与 lockfile 移除。

**Observability:** `register_cpu()` 打印每个逻辑 `CpuId` 到 `PhysCpuId` 的映射；普通逻辑 CPU 日志直接格式化 `CpuId`。重复注册、超出逻辑 `MAX_CPUS`、封存后注册和封存前读取通过纯检查断言暴露；状态变更不写入断言。

**Feedback:** 实现先于公开 RFC 补写。用户依次收紧实现形状：使用 `alloc::Vec` 而非 `boxcar`；物理类型命名为 `PhysCpuId`；registry Vec 必须上锁而非使用 `UnsafeCell`；反向扫描必须注释成本；副作用必须与断言分离；日志直接格式化逻辑 `CpuId`。这些反馈已折回 RFC canonical 文本，未改变目标或削弱不变量。

**Validation:** `visionfive2-rv64` 下 `just build` 通过；临时切换 `qemu-virt-la64` 后 `just build` 通过，随后恢复 `visionfive2-rv64` 并再次构建通过。`git diff --check` 通过。`just fmt kernel --check` 只命中本轮 write set 外既有格式差异，本轮文件没有 formatter diff。构建保留既有 SBI legacy console deprecation warning。

**Next:** 用户侧 RISC-V 实机验证。

### 2026-07-14 - RISC-V 实机验证与收口

**Phase:** 5 / validation and closure

**Change:** 无新增代码；按用户返回的实机结果收口 RFC 和事务状态。

**Audit:** 实机证据不改变 accepted contract；静态 scheduler stack 的物理槽位范围仍是本 RFC 接受的启动协议设计边界。

**Observability:** 用户确认 RISC-V 实机在本改造后运行通过。agent 未独立保存该次实机原始日志，因此这里只记录用户确认，不复制或推断未提供的 hart 映射细节。

**Feedback:** `None`；目标和不变量保持不变。

**Validation:** 用户侧 RISC-V 实机通过。LoongArch 只完成 agent 侧构建；按用户要求未继续运行时验证。

**Next:** 事务关闭。未来若需要 CPU hotplug、任意高位稀疏物理 ID 或 PLIC DT context，启动 follow-up RFC。

## Open Items

- 无。

## Closure

逻辑/物理 CPU identity 拆分已实现并通过 RISC-V、LoongArch 构建；用户确认 RISC-V 实机运行通过。RFC 状态改为 `Implemented / Closed`。CPU hotplug、超出静态 scheduler stack 槽位范围的物理 ID 和完整 PLIC DT context 不在本设计范围内。
