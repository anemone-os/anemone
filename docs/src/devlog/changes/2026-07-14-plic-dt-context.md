# ANE-CHG-20260714-plic-dt-context

**Type:** Small Feature / Architecture Compatibility
**Status:** Completed
**Date:** 2026-07-14
**Authors:** EDGW_, Codex
**Area:** RISC-V / PLIC / device tree / interrupt

## Problem

PLIC 的 `interrupts-extended` entry 由 parent phandle 和 parent 的
`#interrupt-cells` 决定，不能用固定的两 cell 或 `physical_id * 2 + 1` 推导
context。JH7110 的 CPU0 只提供 M-mode entry，固定公式会让后续 S-mode context 索引
错位。

这个问题只涉及 PLIC device-tree 解析和同一设备 owner 内的运行时 context 映射，不改变
跨子系统 shared contract，也不需要分阶段迁移或独立的不变量证明，因此按小迭代记录，
不建立 RFC 或事务日志。

## Scope

本轮只调整 RISC-V PLIC context 的发现和消费路径：

- 按 `interrupts-extended` 的 parent `#interrupt-cells` 动态推进 entry 边界；
- 解析 CPU interrupt-controller 的 phandle 到物理 hart ID；
- 为每个已注册逻辑 CPU 找到唯一的 S-mode external context；
- 让初始化、mask/unmask、claim 和 complete 消费同一份启动期映射；
- 对 malformed device tree 返回可观察的 `SysError`，并由必要的 PLIC 初始化边界
  fail-fast。

本轮不改变 PLIC hwirq 的 device interrupt specifier 语义，不实现 CPU hotplug 或运行期
device tree 变更，也不把 M-mode、VS-mode 或不存在的 context 暴露给 S-mode IRQ runtime。

## Solution

`parse_s_contexts` 读取 PLIC 的原始 `interrupts-extended` 属性。每个 entry 先读取 parent
phandle，再从目标节点读取 `#interrupt-cells`，按该宽度取得 specifier。PLIC binding
要求 parent 是 `riscv,cpu-intc`；当前 binding 的单一 cause cell 中，值 `9` 表示 S-mode
external interrupt。entry 序号保留为具体 SiFive PLIC 的 context index，解析出的
`(PhysCpuId, context)` 再按 CPU registry 顺序压成逻辑 CPU 索引的向量。

Device tree 是 context 拓扑的唯一来源，`s_contexts[CpuId::logical_id()]` 是 PLIC
运行时 context 的唯一行为映射。所有 active CPU 必须有唯一 S-mode context，否则初始化
失败。解析失败在返回 `SysError` 前通过 `kerrln!` 记录具体上下文；PLIC 是必要的早期根
中断控制器，初始化边界不做静默降级。

## Change

- 按 parent phandle 解析目标 `riscv,cpu-intc` 节点，并从 parent 的
  `#interrupt-cells` 动态计算 specifier 长度；
- 按 entry 顺序保留硬件 context index，只收集 cause `9` 的 S-mode external context；
- 读取 CPU `reg` 得到 `PhysCpuId`，再按 CPU registry 的逻辑顺序建立 `s_contexts`；
- 对缺失 property、未知 phandle、截断 entry、错误 cell 宽度、坏 CPU reg、active CPU
  context 缺失或重复返回 `SysError` 并记录诊断；
- 初始化、mask/unmask、claim 和 complete 共用 `s_contexts`。

## Validation

Agent-run validation:

- `just build`（`visionfive2-rv64`, dev, kunit/fs_ext4/spin_lock_irqsave）通过；仅保留既有
  SBI legacy console deprecation warning；
- `git diff --check` 通过；
- source audit 确认初始化、mask/unmask、claim 和 complete 共用启动期映射。

Agent 未运行 QEMU 或实机验证；用户侧仍需复验
`plic: physical CPU -> S-mode context` 日志和原告警是否消失。

## Tracking Issues

### CHG-001 - VisionFive 2 runtime validation

**Status:** Deferred
**Severity:** Euclid

**Issue:** 构建和源码路径已经闭合，但尚未以 VisionFive 2 实机运行证据确认 context 映射
与 claim 路径收敛。

**Resolution:** 保留为本小迭代的验证缺口，不新增 register 条目。用户侧复验应检查每个
active CPU 的 S-mode context 映射、原告警是否消失以及外部中断 claim/complete 是否正常；
若运行时 DT 与构建时 DTS 的 phandle/context 拓扑不一致，先记录设备树证据，再重新归因。

## Risk / Follow-up

- 设备树缺少 active CPU 的 S-mode context 会让启动 fail-fast；这是必要的根 IRQ 能力，
  不能通过恢复物理 ID 公式、屏蔽 `sext` 或降低日志级别静默绕过。
- 当前 `SysError` 只承载错误类别，具体 phandle、offset、hart 和 cell 信息由 `kerrln!`
  保留。
- 当前没有仍然生效的缺陷或已接受能力限制需要写入 register；未运行的实机证据保留在本
  记录的 Tracking Issues 中。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- Register / limitations: None.
- Related RFC: [CPU Logical / Physical ID](../../rfcs/cpu-logical-physical-id/index.md)
