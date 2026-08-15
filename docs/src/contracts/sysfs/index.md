# Sysfs 当前契约

**Owner：** `fs::sysfs` static namespace、persistent backing、initial entry metadata 与只读 attribute protocol；architecture consumer 继续拥有各自事实
**覆盖范围：** 首版可挂载 static sysfs、singleton mount lifetime、`/kernel` 静态目录和两个 architecture text attribute
**不覆盖：** kobject/kset、dynamic namespace、device model、hotplug、自动挂载、symlink、binary/writable attribute、其它 consumer 或 generic metadata mutation handoff
**最后核验：** 2026-08-14

本目录只登记已经通过 `STATIC-SYSFS-CUTOVER` 生效的最小静态表面，不把它外推为 Linux kernfs 或未来动态 sysfs 的设计。

## Contract Surfaces

- [Static filesystem](./static-filesystem.md)：静态 namespace、只读 attribute、canonical mount identity 与 persistent singleton lifetime。

## 邻接契约

- [VFS Mount Admission](../vfs/mount-admission.md)：canonical filesystem identity、no-device handoff 与 `/proc/filesystems` projection。
- [`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`](../../register/open-issues.md#ane-20260814-vfs-metadata-mutation-owner-handoff)：post-mutation owner、stat/DAC 与 persistence 的开放边界。
