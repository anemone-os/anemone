# RFC-20260714-cpu-logical-physical-id

**状态：** Implemented / Closed
**负责人：** EDGW_, Codex
**最后更新：** 2026-07-14
**领域：** CPU discovery / bootstrap / per-CPU / scheduler / interrupt / architecture
**事务日志：** [2026-07-14-cpu-logical-physical-id](../../devlog/transactions/2026-07-14-cpu-logical-physical-id.md)
**开放问题：** None
**下一步：** 后续若需要 CPU hotplug、任意高位稀疏 hart ID 或完整 PLIC DT context 解析，另走 RFC review。

## 摘要

旧 CPU identity 模型把 `CpuId(usize)` 同时用于 per-CPU 数组下标、调度器放置和硬件 hart/core 标识，隐含假设固件物理 ID 从 0 连续排列。该假设在具有非零 boot hart 或固件保留 hart 的平台上不成立，并会让 AP 唤醒、IPI、PLIC context 与 per-CPU 索引混用同一个整数。

本 RFC 把 CPU identity 拆成连续逻辑 `CpuId` 和固件/硬件可见 `PhysCpuId`。CPU early scan 按可用节点顺序注册 CPU；注册表 `Vec<PhysCpuId>` 的下标即逻辑 ID，元素是对应物理 ID。调度、per-CPU、任务放置和 procfs 使用逻辑 ID，只有硬件边界与初始 scheduler stack 槽位使用物理 ID。

## 背景

改造前存在以下耦合：

- `PERCPU_BASES`、任务 `cpuid` 和 scheduler round-robin 需要范围为 `0..ncpus()` 的连续下标。
- RISC-V SBI `hart_start` / `send_ipi`、LoongArch mailbox / IPI 和 PLIC context 需要固件或硬件标识。
- 两架构入口汇编在 per-CPU 初始化前直接用固件物理 ID 选择静态 `STACK0`。
- `STACK0` 在进入 stage 2 后不会释放；guarded alias 的栈指针会保存进 per-CPU `sched_ctx`，长期作为 scheduler stack。

旧实现用 `0..ncpus()` 同时驱动软件数组和硬件调用，因此无法自然表达“4 个可用 CPU，但物理 ID 不是 0、1、2、3”的拓扑。

## 目标

- `CpuId` 只表示从 0 开始、连续分配的逻辑 CPU ID。
- `PhysCpuId` 只表示固件或硬件可见的 CPU ID。
- 以一个 CPU registry 作为逻辑到物理映射和 CPU 数量的单一真相源。
- 在架构 early scan 中完成可用 CPU 注册，在 AP 启动前封存 registry。
- per-CPU、scheduler、任务放置、kthread、timer worker、IPI queue 和 procfs 使用逻辑 ID。
- SBI、LoongArch IPI/mailbox、PLIC context 和架构 timer 边界使用物理 ID。
- 保留初始 scheduler stack 的物理槽位归属，进入 scheduler 后不再按 ID 重复查询栈。
- 让日志能同时关联逻辑 ID 与物理 ID，并让普通逻辑 CPU 日志直接格式化 `CpuId`。

## 非目标

- 不实现 CPU hotplug、registry reopen、CPU 下线或运行期拓扑变化。
- 不把 BSP 强制映射成逻辑 CPU 0；逻辑顺序由 early scan 的可用节点顺序决定。
- 不重写 RISC-V 或 LoongArch 入口汇编的初始栈选择协议。
- 不为任意大或超出 `STACK0` 槽位范围的稀疏物理 ID 引入 trampoline/emergency stack。
- 不在本 RFC 中解析 PLIC `interrupts-extended`；当前 context 公式只改为消费 `PhysCpuId`。
- 不改变 Linux 可见的 `/proc/<pid>/stat` processor 编号和 CPU allowed mask 的逻辑编号语义。

## 文档地图

Canonical：

- [CPU Identity 不变量](./invariants.md)
- [迁移实施计划](./implementation.md)

执行事实：

- [CPU Logical / Physical ID 事务日志](../../devlog/transactions/2026-07-14-cpu-logical-physical-id.md)

## 方案

### Registry 与身份类型

`device/cpu.rs` 拥有 `NoIrqRwLock<Vec<PhysCpuId>>`。`register_cpu()` 在 early scan 中把物理 ID 追加到 Vec，追加前长度就是新分配的逻辑 ID。registry 封存后：

