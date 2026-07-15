# CPU Logical / Physical ID 不变量

**状态：** Canonical
**最后更新：** 2026-07-15
**父 RFC：** [RFC-20260714-cpu-logical-physical-id](./index.md)

## 闭合条件

1. 所有已注册 `CpuId` 构成连续区间 `0..ncpus()`。
2. registry 已初始化前缀的第 `i` 个元素是逻辑 `CpuId(i)` 对应的 `PhysCpuId`。
3. CPU 数量由 registry 的已初始化前缀长度定义，不存在独立可变 `NCPUS` 真相源。
4. registry 在 AP 启动前封存，封存后不再注册、删除或重排 CPU。
5. 软件 CPU owner 使用 `CpuId`；硬件调用只在窄边界消费 `PhysCpuId`。
6. 初始/scheduler stack 的物理 backing 与固件物理 ID 一一对应。
7. 第一次进入 scheduler 后，scheduler stack 由 per-CPU `sched_ctx.sp` 持有，不再依赖运行期 ID 查表。
8. `MAX_PHYS_CPU_ID` 是 platform-owned、含端点的物理 ID 上界；超出上界的 CPU 不进入 registry。
9. `MAX_LOGICAL_CPUS` 是 kconfig-owned 的最大启用逻辑 CPU 数；超限时 registry 包含 BSP 和按发现顺序排在前面的 `MAX_LOGICAL_CPUS - 1` 个 AP。
10. 所有固定 per-CPU 表的索引域由 `CpuTable` 或 `PhysCpuTable` 的类型边界表达，两种 table 的槽位都内建 `CachePadded<T>`。

任一条件不成立时，改造只能视为中间态，不能声明 CPU identity 边界闭合。

## 状态所有权

`device/cpu.rs` 的 CPU registry 是 CPU identity mapping 的唯一 owner：

- `physical_ids: CpuTable<MonoOnce<PhysCpuId>>` 的已初始化前缀保存逻辑到物理映射；
- 静态槽位下标定义逻辑 ID，`logical_cpu_count` 定义已初始化前缀长度；
- `registration_complete` 定义 registry 是否已经封存，并以 Release/Acquire 发布整个前缀；
- early scan 期间只有 BSP 写 registry；注册 API 的 safety contract 禁止并发 writer，封存后所有调用者只读；
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
- 只有 `0..=MAX_PHYS_CPU_ID` 能索引 platform 的物理 per-CPU backing；扫描期越界 ID 必须 warning 并跳过。
- 可用于 SBI hart、LoongArch core/mailbox/IPI、PLIC DT context 解析中的 hart identity、架构 timer ID 和 bootstrap stack 槽位。
- 不得用于 `PERCPU_BASES`、task placement、runqueue、IPI queue 或 procfs 逻辑编号。

### 转换

- `CpuId -> PhysCpuId` 是 registry O(1) 查询。
- `PhysCpuId -> CpuId` 是 O(CPU 数量) 线性扫描，只允许在 BSP/AP bootstrap 中使用。
- 不建立反向 HashMap、数组或 per-CPU 物理 ID 缓存；若未来热路径确实需要反向转换，必须先回到 RFC review。

## 注册与封存

1. 架构 early scan 读取物理 ID 后先校验 `PhysCpuId <= MAX_PHYS_CPU_ID`；越界节点立即 `kwarningln!` 并跳过。
2. 只有通过物理上界和架构可用性检查的 CPU 才调用 `register_cpu()`，且调用者必须是唯一 BSP writer。
3. `register_cpu()` 检查 registry 尚未封存、物理 ID 位于 platform 上界内且未重复；非 BSP 最多接纳 `MAX_LOGICAL_CPUS - 1` 个，BSP 无论设备树顺序都保留。
4. 被逻辑容量规则拒绝的 AP 不初始化槽位；完成扫描后只打印一次 `kwarningln!`，说明逻辑上限、BSP 保留策略和忽略数量。
5. 新槽位必须先完成 `MonoOnce<PhysCpuId>` 初始化，再以 Relaxed 写推进 `logical_cpu_count`；只有 BSP 单写者可以依赖这一顺序。
6. `finish_cpu_registration()` 确认 BSP 已注册，再以 Release `compare_exchange` 发布封存状态；状态变更不得写进断言。
7. `cpu_count()`、正向映射和反向映射先以 Acquire 确认封存，再读取逻辑计数和槽位。
8. 物理 ID 只与 `MAX_PHYS_CPU_ID` 比较，逻辑数量只与 `MAX_LOGICAL_CPUS` 比较；两个边界不能互换。
9. 当前不支持 registry reopen、第二 writer 或 CPU hotplug。

