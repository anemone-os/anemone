# 事务日志

事务日志记录跨多天、跨子系统、需要阶段性审计证据的大型重构或迁移。

日常开发日志仍按双周追加；事务日志只用于需要持续跟踪不变量、实现阶段、旁路审计、可观测性和验证证据的工作。

## Active

- [Mount Tree Legacy API](./2026-06-18-mount-tree-legacy-api.md)
- [KThread Core](./2026-06-16-kthread-core.md)
- [OOM Killer](./2026-06-15-oom-killer.md)
- [Fanotify](./2026-06-08-fanotify.md)
- [Signal Temporary Mask Restore](./2026-06-06-signal-temp-mask-restore.md)
- [FileOps Seek and Char Device ioctl](./2026-06-05-fileops-seek-char-ioctl.md)
- [IOCTL Loop](./2026-06-04-ioctl-loop.md)
- [PROC TGID FD](./2026-06-04-proc-tgid-fd.md)
- [Cred Merge](./2026-06-02-cred-merge.md)
- [Sched Latch](./2026-06-03-sched-latch.md)

## Completed

- [VFS Direct User I/O](./2026-06-29-vfs-direct-user-io.md)
- [Threaded Timer Event](./2026-06-20-threaded-timer-event.md)
- [KThread](./2026-06-14-kthread.md)
- [Inode Shrinker](./2026-06-14-inode-shrinker.md)
- [Sched Wait Refactor](./2026-06-01-sched-wait-refactor.md)
