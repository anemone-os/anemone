# rv64 LTP ioctl 运行证据

本目录保存 2026-06-04 rv64 `ioctl` 组运行证据和对应调查报告。它是 [RFC-20260603-IOCTL-LOOP](../../index.md) 的背景材料，用于支撑后续 ioctl / loop / block / random 兼容性修复的失败归类。

材料：

- [原始 user-test-rv64.log](./user-test-rv64.log)
- [失败调查报告](./investigation.md)

边界：

- 该日志是一次 rv64 user-test 运行结果，不代表所有 ioctl 组失败都已经进入目标 syscall 语义。
- 调查报告把 prerequisite / rootfs / device visibility / kernel semantic gap 分开归类，后续实现和 devlog 应引用本目录内的公开材料，不再引用个人开发环境中的 `etc/` 草稿路径。
