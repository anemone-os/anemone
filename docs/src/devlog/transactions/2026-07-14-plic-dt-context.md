# 2026-07-14 - PLIC DT Context

**Status:** Completed
**Owners:** EDGW_, Codex
**Area:** RISC-V / PLIC / device tree / interrupt
**Canonical Plan:** [RFC-20260714-plic-dt-context](../../rfcs/plic-dt-context/index.md)
**Current Phase:** Closed

## Scope

将 PLIC 的临时 `physical_id * 2 + 1` context 公式替换为按
`interrupts-extended` 解析的 S-mode context 映射，覆盖初始化、enable、claim 和 complete。

## Implementation

- 按 parent phandle 解析目标 `riscv,cpu-intc` 节点。
- 从 parent 的 `#interrupt-cells` 动态计算每个 specifier 的长度。
- 按 entry 顺序保留具体 SiFive PLIC context index。
- 读取 CPU `reg` 得到 `PhysCpuId`，再按 CPU registry 逻辑顺序建立 `s_contexts`。
- 解析错误在返回 `SysError` 前由 `kerrln!` 记录；必要的 PLIC init 边界将错误转为启动 fail-fast。

## Validation

- `just build`（`visionfive2-rv64`, dev, kunit/fs_ext4/spin_lock_irqsave）：通过。
- `git diff --check`：通过。
- QEMU/实机运行：未由 agent 执行，待用户侧复验。

## Closure

PLIC 不再假设 hart/context 连续排列；JH7110 的 CPU0 M-only context 和非零物理 hart ID
可以由同一份 device tree 映射正确表达。后续只需保留用户侧运行证据，不再恢复固定 context 公式。
