# Device 当前契约

**Owner：** device-number numeric namespace、character/block endpoint identity 与各自 registry admission
**覆盖范围：** 本轮按触达提取的 16/16 typed device-number namespace、endpoint-owned number、producer-owned
number/name policy 与 registry-derived key
**不覆盖：** device discovery、devfs/TTY publication lifecycle、provider open/lifetime、block I/O、hotplug/unpublish、
ordinary-filesystem special-node creation
**最后核验：** 2026-07-31

本目录只登记已经由 live code 与既有执行证据证明生效的 Device 共享规则，不声称枚举 device subsystem
全部不变量。

## Contract Surfaces

- [Device number namespace](./device-number.md)：16/16 typed numeric domain、char/block namespace separation、
  endpoint-owned number 与 producer/registry ownership。

## 邻接契约

- [File kind 与 Linux mode projection](../vfs/file-kind.md)：immutable inode kind 与 Linux file-type projection。
- [Mount admission](../vfs/mount-admission.md)：block-source resolution 与 typed block-provider handoff。
- [Serial TTY data plane](../tty/data-plane.md)：TTY endpoint identity、publication 与 lifecycle。
