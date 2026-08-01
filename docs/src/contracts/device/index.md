# Device 当前契约

**Owner：** device-number numeric namespace、character/block endpoint identity 与各自 registry admission
**覆盖范围：** 当前 effective 12/20 category-neutral device-number domain、char/block typed registry namespace、
endpoint-owned number、producer-owned number/name policy 与 registry-derived key
**不覆盖：** device discovery、devfs/TTY publication lifecycle、provider open/lifetime、block I/O、hotplug/unpublish、
ordinary-filesystem special-node creation
**最后核验：** 2026-08-01

本目录只登记已经由 live code 与既有执行证据证明生效的 Device 共享规则，不声称枚举 device subsystem
全部不变量。

## Contract Surfaces

- [Device number namespace](./device-number.md)：12/20 category-neutral numeric domain、canonical Linux
  `dev_t` codec、char/block namespace separation、endpoint-owned number 与 producer/registry ownership；VFS
  Make Node R0 `DEVICE-NUMBER-CUTOVER` 已生效，Stage 2 不重新打开该 namespace。

## 邻接契约

- [File kind 与 Linux mode projection](../vfs/file-kind.md)：immutable inode kind 与 Linux file-type projection。
- [Mount admission](../vfs/mount-admission.md)：block-source resolution 与 typed block-provider handoff。
- [Serial TTY data plane](../tty/data-plane.md)：TTY endpoint identity、publication 与 lifecycle。
