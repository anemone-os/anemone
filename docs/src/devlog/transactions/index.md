# 事务日志

事务日志记录跨多天、跨子系统、需要阶段性审计证据的大型重构或迁移。

日常开发日志仍按双周追加；事务日志只用于需要持续跟踪不变量、实现阶段、旁路审计、可观测性和验证证据的工作。

## Active

- [System Target Model](./2026-07-22-system-target-model.md)：R1 Stage 1-3与Checkpoint 1A-1D、2A-2D、3A已关闭；Stage 4保持Outline / Not Resolved，`BOOT-PROTOCOL-001`保持effective baseline。
- [DW-MSHC / SD Cold Discovery](./2026-07-16-dw-mshc-sd-cold-discovery.md)：两轮 correctness findings 已修复，firmware/String/rootfs input 按用户决定完成边界处置，canonical RFC 已更正；当前处于 Runtime Validation，实机 attach/read/write/rootfs 仍待验证。
- [Mount Tree Legacy API](./2026-06-18-mount-tree-legacy-api.md)
- [KThread Core](./2026-06-16-kthread-core.md)
- [OOM Killer](./2026-06-15-oom-killer.md)
- [Fanotify](./2026-06-08-fanotify.md)
- [Signal Temporary Mask Restore](./2026-06-06-signal-temp-mask-restore.md)
- [FileOps Seek and Char Device ioctl](./2026-06-05-fileops-seek-char-ioctl.md)
- [PROC TGID FD](./2026-06-04-proc-tgid-fd.md)
- [Cred Merge](./2026-06-02-cred-merge.md)
- [Sched Latch](./2026-06-03-sched-latch.md)

## Closed / Deferred

- [Sched EEVDF-lite](./2026-07-09-sched-eevdf-lite.md)：Stage 3/R1 runtime acceptance 失败后延期关闭；关闭时恢复 RR，后续 default 已由 Fair / Stride 接管，四个 Keter 保持未解决。此项不是 Completed。

## Completed

- [IOCTL Loop](./2026-06-04-ioctl-loop.md)：VFS ioctl 分发、统一 block ioctl、静态 loop 设备池与第一阶段 loop ioctl 已完成；扩展 LTP 缺口由 register 跟踪。
- [Unix Job Control](./2026-07-20-unix-jobctl.md)
- [Sched Dynamic Attributes](./2026-07-15-sched-dynamic-attributes.md)
- [CPU Logical / Physical ID](./2026-07-14-cpu-logical-physical-id.md)：物理 ID 上界/逻辑 CPU 容量拆分、无锁 registry 和内建 cache padding 的 typed table 已完成；VisionFive 2 由用户复验通过，最终 table 布局与 LoongArch correction build 未由 agent 运行。
- [Sched RT Class R1](./2026-07-14-sched-rt-class-r1.md)
- [Sched Fair / Stride](./2026-07-13-sched-fair-stride.md)
- [Sched RT Class R0](./2026-07-12-sched-rt-class.md)
- [Sched Wait Preempt Arming](./2026-07-06-sched-wait-preempt-arming.md)
- [VFS Direct User I/O](./2026-06-29-vfs-direct-user-io.md)
- [Threaded Timer Event](./2026-06-20-threaded-timer-event.md)
- [KThread](./2026-06-14-kthread.md)
- [Inode Shrinker](./2026-06-14-inode-shrinker.md)
- [Sched Wait Refactor](./2026-06-01-sched-wait-refactor.md)
