# CPU Logical / Physical ID 不变量

**状态：** Canonical
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260714-cpu-logical-physical-id](./index.md)

## 闭合条件

1. 所有已注册 `CpuId` 构成连续区间 `0..ncpus()`。
2. registry Vec 的第 `i` 个元素是逻辑 `CpuId(i)` 对应的 `PhysCpuId`。
3. CPU 数量从 registry 长度派生，不存在独立可变 `NCPUS` 真相源。
4. registry 在 AP 启动前封存，封存后不再注册、删除或重排 CPU。
5. 软件 CPU owner 使用 `CpuId`；硬件调用只在窄边界消费 `PhysCpuId`。
6. 初始/scheduler stack 的物理 backing 与固件物理 ID 一一对应。
7. 第一次进入 scheduler 后，scheduler stack 由 per-CPU `sched_ctx.sp` 持有，不再依赖运行期 ID 查表。

任一条件不成立时，改造只能视为中间态，不能声明 CPU identity 边界闭合。

## 状态所有权

`device/cpu.rs` 的 CPU registry 是 CPU identity mapping 的唯一 owner：

- `physical_ids: Vec<PhysCpuId>` 保存逻辑到物理映射；
- Vec 下标定义逻辑 ID；
- `registration_complete` 定义 registry 是否已经封存；
- `NoIrqRwLock` 保护 Vec 访问，阶段字段不能替代锁；
- `percpu.rs`、scheduler、task 和 driver 只能消费 `CpuId` 或查询映射，不能复制 registry 状态。

`BSP_CPU_ID` 是启动角色的稳定快照，不是第二套映射。它保存 BSP 的逻辑 `CpuId`，只服务 `bsp_cpu_id()` 比较。

## 身份模型

### `CpuId`

- 表示内核逻辑 CPU 身份。
- 可用于 per-CPU 数组索引、任务放置、scheduler 比较、CPU allowed mask 和 procfs processor 字段。
- `Display` / `Debug` 输出逻辑身份；普通日志直接格式化 `CpuId`，不手工打印 `logical_id()`。
- 只有确实需要数值索引或 ABI 整数时才调用 `logical_id()`。

### `PhysCpuId`

- 表示固件或硬件可见 CPU 身份。
- 可用于 SBI hart、LoongArch core/mailbox/IPI、当前临时 PLIC context 公式、架构 timer ID 和 bootstrap stack 槽位。
- 不得用于 `PERCPU_BASES`、task placement、runqueue、IPI queue 或 procfs 逻辑编号。

### 转换

- `CpuId -> PhysCpuId` 是 registry O(1) 查询。
- `PhysCpuId -> CpuId` 是 O(CPU 数量) 线性扫描，只允许在 BSP/AP bootstrap 中使用。
- 不建立反向 HashMap、数组或 per-CPU 物理 ID 缓存；若未来热路径确实需要反向转换，必须先回到 RFC review。

## 注册与封存

1. 架构 early scan 只为通过可用性检查的 CPU 调用 `register_cpu()`。
2. `register_cpu()` 在写锁内检查 registry 尚未封存、物理 ID 未重复、逻辑 CPU 数量未超过 `MAX_CPUS`，随后追加 Vec。
3. 状态变更和资源操作必须先执行，再对结果做断言；不得把 `push`、`compare_exchange`、注册或发送操作写进 `assert!`。
4. `finish_cpu_registration()` 在写锁事务内确认 registry 非空并发布封存状态。
5. `cpu_count()`、正向映射和反向映射只能在封存后读取。
6. 当前不支持 registry reopen 或 CPU hotplug。

## 软件与硬件边界

必须使用逻辑 `CpuId`：

- `CoreLocal.cpu_id`、`BSP_CPU_ID` 和 `PERCPU_BASES`；
- task `cpuid`、kthread placement、timer worker slot；
- IPI per-CPU message queue、target-online 检查和 scheduler remote enqueue；
- `/proc/<pid>/stat` processor 与 CPU allowed mask/list。

必须转换为 `PhysCpuId`：

- RISC-V `hart_start` 和 SBI hart mask；
- LoongArch mailbox 与 IPI target；
- PLIC hardware context 计算；
- LoongArch timer/TID 硬件参数；
- `STACK0` 和 `GUARDED_STACK_TOPS` 槽位。

`IntrArchTrait::send_ipi(PhysCpuId)` 是 IPI core 到架构硬件的类型边界。IPI core 在转换前必须先按逻辑 ID 发布目标 per-CPU queue。

## Scheduler stack 生命周期

1. 入口汇编在 registry 建立前使用固件物理 ID 选择 `STACK0`；该汇编保持不变。
2. 平台必须保证启动 CPU 的物理 ID 可索引静态 `STACK0`。
3. `remap_boot_stack()` 通过逻辑 CPU 枚举 registry，但用每个 CPU 的物理 ID 选择原始 stack backing 和 guarded-top 槽位。
4. `switch_to_guarded()` 每 CPU 只执行一次物理槽位查询。
5. 第一次 `switch_to()` 保存 scheduler context 后，后续 `switch_out()` 必须直接恢复 `sched_ctx.sp`。
6. `STACK0` backing 在 stage 2 后仍是 scheduler stack，不能释放或复用。

## 禁止退化项

- 重新让裸 `usize` 同时承担逻辑和物理 CPU 身份。
- 用物理 ID 索引 `PERCPU_BASES`、runqueue、task placement 或 IPI queue。
- 用逻辑 ID调用 SBI、LoongArch IPI/mailbox 或 PLIC hardware context。
- 维护第二份 CPU count 或反向映射作为行为真相源。
- 在 scheduler、timer 或 IRQ 热路径调用 `from_physical_id()`。
- 进入 stage 2 后释放 `STACK0` backing，或每次调度重新按 ID 查 scheduler stack。
- 为通过单个平台构建而静默把无效物理 ID压缩成逻辑 ID。

## 完成标准

- 源码搜索确认反向映射只存在于 BSP/AP bootstrap。
- 两架构构建通过，CPU-facing API 不再接受含义不明的 `usize`。
- 用户侧 RISC-V 实机证明当前目标平台能在新 identity 模型下启动和运行。
- 文档必须区分用户确认的实机结果与 agent 保存的原始运行证据；本轮只有前者。
