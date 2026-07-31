# VFS Make Node Tracking Issues

**状态：** R0 Accepted / No Active Findings
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260731-vfs-make-node](./index.md)
**事务日志：** [2026-07-31-vfs-make-node](../../devlog/transactions/2026-07-31-vfs-make-node.md)

本文只跟踪已经影响 target、owner / contract boundary、implementation resolution、停止边界或验收判断的
design finding。implementation 进度与执行证据不放在这里；Draft target 修复已经折回 `index.md` /
`invariants.md`，本文只保留 finding 的问题、决定、修复位置与状态历史。

本轮 review 没有 Apollyon，四个 Keter 与两个 Euclid 均已在 Draft target / implementation manifest 中
Neutralized。这里的
Neutralized 表示文档层问题已有自然落点。2026-08-01 独立复审确认 Apollyon/Keter/Euclid/Safe 全 0，R0
随后接受并建立transaction；Stage 1独立终审再次确认Apollyon/Keter/Euclid/Safe全0，
`DEVICE-NUMBER-CUTOVER`已生效并关闭Stage 1。Stage 2仍为Outline / Not Active，其余target contract保持Not Cut Over。

## Apollyon

None。

## Keter

None active。

## Euclid

None active。

## Safe

None。

## Neutralized

### KETER-VFS-MAKE-NODE-004：Stage 1 manifest 无法覆盖 R0、transaction 与 closure write-back

**原问题：** Stage 1 Ready 的 public write-back 只列出 current device-number contract、RFC 三页、transaction、
transaction index 与双周 devlog，却遗漏 `docs/src/SUMMARY.md`、`docs/src/rfcs.md`、本 tracking page 与
`docs/src/register/current-limitations.md`。device owner index 与旧 mknod register backlink 也会在 cutover/接受后
失真。R0 接受、transaction 双向导航、已接受 umask limitation 与 Stage 1 closure 因而必然需要越过 frozen
manifest。

**决定：** 开发者明确授权完成 R0 接受、transaction setup 与 Stage 1 closure 所需的全部 documentation-level
write-set expansion。第 5.5 节现在显式列出上述四页、device owner index 与相关 open-issue backlink；source
manifest 不扩大，task/umask owner 与 Stage 2 production surface仍不进入 Stage 1。transaction 建立后记录该
授权、实际 baseline 与 activation，不复制第二份计划。

