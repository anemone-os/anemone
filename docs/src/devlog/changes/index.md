# 小迭代记录

小迭代记录保存不需要 RFC、但又值得长期追溯的局部决策、修复、调查和实现事实。Patch 默认不写正式记录；只有代码、测试和 Git 无法低成本恢复本轮判断时，才升级为小迭代。

它的职责是回答：

- 触发这次工作的症状、测试失败或观察是什么；
- 本轮要解决的局部问题和选择的处理方式；
- 本次实际改了什么，不改什么；
- 验证到什么程度；
- 还有哪些风险、延期项、架构摩擦或 register / current limitations 链接。

小迭代记录不是 backlog，也不是小型 RFC。默认正文只需要 `Problem / Context`、`Decision`、`Change`、`Validation` 和 `Remaining Risk / Links`；`Checkpoints`、`Tracking Issues`、`Architecture Friction`、`Contract Impact / Cutover` 和背景材料只在确有内容时增加。默认使用一个 closure checkpoint；确需 review、commit 或授权停止点时，可以在同一个已完整解析的 Implementation Boundary 内使用至多两个 execution checkpoint。CKPT 1 必须独立安全且不改变受保护 visible semantics/current contract，CKPT 2 完成整个 target 并承担至多一次最终 cutover；effective 正文仍只位于 current contract。未决 owner、ABI、shared contract、生命周期/并发协议、probe、transitional contract、跨 checkpoint 重新解析边界、多个独立 cutover、超过两个正式 execution checkpoint 或 target renegotiation 应升级 RFC。

## 命名与链接

- 文件放在 `docs/src/devlog/changes/`。
- 默认使用单文件：`YYYY-MM-DD-short-slug.md`。
- 如果需要背景材料，可以使用同名目录：`YYYY-MM-DD-short-slug/index.md`。
- 目录版记录可以包含 `backgrounds/`，用于保存证据摘要、Linux / LTP 对照、历史材料或运行记录。
- 将新记录加入本页和必要的 mdBook 导航；不要求再写双周日志摘要。
- register、current limitations、RFC 背景材料和事务日志可以按需链接小迭代记录。

## 单文件与目录边界

优先使用单文件，保持小迭代记录低摩擦。只有当单文件会变成难以扫读的证据包时，才升级为目录。

目录版记录仍以 `index.md` 为记录本体。`backgrounds/` 只保存事实材料，不定义计划、不变量、阶段 gate 或独立 review issue。

至多两个 execution checkpoint 仍属于同一份记录，不拆 `implementation.md` 或 transaction。除此之外，如果一个小迭代开始需要仓库级 accepted target、非平凡不变量、跨阶段计划、probe、多个独立 cutover、target renegotiation 或无法在本轮关闭的 Apollyon/Keter，它应升级 RFC，而不是继续扩张 `changes/` 目录。升级时，原记录保留为事实历史并链接新的 RFC；transaction 仍按需创建。

## 当前记录

