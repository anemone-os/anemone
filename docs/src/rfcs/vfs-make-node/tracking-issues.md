# VFS Make Node Tracking Issues

**状态：** Public Draft Review / No Active Findings
**最后更新：** 2026-07-31
**父 RFC：** [RFC-20260731-vfs-make-node](./index.md)
**事务日志：** None；公开 Draft 尚未进入实现

本文只跟踪已经影响 target、owner / contract boundary、implementation resolution、停止边界或验收判断的
design finding。implementation 进度与执行证据不放在这里；Draft target 修复已经折回 `index.md` /
`invariants.md`，本文只保留 finding 的问题、决定、修复位置与状态历史。

本轮 review 没有 Apollyon，两个 Keter 与两个 Euclid 均已在 Draft target 中 Neutralized。这里的
Neutralized 只表示文档层问题已有自然落点；[implementation plan](./implementation.md) 已独立撰写，但不因此
建立 accepted revision、public contract、transaction、Active stage 或代码实现授权。

## Apollyon

None。

## Keter

None active。

## Euclid

None active。

## Safe

None。

## Neutralized

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
已提取为 device-owned [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md#device-number-001--1616-typed-device-number-namespace)
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