**修复位置：** [Stage 1 Resolved Write Set Manifest](./implementation.md#55-resolved-write-set-manifest)。

**状态：** Neutralized / 2026-08-01；独立复审确认 manifest 闭合，R0/transaction/activation 已执行。

### KETER-VFS-MAKE-NODE-003：Process umask 的可见语义与 owner 边界未固定

**原问题：** Draft 已记录 `sys_umask` 是 stub，却同时承诺 Linux mode admission、最终 permission 与 permission
proof，没有规定 `mknodat` 是否屏蔽 process umask。实现可能顺带扩张 task/fs-state 与全部 create call sites，
也可能忽略 umask 却继续声称完整 Linux permission parity；两条路线改变不同的 target/acceptance boundary。

**决定：** 开发者接受本 revision 的可见偏差：`mknodat` 最终 permission 直接使用 requested bits，不应用
process umask。本 RFC 不读取 `sys_umask` stub、不建立 mknod-local/task-local mask，也不扩大其它创建类调用点；
Linux compatibility claim 收窄为 node-kind、`dev_t`、dirfd、capability 与 errno matrix。后续独立 umask 工作必须
建立 task/fs-state 唯一 owner、统一 `umask(2)` 与全部 create call sites，并以跨创建路径验证完成后才能退出
current limitation。

**修复位置：** [摘要、目标、非目标、ABI 与接受边界](./index.md)、
[MAKE-NODE-ABI-001](./invariants.md#make-node-abi-001--rv64la64-使用-canonical-mknodat-与-linux-node-matrix)、
[Stage 2 protected target](./implementation.md#72-受保护-target-与禁止扩张)。

**状态：** Neutralized / 2026-08-01；已登记 current limitation，Stage 1 source manifest 保持不变。

### KETER-VFS-MAKE-NODE-001：Linux device-number ABI 范围与内部 identity 边界尚未解析

**原问题：** Draft 把 syscall 输入称为 raw `dev_t`，承诺未知或未注册 device number 也可形成 namespace
identity，但没有固定输入宽度、Linux decode 范围和无法进入当前 internal key 时的行为。Linux
`mknodat` 接收 32-bit encoded device number，并由 `new_decode_dev()` 形成 12-bit major + 20-bit minor；live
`CharDevNum` / `BlockDevNum` 使用 16-bit major + 16-bit minor。minor 大于 `65535` 时，当前 typed key 无法直接
表示；`DeviceId::Raw` 又不能保留结构化 numeric identity。

**决定：** 不接受 representable subset。device-owned common value domain 扩展为完整 12-bit major + 20-bit
minor，全部 32-bit Linux encoding 可 round-trip。generic inode 只保存 category-neutral `DeviceNumber`；
`Inode::ty()` 单独决定 Char/Block namespace，`CharDevNum` / `BlockDevNum` 只在 registry consumer boundary
构造。Linux packed layout 只存在于 syscall/stat/statx、loop ioctl 与 ext4 persistence codec，不新增
`DeviceId::Raw`-like escape。

当前 16/16 行为先从
[`ANE-CHG-20260722-device-devnum-ownership`](../../devlog/changes/2026-07-22-device-devnum-ownership.md)
已提取为 device-owned [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md#device-number-001--1220-category-neutral-device-number-domain)
current baseline；12/20 normalization 与既有 devfs/TTY/char/block/
loop/stat consumer migration 使用独立 `DEVICE-NUMBER-CUTOVER`，关闭后才进入 make-node stage。既有 static
major/minor、char/block namespace、endpoint name 与 publication lifecycle 保持不变。

**修复位置：** [摘要、目标、ABI 与接受边界](./index.md)、
[DEVICE-NUMBER-001 Target Refine](./invariants.md#device-number-001-target-refine--numeric-identity-覆盖完整-linux-1220-domain)、
[Contract Impact](./invariants.md#contract-impact)。

**状态：** Neutralized / 2026-07-31 Draft review；baseline extraction 已 docs-only 完成，不自动执行任一 cutover。

### KETER-VFS-MAKE-NODE-002：`VFS-RDEV-001` 的 current baseline、变化分类与 devfs scope 未闭合

**原问题：** Draft 把全局 `VFS-RDEV-001` 列为 Introduce，同时声明 character/block inode 的 immutable
`rdev` 由 filesystem-neutral inode metadata identity 唯一拥有；但 live devfs 已在 publication record 中保存
typed `rdev`，devfs stat 与 char/block operation 读取它，mount admission 也消费 `InodeStat::rdev`。RFC 又明确
不改变 devfs publication registry，因此不能用 Introduce 静默重新拥有既有状态。

**决定：** 删除全局 `VFS-RDEV-001`。既有 device-number semantics 由 device-owned `DEVICE-NUMBER-001`
baseline/Refine 承接；make-node 只 Introduce `VFS-SPECIAL-NODE-RDEV-001`，覆盖 ext4/ramfs filesystem-backed
special node 的 numeric `rdev` persistence/projection。devfs 继续从 typed endpoint capability 投影 number 并
验证 node kind，其 publication record、registry、provider 与 lifecycle 均保持原 owner。

generic inode 不保存 `DeviceId::Char/Block` 这一第二份 category truth。`Inode::ty()` 唯一选择 namespace；mount
先检查 Block kind，再从 numeric pair 构造 `BlockDevNum` 查询 registry。这同时 Preserve
`VFS-FILE-KIND-001`，避免 make-node contract 反向改写 device/devfs owner。

**修复位置：** [Kind、rdev 与 backend coverage](./index.md#kindrdev-与-backend-coverage)、
[VFS-SPECIAL-NODE-RDEV-001](./invariants.md#vfs-special-node-rdev-001--filesystem-backed-special-node-rdev-是单一-numeric-truth)、
[VFS-MOUNT-ADMISSION-002 Target Refine](./invariants.md#vfs-mount-admission-002-target-refine--non-block-source-先于-registry-lookup-返回-enotblk)、
[Contract Impact](./invariants.md#contract-impact)。

**状态：** Neutralized / 2026-07-31 Draft review；不建立 devfs ownership 或 current-contract cutover。

### EUCLID-VFS-MAKE-NODE-002：Unsupported backend 与 special-node open 的 errno 尚未固定

**原问题：** Draft 只要求 pseudo filesystem 的 directory `make_node` 返回 unsupported/permission error，并
要求未纳入数据面的 special-node open 返回稳定错误，但没有固定具体 errno。live
`SysError::NotSupported` 映射为 `EOPNOTSUPP`，而 Linux directory 缺少 mknod operation 的常见结果是 `EPERM`；
FIFO、character、block 与 socket open 也可能被实现成不同错误。

**决定：** visible errno 按语义 owner 固定：non-directory parent `ENOTDIR`；read-only mount `EROFS`；writable
directory backend 缺少 make-node capability `EPERM`；named-FIFO data plane 缺失 `EOPNOTSUPP`；普通 filesystem
character/block node 缺少 resolver `ENXIO`；unbound socket-kind node `ENXIO`；non-block mount source
`ENOTBLK`；合法 block number 的 provider miss 保持当前 `ENOENT`。

live `SysError` 尚无 `ENOTBLK` mapping；实现期需要增加 system-wide semantic error 的窄映射，不能继续让 mount
返回 `EINVAL` 或使用 raw errno。内部 variant 名称仍是 implementation preference。

**修复位置：** [后续 operation 边界与 errno matrix](./index.md#后续-operation-边界)、
[MAKE-NODE-ERROR-001](./invariants.md#make-node-error-001--unsupported-capability-与-missing-provider-使用稳定-errno)、
[VFS-MOUNT-ADMISSION-002 Target Refine](./invariants.md#vfs-mount-admission-002-target-refine--non-block-source-先于-registry-lookup-返回-enotblk)。

**状态：** Neutralized / 2026-07-31 Draft review；只固定 target，不授权修改 `SysError` 或 filesystem ops。

### EUCLID-VFS-MAKE-NODE-001：`CAP_MKNOD` matrix 未说明 Linux whiteout 例外

**原问题：** Draft 声明所有 character/block creation 都要求 effective `CAP_MKNOD`，但只引用
`may_mknod()` / `do_mknodat()`；Linux `vfs_mknod()` 对 `S_IFCHR + WHITEOUT_DEV` 保留 capability 例外。未说明
该差异会让 character 0:0 的用户可见结果成为 implementation 偶然选择。

**决定：** R0 不引入 overlayfs whiteout identity、copy-up/rename protocol 或 whiteout lifecycle，因此不采用
该 capability 例外。用户态 character 0:0 与其它 character node 一样要求 effective `CAP_MKNOD`，缺少时返回
`EPERM`；它不得被解释成 whiteout 或无特权 special node。

**修复位置：** [目标、非目标、ABI matrix 与接受边界](./index.md)、
[MAKE-NODE-ABI-001](./invariants.md#make-node-abi-001--rv64la64-使用-canonical-mknodat-与-linux-node-matrix)；
公共证据补充 `xref:linux-6.6.32:fs/namei.c#vfs_mknod`。

**状态：** Neutralized / 2026-07-31 Draft review；不建立 accepted revision、implementation authorization 或
current-contract cutover。
