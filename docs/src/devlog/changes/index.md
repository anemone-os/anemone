# 小迭代记录

小迭代记录保存不需要 RFC、但又比双周开发日志摘要更具体的修复、调查和局部实现事实。它不是单纯的事后流水账；当一个小问题需要先写清楚再动手时，小迭代记录也可以作为轻量方案记录使用。

它的职责是回答：

- 触发这次工作的症状、测试失败或观察是什么；
- 本轮要解决的局部问题是什么，为什么不需要 RFC；
- 选择的局部解决方案是什么，拒绝了哪些轻量替代方案；
- 本次实际改了什么，不改什么；
- 验证到什么程度；
- 还有哪些局部 tracking issues、风险、延期项或 register / current limitations 链接。

小迭代记录不是 backlog，也不是中大型设计草案。它可以在记录本体中包含 `Problem`、`Solution` 和 `Tracking Issues` 章节，让这次局部迭代自洽可读；但 tracking issues 只服务于当前记录，不承担仓库级 accepted contract、跨子系统不变量或长期阶段计划。未定稿的中大型方案应走私有草案或 RFC 工作流；跨多天、跨子系统、需要阶段 gate 或审计证据的实现应走事务日志。

## 命名与链接

- 文件放在 `docs/src/devlog/changes/`。
- 默认使用单文件：`YYYY-MM-DD-short-slug.md`。
- 如果需要背景材料，可以使用同名目录：`YYYY-MM-DD-short-slug/index.md`。
- 目录版记录可以包含 `backgrounds/`，用于保存证据摘要、Linux / LTP 对照、历史材料或运行记录。
- 双周开发日志保留一条短摘要，并在 `Related` 或 `Details` 中链接对应记录。
- register、current limitations、RFC 背景材料和事务日志可以按需链接小迭代记录。

## 单文件与目录边界

优先使用单文件，保持小迭代记录低摩擦。只有当单文件会变成难以扫读的证据包时，才升级为目录。

目录版记录仍以 `index.md` 为记录本体，回答 problem、scope、solution、change、validation、tracking issues、risk 和 links。`backgrounds/` 只保存事实材料，不定义计划、不变量、阶段 gate 或独立 review issue。

如果一个小迭代记录开始需要仓库级 accepted contract、非平凡不变量、跨阶段计划、独立 `tracking-issues.md`、多轮文档层 review 或多个 agent/checkpoint 编排，它应升级为 RFC 工作流，而不是继续扩张 `changes/` 目录。升级时，原小迭代记录保留为事实历史，并链接到新的 RFC 或事务日志。

## 当前记录

- [2026-06-23 - User-test LTP runner 结构拆分](./2026-06-23-user-test-ltp-structure-cleanup.md)
- [2026-06-22 - spin lock irqsave kconfig feature](./2026-06-22-spin-lock-irqsave-feature.md)
- [2026-06-18 - RFC workflow feedback loop](./2026-06-18-rfc-feedback-loop.md)
- [2026-06-17 - splice family copy-backed stage-1](./2026-06-17-splice-copy-stage1.md)
- [2026-06-15 - backend-aware fcntl pipe-size 分发](./2026-06-15-backend-aware-fcntl.md)
- [2026-06-14 - SysV shm credentials permission hook](./2026-06-14-sysv-shm-cred-permissions.md)
- [2026-06-14 - procfs sysctl PDE 静态树](./2026-06-14-procfs-sysctl-pde-tree.md)
- [2026-06-14 - waitid exited-child syscall bridge](./2026-06-14-waitid.md)
- [2026-06-14 - timerfd anonymous fd](./2026-06-14-timerfd.md)
- [2026-06-13 - eventfd2 anonymous fd](./2026-06-13-eventfd.md)
- [2026-06-13 - VFS stream file mode 边界清理](./2026-06-13-vfs-stream-file-mode.md)
- [2026-06-10 - FileOps status ctx 边界清理](./2026-06-10-fileops-status-ctx.md)
- [2026-06-09 - User-test staged 工具通道](./2026-06-09-user-test-staged-tools.md)
- [2026-06-08 - 空 iomux 超时睡眠修复](./2026-06-08-iomux-empty-timeout-sleep.md)
- [2026-06-08 - pselect6 exceptfds compat](./2026-06-08-pselect6-exceptfds-compat.md)
- [2026-06-07 - User-test LTP Pgrp Isolation](./2026-06-07-user-test-ltp-pgrp-isolation.md)
- [2026-06-07 - Signal LTP Tgkill Sigqueueinfo](./2026-06-07-signal-ltp-tgkill-sigqueueinfo.md)
- [2026-06-05 - Block Byte I/O Loop Mkfs](./2026-06-05-block-byte-io-loop-mkfs.md)