- `CpuId::logical_id()` 返回 Vec 下标；
- `CpuId::physical_id()` 按逻辑下标读取 `PhysCpuId`；
- `ncpus()` 从 Vec 长度派生，不维护第二份 CPU count；
- `CpuId::from_physical_id()` 线性扫描 Vec，只允许用于 BSP/AP 启动转换。

registry 的锁负责并发访问安全，`registration_complete` 只表达“扫描中/已封存”的协议阶段，避免调用者读取部分拓扑。

### 启动与架构边界

RISC-V 和 LoongArch 各自在架构 `early_scan_cpu_count()` 中筛选并注册 CPU。BSP 完成扫描后由固件物理 ID 反查逻辑 `CpuId`；AP 从架构入口取得物理 ID，同样只在 `ap_setup()` 中执行一次反查，然后进入逻辑 per-CPU 世界。

AP 唤醒遍历逻辑 CPU 集合，但 `hart_start`、LoongArch mailbox 和架构 IPI 只接收转换后的 `PhysCpuId`。IPI core 先用逻辑 ID 找目标 per-CPU queue，到真正触发硬件 IPI 时才转换。

### Scheduler stack

入口汇编继续用固件物理 ID 选择 `STACK0[physical_id]`。`remap_boot_stack()` 遍历注册的逻辑 CPU，通过映射取得其物理 ID，并为同一物理栈 backing 建立 guarded alias；`GUARDED_STACK_TOPS` 仍按物理槽位索引。

每个 CPU 的 `switch_to_guarded()` 是最后一次基于 ID 查询 scheduler stack。第一次 scheduler context switch 会把该 `sp` 保存进 per-CPU `sched_ctx`，以后任务 `switch_out()` 直接恢复该指针，不再读取栈表。

## 接受边界

本文被接受表示：

- 软件 CPU identity 默认是连续逻辑 `CpuId`；裸 `usize` 不能继续在内核层表示未分类 CPU 身份。
- 物理 ID 只能出现在 firmware entry、架构硬件接口和明确记录的 scheduler stack bootstrap 边界。
- `Vec<PhysCpuId>` 是逻辑到物理映射与 CPU 数量的唯一真相源；不得新增并列 count、反向表或 task/per-CPU 物理 ID 缓存。
- 反向查询是 O(CPU 数量) 的启动期操作，不能进入 scheduler、timer、IPI 或外部中断热路径。
- 入口 scheduler stack 仍要求固件物理 ID 可索引静态 `STACK0`；本 RFC 不增加汇编检查。

如果后续需要 CPU hotplug、任意高位稀疏 hart ID、动态 scheduler stack 或按 DT `interrupts-extended` 建立 PLIC context，必须更新本文或新建 follow-up RFC，不能在现有 registry 上叠加第二套状态。

## 备选方案

### 继续让 `CpuId` 表示硬件 ID

拒绝。per-CPU 数组和 scheduler placement 需要连续索引；用物理 ID 会把固件拓扑约束扩散到所有软件子系统。

### 同时保存逻辑到物理和物理到逻辑两张表

拒绝。AP 只在启动时反查一次，小 CPU 数量下线性扫描足够；第二张表会制造同步和生命周期上的并列真相源。

### 使用 lock-free append-only Vec

拒绝。CPU 注册只发生在单次 early scan，运行期只读；普通 `alloc::Vec` 加明确锁更符合当前需求，也避免引入专用依赖。

### 在入口汇编中先转换成逻辑 ID

延期。BSP 在解析 FDT 前没有 registry，AP 则需要额外传参或 trampoline。当前平台继续以物理槽位选择初始栈，逻辑转换放在 Rust 启动路径。

## 风险

- `PhysCpuId` 超出静态栈槽位会在进入 Rust 前破坏初始栈选择。这是当前启动协议的设计前提，不通过 Rust 断言伪装成可恢复错误。
- registry 反向扫描若扩散到运行期会形成不必要热路径成本。控制方式是 API 注释、调用点审计和 transaction 中的 `rg` 证据。
- PLIC context 仍采用临时公式，物理 ID 类型化不等于完整 DT context 解析。
- RISC-V 实机由用户确认运行通过；agent 未独立保存该次实机的原始运行日志。

## 收口

代码迁移、双架构构建和用户侧 RISC-V 实机验证已完成，详见事务日志。用户确认当前 RISC-V 实机能够按新 identity 模型启动和运行；agent 侧没有独立原始日志，因此事务记录保留证据来源。当前启动协议设计不覆盖超出静态 scheduler stack 槽位范围的物理 ID。
