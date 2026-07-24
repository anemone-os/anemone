# ANE-CHG-20260722-device-devnum-ownership

**Type:** Cleanup
**Status:** Completed
**Date:** 2026-07-22
**Authors:** doruche, Codex
**Area:** device model / devnum / char / block / MMC / virtio

## Problem

Anemone 当前的 char 与 block core 同时承担设备 registry、driver registry、dynamic major
分配和部分 block 节点命名。物理驱动为了取得 major 必须先注册为 `CharDriver` 或
`BlockDriver`，block core 还通过 `BlockDevClass` 和 class-local counter 生成 `vda`、
`mmcblk0`、`loop0`、`ram0`。这把静态设备号归属、具体 endpoint capability 和通用文件行为
混在了同一层，也让后续 TTY 等领域需要经过不属于自己的 major allocator。

## Scope

本轮只收口当前内建 char/block 发布域的设备号 ownership：

- 在 `device::devnum` 中声明当前 char/block producer 使用的静态 major；
- 移除 char/block core 的 dynamic major、driver registry 和 block class naming；
- 让 char/block registry 从 endpoint capability 的 `devnum()` 派生唯一 key；
- 让 block producer 同时生成自己的 local minor、canonical name 和 capability；
- 在 Linux `struct stat` 边界编码 userspace `dev_t`，保持 `statx` 的分离 major/minor；
- 保持现有节点名、block registry、devfs、通用 block ioctl 和 I/O 行为。

本轮不修改 TTY、MMC host/card/discovery、block I/O/partition/hotplug/unregister、mount
语义或通用 probe transaction；也不新增 `DeviceId -> open provider` resolver。

## Solution

`device::devnum` 现在保存当前发布域的静态 namespace 常量：char memory/TTY/misc/raw serial
使用 major `1/4/10/234`，block ramdisk/loop/SCSI/MMC/virtio 使用 major
`1/7/8/179/2048`。char 与 block 是独立 namespace，同一个 numeric major 可以在两个发布域
分别存在。

`CharDev::devnum()` 与 `BlockDev::devnum()` 是 endpoint identity 的唯一真相源。registry 在
自己的锁内从 capability 读取它并拒绝重复 devnum/name；registration 不再接收并列的第二份
devnum。char producer 只提交 name 与 capability。virtio、MMC block frontend、loop 和 ramdisk
各自用一个 local instance id 同时推导 minor 与 canonical name；NS16550A 使用静态 raw-serial
major 和 driver-local minor allocator。

内部 `CharDevNum` / `BlockDevNum` 继续使用 16-bit major / 16-bit minor。只有转换为 Linux
`struct stat` 时才生成非连续的 userspace `dev_t` 位布局；`statx` 继续直接报告内部
major/minor，`Raw` device id 保持既有投影。

## Change

- 删除 `CharDriver`、`BlockDriver`、dynamic major allocator、driver map、`BlockDevClass`、
  `next_name_idx` 和 block-core-owned name formatter。
- 将 static major 与 producer-local devnum/name 映射放入 `devnum.rs`、NS16550A、virtio,
  MMC、loop 和 ramdisk owner；char built-in endpoints 改用 capability-derived registration。
- 保留 block registry 的 name/devnum 索引、I/O handle、readahead、通用 FileOps、devfs bridge
  和 `BLK*` 行为，不再让 registry 决定 name prefix 或 instance index。
- 在 inode ABI conversion 中增加 Linux `dev_t` encoder，并以 KUnit 覆盖内部 key、namespace
  uniqueness、registry duplicate rejection、producer naming 和 `stat`/`statx` 一致性。

## Validation

Agent-run validation:

- `just fmt kernel --check` 通过；
- `git diff --check` 通过；
- source audit 确认 `CharDriver`、`BlockDriver`、`BlockDevClass`、dynamic major allocator、
  `register_*_driver` major path 和 block-core name allocator 均无命中；
- `just build` 通过，包含 `kunit`、`fs_ext4` 和 `spin_lock_irqsave` 特性；
- RV64 clean-rootfs smoke 已进入 `Running 212 tests...`，KUnit 输出 `All tests passed!`，
  `/sbin/init` 启动成功并完成 virtio block 初始化；
- 当前 QEMU RV64 `virt` 平台没有 MMC endpoint，因此 `mmcblk0` hardware smoke 未运行。

未运行项不被记录为通过；MMC 验证保留在本记录的 tracking issue 中。运行脚本随后执行的
LTP profile 不是本次 cleanup 的验收边界。

## Tracking Issues

### CHG-001 - partition minor layout 不在本轮证明范围

**Status:** Deferred
**Severity:** Safe

**Issue:** 未来引入 MMC、virtio 或 SCSI partition node 时，whole-disk endpoint 可能需要
预留 minor stride；本轮没有 partition node 或对应生命周期。

**Resolution:** 本轮只保证当前 whole-device endpoint 的静态 namespace 与唯一性。引入
partition 前由对应 block frontend 明确 minor stride 和 ABI 迁移，不把策略放回通用 block
naming registry。

### CHG-002 - block registry 保留名称索引但不生成名称

**Status:** Neutralized
**Severity:** Keter

**Issue:** 完全删除 block name index 会破坏 rootfs name lookup、devfs publish 和全局名称
唯一性；继续由 block core 按 class 生成名称又会保留错误 owner。

**Resolution:** producer 生成并移交 canonical name；block registry 只校验、存储和查询，
不决定 prefix/index policy。该边界已落实在 `BlockDevRegistration { name, device }`。

### CHG-003 - MMC endpoint runtime smoke

**Status:** Deferred
**Severity:** Safe

**Issue:** 当前 QEMU RV64 平台没有 MMC endpoint，无法在本轮确认真实首个 SD Memory endpoint
为 `mmcblk0` / major `179`，以及普通 `stat` 与 `statx` 的硬件路径一致性。

**Resolution:** 保留为环境受限的验证缺口，不新增 register/current-limitations 条目；在
有 MMC 平台时运行 frontend smoke，并检查 endpoint name/devnum 与两种 stat 投影。

## Risk / Follow-up

- probe 在 endpoint 注册前失败时可能消耗 producer-local instance id 并留下命名间隙；本轮
  接受该启动期限制，不引入通用 rollback 或 probe transaction。
- 静态 major 替代 dynamic major 是有意的设备号可见变化；当前没有旧 dynamic major ABI
  承诺。
- TTY 开工前置边界已经收口：TTY 使用自己的静态 major `4` 和 TTY registry，不经过 char
  dynamic major path。

## Links

- Biweekly devlog: [2026-07-06 至 2026-07-19](../2026-07-06_to_2026-07-19.md)
- Current contract: None; this cleanup does not change an effective shared contract.
- Register / limitations: None required; the MMC item above is an environment-limited validation gap.
- RFC / transaction: None; the private design note was promoted as this small change record.