## Per-CPU 表

- `CpuTable<T, const N: usize = MAX_LOGICAL_CPUS>` 只接受 `CpuId`，默认容量为 `MAX_LOGICAL_CPUS`；registry 和 `PERCPU_BASES` 属于该域。
- `PhysCpuTable<T, const N: usize = { MAX_PHYS_CPU_ID + 1 }>` 只接受 `PhysCpuId`，默认容量覆盖含端点的物理 ID 空间；`STACK0` 和 `GUARDED_STACK_TOPS` 属于该域。
- 两种 table 的 backing 都是 `[CachePadded<T>; N]`，索引返回 `T`；都允许调用者显式覆盖 `N`，但索引身份不随容量改变；构造、`Index` 和 `IndexMut` 必须 `inline(always)`。
- `PhysCpuTable` 必须保持 transparent layout；两架构必须编译期证明 `RawKernelStack` 与汇编栈步长相同，且 `CachePadded<RawKernelStack>` 不改变元素大小。

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
2. 平台必须保证 BSP 物理 ID 不超过 `MAX_PHYS_CPU_ID`；AP 越界 ID由 early scan 过滤。
3. `remap_boot_stack()` 通过逻辑 CPU 枚举 registry，但用每个 CPU 的物理 ID 选择原始 stack backing 和 guarded-top 槽位。
4. `switch_to_guarded()` 每 CPU 只执行一次物理槽位查询。
5. 第一次 `switch_to()` 保存 scheduler context 后，后续 `switch_out()` 必须直接恢复 `sched_ctx.sp`。
6. `STACK0` backing 在 stage 2 后仍是 scheduler stack，不能释放或复用。

## 禁止退化项

- 重新让裸 `usize` 同时承担逻辑和物理 CPU 身份。
- 用物理 ID 索引 `PERCPU_BASES`、runqueue、task placement 或 IPI queue。
- 用逻辑 ID调用 SBI、LoongArch IPI/mailbox 或 PLIC hardware context。
- 维护第二份 CPU count 或反向映射作为行为真相源。
- 用 `MAX_LOGICAL_CPUS` 校验 `PhysCpuId`，用 `MAX_PHYS_CPU_ID` 限制逻辑 CPU 数，或在逻辑超限时把 BSP 排除在拓扑之外。
- 用裸数组重新承载固定 per-CPU backing，使调用点能以 `usize` 或错误的 CPU ID 类型索引。
- 在 `registration_complete` 发布后再次初始化槽位，或绕过 Acquire 检查读取未封存前缀。
- 在 scheduler、timer 或 IRQ 热路径调用 `from_physical_id()`。
- 进入 stage 2 后释放 `STACK0` backing，或每次调度重新按 ID 查 scheduler stack。
- 为通过单个平台构建而静默把无效物理 ID压缩成逻辑 ID。

## 完成标准

- 源码搜索确认反向映射只存在于 BSP/AP bootstrap。
- 两架构构建通过，CPU-facing API 不再接受含义不明的 `usize`。
- 用户侧 RISC-V 实机证明当前目标平台能在新 identity 模型下启动和运行。
- 文档必须区分用户确认的实机结果与 agent 保存的原始运行证据；本轮只有前者。
