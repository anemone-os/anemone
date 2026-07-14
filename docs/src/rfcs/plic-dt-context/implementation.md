# PLIC DT Context 实施计划

**状态：** Completed
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260714-plic-dt-context](./index.md)

## 阶段 1：按规范解析 entry

**状态：** Completed

- 读取 `interrupts-extended` 原始 property。
- 按 parent phandle 的 `#interrupt-cells` 动态确定 specifier 长度。
- 校验 parent 是 `riscv,cpu-intc`，读取其父 CPU 节点的 `reg` hart ID。
- 对缺失 property、未知 phandle、截断 entry、错误 cell 宽度和坏 CPU reg 返回 `SysError`，并在每个失败点记录 `kerrln!`。

## 阶段 2：建立运行时映射

**状态：** Completed

- 只收集 cause `9` 的 S-mode external context。
- 保留 property entry 序号作为 SiFive PLIC context index。
- 按 CPU registry 的逻辑顺序生成 `s_contexts`，禁止 active CPU 缺失或重复 context。
- 初始化、mask/unmask、claim、complete 共用 `s_contexts`。

## 验证

- `just build`：通过，保留既有 SBI legacy console deprecation warning。
- `git diff --check`：通过。
- QEMU/实机运行：本轮未由 agent 执行；需用户侧复验 `plic: physical CPU -> S-mode context` 日志和原告警是否消失。

## 停止条件

- 若运行时 DT 与构建时 DTS 的 phandle/context 拓扑不一致，停止扩大实现并先记录设备树证据。
- 不通过屏蔽 `sext`、降低日志级别或恢复物理 ID 公式来绕过 claim 失败。
