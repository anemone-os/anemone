# CPU Logical / Physical ID 迁移实施计划

**状态：** Completed
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260714-cpu-logical-physical-id](./index.md)
**不变量：** [CPU Logical / Physical ID 不变量](./invariants.md)

## 迁移原则

- 先建立 identity owner，再迁移调用点，避免用重命名掩盖逻辑/物理混用。
- 软件内部默认传 `CpuId`，只在最终硬件边界转换成 `PhysCpuId`。
- scheduler stack 保留物理槽位，不把固件前置条件伪装成逻辑数组语义。
- registry 只支持 early registration 和永久封存，不提前设计 hotplug。
- 实现事实写事务日志；若硬件证据推翻 accepted boundary，先回写 RFC 再改代码。

## 阶段 1：Registry 与架构发现

**状态：** Completed

交付：

- 在 `device/cpu.rs` 定义 `CpuId`、`PhysCpuId` 和锁保护的 `Vec<PhysCpuId>` registry。
- `register_cpu()` 以 Vec 下标分配逻辑 ID，`finish_cpu_registration()` 封存拓扑。
- RISC-V 与 LoongArch 各自的 `early_scan_cpu_count()` 注册通过架构检查的 CPU。
- 从 `open_firmware` 删除通用 CPU count scanner。

审计：

- `boxcar` 依赖完全移除。
- registry mutation 不写入断言。
- `from_physical_id()` 注释明确 O(CPU 数量) 和 bootstrap-only 边界。

退出条件：registry 是 CPU count 和 mapping 的唯一 owner，AP 尚未启动时已完成封存。

## 阶段 2：逻辑 CPU owner 迁移

**状态：** Completed

交付：

- `CoreLocal`、`BSP_CPU_ID`、per-CPU remote access 和 `PERCPU_BASES` 改用 `CpuId`。
- 删除独立 `NCPUS`，`ncpus()` 从 registry 派生。
- scheduler remote enqueue、IPI core、kthread placement、threaded timer worker 和 procfs processor 使用逻辑 ID。
- 普通诊断直接格式化 `CpuId`；索引和 ABI 数值才使用 `logical_id()`。

退出条件：软件 owner 不再接收含义不明的 CPU `usize`。

## 阶段 3：硬件边界类型化

**状态：** Completed

交付：

- `IntrArchTrait::send_ipi()` 改为接收 `PhysCpuId`。
- RISC-V `hart_start` 与 SBI hart mask 使用物理 ID，单 hart mask 使用 `from_mask_base(1, physical_id)`。
- LoongArch mailbox/IPI 与 timer ID 使用物理 ID。
- PLIC 临时 context 公式使用物理 ID；不扩展 `interrupts-extended` 解析。

退出条件：硬件 API 不能从类型上误收逻辑 `CpuId`。

## 阶段 4：Scheduler stack 物理槽位

**状态：** Completed

交付：

- 两架构入口汇编保持物理 ID 索引 `STACK0`，不增加检查。
- `remap_boot_stack()` 只映射已注册 CPU，但 backing 和 guarded top 均按物理槽位归属。
- `switch_to_guarded()` 注释说明这是最后一次 ID-based scheduler stack lookup。

退出条件：第一次 scheduler switch 后只通过 `sched_ctx.sp` 恢复 scheduler stack。

## 阶段 5：验证与收口

**状态：** Completed

已完成：

- `visionfive2-rv64` 配置下 `just build` 通过。
- 临时切换 `qemu-virt-la64` 后 `just build` 通过，随后恢复 `visionfive2-rv64`。
- `git diff --check` 通过。
- `just fmt kernel --check` 未全仓通过，但输出只包含本轮 write set 外既有差异；本轮文件无 formatter diff。
- 源码审计确认 `from_physical_id()` 只在两架构 BSP/AP setup 中调用，`GUARDED_STACK_TOPS` 只在两架构 `switch_to_guarded()` 中读取。

用户侧完成：

- 用户确认当前 RISC-V 实机在本改造后启动和运行通过。
- 该证据由用户提供；agent 未独立保存实机原始日志，不能据此扩大到未测试平台或高位稀疏物理 ID。

停止条件：

- BSP 物理 ID无法在 registry 中反查；
- AP 使用错误的逻辑 per-CPU base；
- hardware IPI/PLIC 仍消费逻辑 ID；
- scheduler stack 在 guarded 切换后出现物理槽位错配。

后续若出现任一失败，必须重新打开事务日志并回到 RFC review，不能用重新假设物理 ID 连续来绕过。

## 旁路审计清单

- 搜索 `cur_cpu_id().get()`、`cpuid().get()` 和 CPU-facing `usize` 参数。
- 搜索所有 `send_ipi`、`hart_start`、mailbox、PLIC context 和 timer ID 调用。
- 搜索 `PERCPU_BASES`、`STACK0`、`GUARDED_STACK_TOPS` 的索引来源。
- 搜索 `from_physical_id()`，只允许 BSP/AP bootstrap 调用点。
- 搜索打印路径中的 `logical_id()`，普通逻辑 ID 日志应直接格式化 `CpuId`。

## 实现期反馈记录

- 2026-07-14：用户要求使用 `alloc::Vec`，拒绝 `boxcar`；registry 改为普通 Vec 并移除依赖。
- 2026-07-14：物理类型由含义过宽的 `PhysicalId` 改名为 `PhysCpuId`。
- 2026-07-14：用户要求 Vec 显式上锁，移除 `UnsafeCell` 方案；反向查询补充全 CPU 线性扫描成本注释。
- 2026-07-14：用户要求副作用与断言分离；registry publish、CPU 查询和范围检查均先求值再断言。
- 2026-07-14：用户要求日志直接格式化逻辑 `CpuId`；保留 `logical_id()` 仅用于索引、范围判断、ABI 和 procfs 数值。

以上反馈均保持 RFC 的逻辑/物理身份目标，只收紧实现形状，没有削弱不变量。

## Write Set

实现涉及：CPU registry、两架构 CPU discovery/bootstrap/IPI、per-CPU、IPI core、scheduler/kthread/timer、PLIC、procfs 和依赖清理。未改变 syscall ABI、CPU hotplug、PLIC DT 解析或入口汇编协议。
