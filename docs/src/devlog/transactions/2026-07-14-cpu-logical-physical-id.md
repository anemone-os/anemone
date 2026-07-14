# 2026-07-14 - CPU Logical / Physical ID

**Status:** Validation Pending
**Owners:** EDGW_, Codex
**Area:** CPU discovery / bootstrap / per-CPU / scheduler / interrupt / architecture
**Canonical Plan:** [RFC-20260714-cpu-logical-physical-id](../../rfcs/cpu-logical-physical-id/index.md)
**Current Phase:** Post-close correction / user validation

## Scope

本事务把原先同时表示软件下标和硬件标识的 `CpuId(usize)` 拆为逻辑 `CpuId` 与物理 `PhysCpuId`，建立 early-scan CPU registry，并迁移 per-CPU、scheduler、IPI、PLIC、架构启动和 scheduler stack 边界。

本事务不实现 CPU hotplug、任意高位稀疏物理 ID 的 bootstrap trampoline、PLIC `interrupts-extended` 解析或入口汇编栈协议重写。

## Invariants

- registry 的逻辑索引前缀是连续逻辑 ID 到物理 ID 的唯一映射。
- registry 是 mapping 和 CPU count 的唯一真相源，AP 启动前永久封存。
- 软件 owner 使用 `CpuId`，最终硬件边界使用 `PhysCpuId`。
- platform `MAX_PHYS_CPU_ID` 与 kconfig `MAX_LOGICAL_CPUS` 分别拥有物理 ID backing 和逻辑启用容量。
- 固定 per-CPU backing 通过 `CpuTable` / `PhysCpuTable` 限制索引身份。
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

### 2026-07-14 - 更正：PLIC follow-up 文档层级

前文 `Next` 将后续 PLIC DT context 解析预设为 follow-up RFC。实际改动局限于 PLIC
device-tree 解析和同一设备 owner 内的运行时映射，不需要跨子系统 contract 或阶段 gate，
因此按 [PLIC device-tree context 小迭代记录](../changes/2026-07-14-plic-dt-context.md)
归档，不建立独立 RFC 或事务日志。CPU identity 本身的范围和不变量不变。

### 2026-07-14 - Post-close 静态无锁 Registry 与逻辑容量语义

**Phase:** post-close implementation feedback

**Change:** 用户要求用固定 cache-padded 槽位替换带锁 `Vec`，并进一步明确 registry 不应加锁。最终 registry 使用 `[CachePadded<MonoOnce<PhysCpuId>>; MAX_CPUS]`；BSP 是 AP 启动前的唯一 writer，槽位先初始化、逻辑计数后推进，`registration_complete` 以 Release/Acquire 发布整个前缀。两架构 early scan 把 BSP 物理 ID 传给 registry：`MAX_CPUS` 只限制逻辑 CPU 数，超限时保留 BSP 和前 `MAX_CPUS - 1` 个 AP，统一打印 `kwarningln!` 并忽略剩余可用 CPU。

**Audit:** registry 不再持有 `Vec` 或 `NoIrqRwLock`；容量判断只使用已注册逻辑 CPU/AP 数，不比较 `PhysCpuId` 数值。BSP 槽位在扫描 BSP 节点前即通过 AP 上限预留，因此不依赖设备树把 BSP 排在前部。

**Validation:** Pending dual-architecture build and source audit.

### 2026-07-14 - 更正：物理 ID 上界与逻辑 CPU 容量拆分

**Phase:** post-close runtime feedback correction

**Symptom:** 用户运行在 VisionFive 2 上完成 `PhysCpuId(1)`、`PhysCpuId(2)` 的 scheduler stack remap 后，于 RISC-V `remap_boot_stack()` 处理下一 CPU 时 panic：长度 3 的数组被索引 3。该证据由用户在对话中提供，agent 未持有完整原始日志。

**Root Cause:** 单一 platform `MAX_CPUS=3` 同时充当最大启用逻辑 CPU 数和物理 ID 索引数组长度。逻辑数量 3 的合法拓扑可以包含物理 ID 1、2、3，但物理 ID 3 不能索引长度 3 的 `GUARDED_STACK_TOPS`。这不是 registry 截断顺序问题，而是配置 owner 与数组索引域混用。

**Change:** platform config 改为含端点的 `max_phys_cpu_id` / `MAX_PHYS_CPU_ID`，kconfig 新增 `max_logical_cpus` / `MAX_LOGICAL_CPUS`。early scan 遇到物理 ID 越界时逐个 `kwarningln!` 并跳过，随后仍按逻辑上限保留 BSP 和前 N-1 个 AP。新增全内联 `CpuTable` / `PhysCpuTable`：registry 与 `PERCPU_BASES` 使用逻辑索引，`STACK0` 与 guarded tops 使用物理索引；`PhysCpuTable` 保持 transparent layout 供入口汇编访问。

**Validation:** Agent 在用户要求停止测试前完成当前 `visionfive2-rv64` 的 `just build`，生成值为 `MAX_LOGICAL_CPUS=3`、`MAX_PHYS_CPU_ID=4`；`git diff --check` 和 split-bound source audit 通过。`just fmt kernel --check` 仍被 write set 外既有 `lwext4`、PLIC、generated defs 和 kthread 格式差异阻塞，本轮命中的文件已按 formatter 输出修正但未按用户要求复跑。LoongArch 构建与 VisionFive 2 运行时复验未运行，由用户接手。

### 2026-07-15 - Typed Array 默认容量与 VisionFive 2 复验

**Phase:** post-close runtime validation

**Change:** typed container 最终命名为 `CpuTable` / `PhysCpuTable`，并增加可覆盖的 const 泛型容量；默认值分别为 `MAX_LOGICAL_CPUS` 和 `MAX_PHYS_CPU_ID + 1`。现有 registry、per-CPU base 与 bootstrap stack 调用均使用默认容量，索引类型边界保持不变。

**Validation:** 用户确认修正后的 VisionFive 2 运行测试成功，原 `PhysCpuId(3)` guarded-stack 越界未再出现。该证据由用户提供，agent 未持有原始运行日志。LoongArch 构建仍未运行，事务保持 validation pending。