- [2026-08-07 - TCP local reverse TIME_WAIT admission](./2026-08-07-tcp-local-time-wait-admission.md)
- [2026-08-07 - Ext4 single Mutex](./2026-08-07-ext4-single-mutex.md)
- [2026-08-07 - Ext4 synchronous I/O batching](./2026-08-07-ext4-synchronous-io-batching.md)
- [2026-08-07 - ELF demand paging](./2026-08-07-elf-demand-paging.md)
- [2026-08-06 - IPv4 UDP ICMP extended error](./2026-08-06-ipv4-udp-icmp-extended-error.md)
- [2026-08-06 - Inode address space](./2026-08-06-inode-address-space.md)
- [2026-08-06 - User-access typed-copy soundness](./2026-08-06-user-access-typed-copy-soundness.md)
- [2026-08-05 - Kernel performance observation](./2026-08-05-kernel-performance-observation.md)
- [2026-08-05 - Kworker](./2026-08-05-kworker.md)
- [2026-08-05 - OOM periodic sampling](./2026-08-05-oom-periodic-sampling.md)
- [2026-08-05 - brk shrink backing decommit](./2026-08-05-brk-shrink-decommit.md)
- [2026-08-04 - Unix seqpacket](./2026-08-04-unix-seqpacket.md)
- [2026-08-03 - Serial TTY RX conditioning](./2026-08-03-tty-serial-rx-conditioning.md)
- [2026-08-03 - Named FIFO](./2026-08-03-named-fifo.md)
- [2026-08-03 - App Command build driver](./2026-08-03-app-command-driver.md)
- [2026-08-03 - Pipe dynamic capacity](./2026-08-03-pipe-dynamic-capacity.md)
- [2026-08-02 - Kernel backtrace symbolization](./2026-08-02-kernel-symbolization.md)
- [2026-08-02 - KUnit execution boundary](./2026-08-02-kunit-execution-boundary.md)
- [2026-08-02 - Kernel developer logging](./2026-08-02-kernel-developer-logging.md)
- [2026-08-02 - lwext4 Rust safe facade](./2026-08-02-lwext4-rust-safe-facade.md)
- [2026-08-02 - Pipe Event wait](./2026-08-02-pipe-event-wait.md)
- [2026-08-02 - rlimit core 与 RLIMIT_NOFILE](./2026-08-02-rlimit-core-nofile.md)
- [2026-08-02 - VFS/kernel creation boundary](./2026-08-02-vfs-kernel-creation-boundary.md)
- [2026-08-02 - RISC-V architecture syscalls](./2026-08-02-riscv-arch-syscalls.md)
- [2026-08-01 - Alpha/Omega POSIX lock/VFS integration](./2026-08-01-alpha-omega-posix-lock-vfs-integration.md)
- [2026-07-31 - Network host test organization](./2026-07-31-net-host-test-organization.md)
- [2026-07-31 - Alpha/Omega Flock/UDP integration](./2026-07-31-alpha-omega-flock-udp-integration.md)
- [2026-07-31 - Anonymous inode UAPI kind](./2026-07-31-anon-inode-uapi-kind.md)
- [2026-07-31 - FS owner-local syscall API](./2026-07-31-fs-owner-local-syscall-api.md)
- [2026-07-31 - Net UDP external peer retirement](./2026-07-31-net-udp-external-peer-retirement.md)
- [2026-07-31 - IRQ flow protocol](./2026-07-31-irq-flow-protocol.md)
- [2026-07-29 - Minimal global membarrier](./2026-07-29-minimal-global-membarrier.md)
- [2026-07-27 - umask 文件创建掩码](./2026-07-27-umask-file-creation-mask.md)
- [2026-07-25 - Asynchronous wake delivery](./2026-07-25-asynchronous-wake-delivery.md)
- [2026-07-24 - QEMU SMP Platform用途别名](./2026-07-24-qemu-smp-platform-aliases.md)
- [2026-07-24 - mount fstype/source compatibility](./2026-07-24-mount-fstype-source-compat.md)
- [2026-07-24 - FIONBIO opened-description status 更新](./2026-07-24-fionbio.md)
- [2026-07-24 - github/main PR #136 集成](./2026-07-24-github-main-pr136-integration.md)
- [2026-07-24 - xref 外部源码注册表](./2026-07-24-xref-source-registry.md)
- [2026-07-24 - alpha/omega RFC 结果合流](./2026-07-24-alpha-omega-integration.md)
- [2026-07-22 - RFC rolling stage resolution 与 target renegotiation](./2026-07-22-rfc-stage-resolution-renegotiation.md)
- [2026-07-22 - device devnum ownership](./2026-07-22-device-devnum-ownership.md)
- [2026-07-16 - Current contract layer](./2026-07-16-current-contract-layer.md)
- [2026-07-14 - RFC semantic revision workflow](./2026-07-14-rfc-semantic-revisions.md)
- [2026-07-14 - PLIC device-tree context 解析](./2026-07-14-plic-dt-context.md)
- [2026-07-13 - RISC-V Svade A/D 位兼容](./2026-07-13-riscv-ad-bits-svade.md)
- [2026-07-05 - read-write request 结构整理](./2026-07-05-read-write-request-structure.md)
- [2026-06-23 - User-test LTP judge 输出兼容](./2026-06-23-user-test-ltp-judge-compat.md)
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
