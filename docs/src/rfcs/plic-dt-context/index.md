# RFC-20260714-plic-dt-context

**状态：** Implemented / Closed
**负责人：** EDGW_, Codex
**最后更新：** 2026-07-14
**领域：** RISC-V / PLIC / device tree / interrupt
**事务日志：** [PLIC DT Context 事务日志](../../devlog/transactions/2026-07-14-plic-dt-context.md)
**开放问题：** None
**下一步：** 用户侧 VisionFive 2 实机复验中记录 context 映射与告警收敛结果。

## 摘要

PLIC 的 `interrupts-extended` entry 由 parent phandle 和 parent 的
`#interrupt-cells` 决定，不能用固定的两 cell 或 `physical_id * 2 + 1` 推导
context。JH7110 的 CPU0 只提供 M-mode entry，导致后续 S-mode context 索引错位。

本 RFC 让 PLIC 在启动时按 device tree 顺序解析 entry，解析每个 parent 的
`#interrupt-cells`，筛选 RISC-V S-mode external interrupt，并按已注册逻辑 CPU
保存对应的硬件 context。初始化、mask/unmask、claim 和 complete 共享同一份映射。

## 目标

- 按 `interrupts-extended` 的 parent `#interrupt-cells` 动态推进 entry 边界。
- 解析 CPU interrupt-controller 的 phandle 到物理 hart ID。
- 为每个已注册 CPU 找到唯一的 S-mode external context。
- 让 PLIC 运行时路径只消费启动时建立的 immutable context snapshot。
- 对 malformed device tree 返回可观察的 `SysError`，由必要的系统初始化边界 fail-fast。

## 非目标

- 不改变 PLIC hwirq 的 device interrupt specifier 语义。
- 不实现 CPU hotplug 或运行期 device tree 变更。
- 不把 M-mode、VS-mode 或不存在的 context 暴露给 S-mode IRQ runtime。

## 方案

`parse_s_contexts` 读取 PLIC 的原始 `interrupts-extended` 属性。每个 entry 先读取
parent phandle，再从目标节点读取 `#interrupt-cells`，按该宽度取得 specifier。
PLIC binding 要求 parent 是 `riscv,cpu-intc`；当前 binding 的单一 cause cell 中，
值 `9` 表示 S-mode external interrupt。entry 的序号保留为具体 SiFive PLIC 的
context index，解析出的 `(PhysCpuId, context)` 再按 CPU registry 顺序压成逻辑 CPU
索引的向量。

解析失败返回 `SysError` 并在失败点写入 `kerrln!`；PLIC 是必要的早期根中断控制器，
`CoreIrqChip::init` 在收到错误后直接报告并停止启动。

## 接受边界

- Device tree 是 context 拓扑的唯一来源；不再按物理 hart ID 推导 PLIC context。
- `s_contexts[CpuId::logical_id()]` 是 PLIC 对运行时 context 的唯一行为映射。
- 所有 active CPU 必须有唯一 S-mode context，否则初始化失败。
- agent 侧验证 floor 是 `just build`、`git diff --check` 和 source audit；实机告警收敛由用户侧复验。

## 风险

- 设备树缺少 active CPU 的 S-mode context 会让启动 fail-fast；这是必要的根 IRQ 能力，不能静默降级。
- 当前 `SysError` 只承载错误类别，具体 phandle、offset、hart 和 cell 信息通过 `kerrln!` 保留。
- 尚未在本轮运行 QEMU/实机验证 context 日志；构建验证不证明硬件 claim 已收敛。

## 收口

实现和构建证据记录在 [PLIC DT Context 事务日志](../../devlog/transactions/2026-07-14-plic-dt-context.md)。
