# VFS Make Node 目标和不变量

**状态：** R2 Effective / RFC Closed
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260731-vfs-make-node](./index.md)
**适用修订：** R2

本文定义本RFC的contract delta、target invariants与RFC-local proof obligations。当前已经生效的共享规则仍以
`docs/src/contracts/`中的稳定ID为准；`DEVICE-NUMBER-001` Refine已在`DEVICE-NUMBER-CUTOVER`生效，make-node
两个Introduce ID与mount Refine也已在`VFS-MAKE-NODE-CUTOVER`生效。

## 规则分类

- **Correctness Invariant：** kind/numeric `rdev` 单一真相源、owner boundary、正常执行下有序且串行化的
  backend publication、成功路径reload一致性、可实施cleanup与ABI诚实性；违反即实现不正确，不能降级接受。
- **Target Guarantee / Capability：** Linux node-kind/capability matrix、明确不应用 process umask 的 requested
  permission semantics、`InodeOps::make_node`、ext4+ramfs coverage、metadata observation、mount `ENOTBLK`
  与用户态 proof；新修订接受前不能缩减或把已接受限制写成 Linux parity。
- **Implementation Preference：** Rust description 类型、字段物理落点、regular dispatch、helper/module、
  lock/transaction shape 与精确测试命令；保持 target 时由后续 implementation resolution 决定。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `VFS-FILE-KIND-001` | Preserve | [当前规则](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth) | make-node 只构造既有 immutable kind，不新增第二份 file-type truth | 全程；无需改写 current rule |
| `DEVICE-NUMBER-001` | Refine | [当前12/20规则](../../contracts/device/device-number.md#device-number-001--1220-category-neutral-device-number-domain)；2026-07-31 docs-only extraction曾建立16/16 baseline | common numeric domain改为12-bit major + 20-bit minor；通用inode metadata不保存Char/Block category或raw packing；typed registry key只在consumer boundary构造 | `DEVICE-NUMBER-CUTOVER` Effective / 2026-08-01 |
| `TTY-ENDPOINT-001` | Preserve | [当前规则](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction) | 保持 `ttyS<N>` 4:`64+N`、console 5:1、deterministic identity 与 publication lifecycle | 全程；device-number cutover 需证明数值未变 |
| `VFS-MAKE-NODE-001` | Introduce | [当前规则](../../contracts/vfs/make-node.md#vfs-make-node-001--make-node-admission-与-backend-publication-只有一个-handoff) | VFS admission 经 `InodeOps::make_node` 向 backend 交付窄 semantic description；backend-local先完成最终metadata，再串行化进入dirent publication，并清理仍未链接的失败分配 | `VFS-MAKE-NODE-CUTOVER` Effective / 2026-08-01 |
| `VFS-SPECIAL-NODE-RDEV-001` | Introduce | [当前规则](../../contracts/vfs/make-node.md#vfs-special-node-rdev-001--filesystem-backed-special-node-rdev-是单一-numeric-truth) | ext4/ramfs filesystem-backed character/block node 持久/驻留保存 category-neutral numeric `rdev`；kind 只来自 `Inode::ty()` | `VFS-MAKE-NODE-CUTOVER` Effective / 2026-08-01 |
| `VFS-MOUNT-ADMISSION-002` | Refine | [当前规则](../../contracts/vfs/mount-admission.md#vfs-mount-admission-002--source-kind-owned-admission) | 保持 source-kind owner；non-block inode source 在 registry lookup 前稳定返回 `ENOTBLK` | `VFS-MAKE-NODE-CUTOVER` Effective / 2026-08-01 |

R0 acceptance曾为`DEVICE-NUMBER-001`建立pending-successor link；2026-08-01 `DEVICE-NUMBER-CUTOVER`已把该
target原子写入current contract。`DEVICE-NUMBER-BASELINE-EXTRACTION`建立的16/16规则只保留为历史baseline。
后续make-node stage复用了已关闭的device-number owner边界。最终VFS cutover已在VFS contract下建立单一
make-node/filesystem-backed-rdev surface容纳两个Introduce ID，没有为每个ID单独建页，也没有把RFC-local
validation规则写入current contract。

R2再次修订原子性接受边界：backend-local final metadata必须先于dirent publication，同一backend锁必须阻止
正常并发lookup观察中间状态；成功路径必须在normal sync/reload后恢复同一metadata。可预先判定的错误不得发布node，
已分配但仍未链接的inode必须尝试清理并传播cleanup失败。lwext4 block-cache写回、任意内部I/O failure以及
crash/power-loss下inode/dirent全有或全无明确不属于本revision保证，由
[accepted limitation](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)与后续
lwext4/Rust wrapper事务承接。既有common-create跨owner窗口继续由独立open issue承接。

### DEVICE-NUMBER-001 Target Refine — Numeric identity 覆盖完整 Linux 12/20 domain

**分类：** Correctness Invariant / ABI Correctness / Target Prerequisite。

**规则：** device-owned common value domain 使用 12-bit major + 20-bit minor，能够无损 decode/encode
`mknodat` 的全部 32-bit Linux encoded device number。通用 inode metadata 只保存 category-neutral numeric
pair；`Inode::ty()` 是 character/block category 的唯一 truth。`CharDevNum` / `BlockDevNum` 保持为独立 registry
domain key，只能在已知 consumer namespace 的边界由 numeric pair 构造；它们不能重新进入通用 inode 形成
第二份 kind truth。

Linux packed representation 只允许存在于 syscall/stat/statx、loop ioctl 与 ext4 on-disk codec 等明确
ABI/persistence boundary。不得新增或保留用于逃避结构化 identity 的 `Raw` variant。existing devfs、TTY、char/
block registry 与 producer-local allocator 可以迁移 value type，但 publication owner、namespace 分离、
provider lifecycle、既有 static major/minor 和 endpoint name 都不改变。

**Owner：** `device::devnum` numeric domain；inode kind 仍由 `VFS-FILE-KIND-001` 拥有；char/block registry
分别拥有自己的 typed lookup namespace。

**依赖：** `VFS-FILE-KIND-001`、`TTY-ENDPOINT-001`（Preserve）。

**违反表现：** 对合法 32-bit device input 截断、panic 或额外 `EINVAL`；内部继续采用 16/16；generic inode
保存 `Char/Block` tag 或 raw packed integer并驱动行为；stat/loop/ext4 各自发明不兼容 packing；或迁移导致
TTY/block 既有号码、name、registry namespace 或 publication lifecycle 改变。

**Cutover：** live baseline 已完成 docs-only 提取；随后 `DEVICE-NUMBER-CUTOVER` 原子迁移既有 producer、allocator、
devfs/inode projection、TTY、char/block registry、loop/stat codec，并单独证明 current contract Refine。该 gate
关闭前不得进入 make-node implementation stage。

## Target Invariants

### VFS-MAKE-NODE-001 — Make-node admission 与 backend commit 只有一个 handoff

**分类：** Correctness Invariant / Target Guarantee。

**规则：** syscall adapter 只拥有 Linux `mknodat` 参数读取、mode/`dev_t`/`dirfd`/capability admission；VFS
唯一拥有 parent resolution、writable mount、directory DAC、共同 creation policy 与 dentry materialization；
directory inode 通过唯一 `InodeOps::make_node` function pointer 把已经 admission 的 filesystem-neutral node
description 交给 backend。backend 只拥有 inode allocation、resident/persistent encoding 与 directory-entry
transaction，不重新读取 task、fd table、raw Linux mode、capability、FIFO state 或 device registry。

node description 语义上只包含 final kind、permission/owner result 与适用的 category-neutral numeric `rdev`。
精确 Rust type、字段布局、参数顺序、backend 的 transaction/rollback 形状以及 regular node 是否内部复用
`touch` 不属于本 invariant。

**Owner：** syscall ABI adapter、VFS creation protocol 与 filesystem backend 各自拥有上述局部状态；跨域
handoff owner 是 VFS make-node protocol。

**依赖：** `VFS-FILE-KIND-001`、已生效的 target `DEVICE-NUMBER-001`。

**违反表现：** ext4/ramfs 解析 raw `mode_t`；backend 查询当前 task/CAP_MKNOD；syscall 根据 filesystem name
分发；VFS 根据 private downcast 猜 kind；新增第二个 make-node trait/factory；或 callback 返回后由 VFS 再补一份
驱动行为的 backend metadata truth。

**Cutover：** `VFS-MAKE-NODE-CUTOVER`。

### VFS-SPECIAL-NODE-RDEV-001 — Filesystem-backed special-node `rdev` 是单一 numeric truth

**分类：** Correctness Invariant。

**规则：** ext4/ramfs 的 character/block node 在创建时一次确定 category-neutral numeric `rdev`；该 number
由 filesystem-neutral inode metadata owner 唯一拥有，category 只由 immutable `Inode::ty()` 决定。ext4 必须
将 number 编码到 on-disk inode并在 eviction/remount 后恢复；ramfs 必须在 resident inode lifetime 内保持。
`stat`、`statx`、mount admission 与未来另行接受的 device resolver 读取同一 number，再由 inode kind 选择
character/block namespace。regular/FIFO/socket 的 `dev` 被 ABI adapter 忽略，不形成隐藏 raw id。

`rdev` 不代表 provider handle 或 provider 存在性。make-node 不查询 registry，不因 provider miss 拒绝
creation，也不把 devfs node、device class 或 strong endpoint capability 保存到普通 filesystem inode。

本 ID 只覆盖 filesystem-backed special node 的 persistence/projection。devfs 继续从已经注册的 typed endpoint
capability 投影 numeric pair并验证 node kind；其 publication record、registration、open provider 与 lifecycle
仍由 device/devfs current owner 保持，不被本 ID 重新拥有。

**Owner：** filesystem-neutral inode numeric metadata identity；ext4/ramfs 分别拥有其持久/resident
representation；`Inode::ty()` 独立拥有 category。

**依赖：** `DEVICE-NUMBER-001`、`VFS-FILE-KIND-001`、`VFS-MAKE-NODE-001`。

**违反表现：** 首次 stat 正确但 reload 后 `rdev=0`；generic metadata 再次保存 Char/Block category；stat 与
mount 各读一份 value；provider registration 反向改写 inode；未知 device number 无法创建；普通 node 保存
devfs/private provider object；或本 ID 改写 devfs publication lifecycle。

**Cutover：** `VFS-MAKE-NODE-CUTOVER`。

### MAKE-NODE-ATOMIC-001 — Backend-local 有序可见性与诚实 cleanup

**分类：** Correctness Invariant。

**规则：** backend必须在自己的creation boundary内先写入最终kind、permission、owner、适用`rdev`，再进入
directory-entry publication；同一backend锁覆盖这段顺序，使正常并发lookup不能观察final metadata形成前的node。
可在publication前判定的invalid input、duplicate与admission failure不得留下可lookup node。已经分配但仍未链接的
inode必须尝试回收；cleanup自身失败必须向上返回或记录，不能伪装原始operation成功。

live common create path 当前在 backend callback 后调用 owner initialization，不能直接满足本规则。make-node
stage 必须让 VFS 在 callback 前确定 final uid/gid/inheritance 并把结果放入窄description，同时补齐 ext4 owner
persistence；不得把post-dirent owner patch-up写成已接受的backend-local有序语义。

R2不要求lwext4证明block-cache物理写回顺序，也不承诺`add_entry`内部任意I/O failure、crash或power-loss下
child inode与parent dirent全有或全无。不得为制造该证明在本RFC中增加forced flush、伪journal、双阶段truth或
validation-only production hook。ramfs或未来backend能够提供的更强transaction语义可以保留，但不是两backend
共同acceptance floor。

callback成功后的inode-cache/dentry materialization复用现有common-create handoff。本RFC要求make-node不新增比
touch/mkdir更弱的failure window、不用panic或success stub掩盖失败，但不要求在Stage 2内为既有backend-commit后
窗口建立新的跨owner rollback。若关闭该窗口需要同时改造backend、inode cache、dentry cache与全部create caller，
按登记问题处理，不阻塞本invariant的backend-local closure。lwext4/Rust wrapper自身的strict failure/crash
atomicity由独立后续事务负责，同样不反向否定当前内核中已经正确的syscall/VFS/metadata路径。

unlink 只移除 namespace/link owner；本 revision 没有 FIFO endpoint、device provider 或 socket endpoint
需要由 unlink cleanup。已创建 special node 的 unsupported open 必须返回错误，不允许 panic 放大资源泄漏或
留下 partially opened file。

**Owner：** 对应ext4/ramfs backend-local creation/publication protocol；VFS继续拥有既有dentry materialization
handoff。lwext4内部dirty-block、journal与failure rollback由后续lwext4/Rust wrapper事务拥有；跨owner
common-create redesign不由本RFC新增owner。

**依赖：** `VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`。

**违反表现：** 正常并发lookup能观察final metadata形成前的node；可预先判定的错误在publication后才处理；
已确认仍未链接的allocated inode不尝试cleanup；ext4正常sync/reload丢失成功结果metadata；make-node引入比现有
common-create更弱的新handoff；或open通过`unimplemented!()` / `unreachable!()` panic。lwext4内部I/O failure或
crash后出现partial persistence属于已登记limitation，不得反写成已证明或已修复。

**Cutover：** RFC-local proof obligation；长期共享部分并入 `VFS-MAKE-NODE-001`。

### MAKE-NODE-ABI-001 — RV64/LA64 使用 canonical `mknodat` 与 Linux node matrix

**分类：** Target Guarantee / ABI Correctness。

**规则：** RV64 与 LA64 注册 asm-generic `SYS_MKNODAT = 33`。relative pathname 按 `dirfd` 解析，
`AT_FDCWD` 使用 cwd，absolute pathname 忽略 `dirfd`；libc `mknod()` 经 `mknodat(AT_FDCWD, ...)` 取得同一
能力，不新增当前架构 ABI 不存在的 legacy syscall number。

type bits 为 0 或 `S_IFREG` 创建 empty regular node；`S_IFIFO`、`S_IFCHR`、`S_IFBLK`、`S_IFSOCK`
分别创建对应 kind；`S_IFDIR` 以 `EPERM` 拒绝，`S_IFLNK` 与其它非法 type bits 以 `EINVAL` 拒绝。只有
character/block creation 要求 effective `CAP_MKNOD`；只有这两类消费并 decode `dev`，其它支持 kind 忽略
`dev`。R2 继承 R0/R1 的决定，不实现 Linux whiteout identity/lifecycle，也不采用
`S_IFCHR + WHITEOUT_DEV` 的 capability 例外；
character 0:0 缺少 effective `CAP_MKNOD` 时仍返回 `EPERM`。

R2 继承 R0/R1 的 no-umask 边界。最终 permission 直接使用调用者 `mode` 中请求的 permission bits；syscall/VFS/backend
不得读取当前 `sys_umask` stub、缓存 task-local mask 或建立 mknod-local mask owner。该规则是本 revision
明确接受的 Linux 可见偏差，不得把 node-kind/dev_t/errno compatibility claim 扩大成 umask parity。退出该偏差
需要后续独立工作为 task/fs-state mask、`umask(2)` 与全部创建类调用点建立共同 owner、handoff 和验证。

`dev` syscall argument 是 32-bit Linux encoded device number；全部输入无损 decode 为 12-bit major + 20-bit
minor 的 `DeviceNumber`。syscall adapter 不把 packed value 或 Char/Block tag传给 filesystem。

**Owner：** syscall ABI adapter；VFS/backend 只接收 normalized semantic input。

**依赖：** `DEVICE-NUMBER-001`、`VFS-MAKE-NODE-001`。

**违反表现：** `mknod()` libc wrapper 继续得到 `ENOSYS`；新增错误 legacy number；FIFO-only；socket node 被
当成 endpoint；regular/FIFO/socket 被错误要求 CAP_MKNOD；device number 在 backend 才 decode；invalid type
成功创建 ordinary file；character 0:0 被无特权创建或被误解释为已有 whiteout protocol；当前 umask stub
偶然改变 permission；或文档/测试把未实现的 umask adjustment 宣称为 Linux-compatible behavior。

**Cutover：** RFC-local ABI target，在 `VFS-MAKE-NODE-CUTOVER` 一并验证。

### MAKE-NODE-FS-001 — ext4 与 ramfs 是同一首版 backend closure

**分类：** Target Guarantee。

**规则：** ext4 与 ramfs 都必须接受本 RFC 支持的 node matrix，并提供 lookup、stat/statx、getdents64 与
unlink。ext4 还必须证明 kind、permission、owner 与 character/block `rdev` 的 eviction/remount reload；ramfs
必须证明 resident lifetime 内 identity/metadata 稳定。devfs、procfs、anonymous filesystem 与其它 pseudo
filesystem 不获得用户可写 make-node namespace，明确拒绝其 directory `make_node`。

支持 make-node 的 directory backend 之外，writable pseudo-filesystem directory 或缺少 operation 的 directory
以 `EPERM` 拒绝；non-directory parent 由 namei/VFS 以 `ENOTDIR` 拒绝。backend 不得自行改成 `ENOSYS` 或
filesystem-specific errno。

本 target 不要求所有未来 filesystem 自动支持 make-node；新 backend 可以显式拒绝，但不得 success 后创建
错误 kind。ext4 与 ramfs 任一从本 revision 移除都属于 acceptance-boundary 变化。

**Owner：** ext4 与 ramfs backend；VFS 保持 common handoff。

**依赖：** `VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`、`MAKE-NODE-ATOMIC-001`。

**违反表现：** ext4-only；ramfs 根据 path 特判；pseudo filesystem 变成可写；reload 丢 type/owner/rdev；
unsupported backend 静默创建 regular node。

**Cutover：** `VFS-MAKE-NODE-CUTOVER`。

### MAKE-NODE-DATA-BOUNDARY-001 — Node creation 不取得 special-file 数据面 ownership

**分类：** Correctness Invariant / Target Boundary。

**规则：** make-node success 只承诺 namespace 与 metadata。Regular node 复用现有 ordinary regular-file
behavior；FIFO/character/block/socket 不因创建成功自动获得 data plane。未实现的 open 必须返回明确错误。

named FIFO buffer、reader/writer membership、wait/readiness、EOF/`EPIPE` 与 unlink-while-open 归 pipe/FIFO
owner；character/block/TTY/console provider lookup 归未来单独接受的 resolver 与各设备 owner；pathname socket
bind/connect 归 socket owner。make-node 不创建、缓存或清理这些状态。

**Owner：** make-node 只拥有 creation protocol；各后续 operation owner 保持独立。

**依赖：** `VFS-MAKE-NODE-001`。

**违反表现：** 每次 FIFO open 新建匿名 pipe；persistent inode 保存 provider strong handle；VFS 把 TTY 降格为
generic CharDev；socket-kind node 被当成 bound endpoint；unsupported open panic；或为了 LTP case 在 pathname
dispatch 中建立数据面。

**Cutover：** RFC-local target boundary；数据面 follow-up 不进入本 cutover。

### MAKE-NODE-ERROR-001 — Unsupported capability 与 missing provider 使用稳定 errno

**分类：** Target Guarantee / ABI Correctness。

**规则：** visible failure 由最接近语义 owner 的边界确定：non-directory parent 为 `ENOTDIR`；read-only
mount 为 `EROFS`；writable directory backend 缺少 make-node capability 为 `EPERM`；已创建 FIFO node 在 named
FIFO data plane 缺失时 open 返回 `EOPNOTSUPP`；ordinary-filesystem character/block node 在 device resolver
缺失时 open 返回 `ENXIO`；未绑定 endpoint 的 socket-kind node open 返回 `ENXIO`；non-block mount source 在
registry lookup 前返回 `ENOTBLK`；合法 block number 但 provider 未注册继续使用当前 provider-miss errno
（当前为 `ENOENT`）。

`ENOTBLK` 必须通过 system-wide semantic error 的明确 errno mapping 表达，不能继续复用 `EINVAL` 或只在
mount syscall 中塞入 raw errno。内部 error variant 名称属于 implementation preference。

**Owner：** namei/VFS parent admission、mount writable admission、filesystem operation dispatch、special-node
open boundary 与 mount source-kind admission 分别拥有上述局部错误；make-node backend 不替其它 owner 选择错误。

**依赖：** `VFS-MAKE-NODE-001`、`MAKE-NODE-DATA-BOUNDARY-001`、current
`VFS-MOUNT-ADMISSION-002`。

**违反表现：** pseudo-fs 返回 `ENOSYS`/`EOPNOTSUPP`；non-directory parent 到 backend 后才失败；unsupported
special-node open panic或全部返回同一错误；non-block mount 返回 `EINVAL`/provider miss；或 provider miss 被
改成 `ENOTBLK`。

**Cutover：** make-node 与 mount-visible 部分在 `VFS-MAKE-NODE-CUTOVER` 一并验证；不改变 named FIFO/device
resolver/socket endpoint 的 non-target 边界。

### VFS-MOUNT-ADMISSION-002 Target Refine — Non-block source 先于 registry lookup 返回 `ENOTBLK`

**分类：** Target Guarantee / ABI Correctness。

**规则：** block-backed filesystem 的 legacy mount adapter 继续唯一拥有 raw source resolution。它先读取
`Inode::ty()`；只有 Block kind 才读取 category-neutral numeric `rdev`、在 registry boundary 构造
`BlockDevNum` 并进入 block lookup。character、regular、FIFO、socket 或无 `rdev` source 在 registry lookup 前
返回 `ENOTBLK`。合法 block `rdev` 但 provider 未注册仍按当前 provider-miss error（`ENOENT`）处理，不能与
kind mismatch 合并。

**Owner：** 现有 VFS mount-admission protocol；make-node 只提供 inode identity。

**依赖：** current `VFS-MOUNT-ADMISSION-002`、`VFS-FILE-KIND-001`、`DEVICE-NUMBER-001`、
`VFS-SPECIAL-NODE-RDEV-001`、`MAKE-NODE-ERROR-001`。

**违反表现：** character source 返回 `EINVAL`/provider miss；通过 `rdev` variant 而不是 inode kind 判断
category；查询 registry 后才判断 kind；mount 重新 decode raw dev_t；或 make-node 接管 mount/provider policy。

**Cutover：** `VFS-MAKE-NODE-CUTOVER` 原子 Refine current rule。

### MAKE-NODE-VALIDATION-001 — 用户态 proof 复用现有 runner且临时 probe 必须退出

**分类：** RFC-local Proof Obligation。

**规则：** 用户态 proof 首先使用现有 `user-test` runner 的 focused `mknod*`、`mknodat*` 与 `mount02` LTP。
LTP 未覆盖的 `st_rdev`/statx、ramfs 或 ext4 reload 可以临时修改现有 `user-test`；临时 probe 必须只有真实
target gap consumer，不进入 production dependency，并在 `VFS-MAKE-NODE-CUTOVER` closure 前删除。transaction 记录执行
逻辑、结果、架构/filesystem、删除事实与 Not Run 项。

不新建长期 test app、test target 或 validation facade。KUnit/source audit 只证明内部 codec、rollback、
exhaustive dispatch 与 no-panic，不能代替真实 syscall runtime。

**Owner：** implementation transaction 的 validation protocol；production source 不拥有测试用例名称或路径。

**依赖：** 全部本 RFC target。

**违反表现：** production dispatch 匹配 LTP pathname；新 app 无 production consumer；临时 probe 留成公共 API；
只跑 KUnit 就宣称 userspace ABI PASS；或把单架构/单 filesystem 证据替代完整 claim。

**Cutover：** RFC-local；不进入 current contract。

## 状态所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 |
| --- | --- | --- |
| raw `dirfd` / pathname / `mode_t` / `dev_t` syscall input | `mknodat` ABI adapter | VFS 只接收 normalized request |
| requested permission bits | 当前 `mknodat` adapter / VFS creation handoff | backend 保存不经 process umask 屏蔽的 final permission；后续独立 umask owner 尚不存在 |
| 12/20 numeric device-number domain | `device::devnum` | ABI/persistence codec 与 registry key 只做边界转换 |
| parent resolution、mount/DAC/common-create admission、dentry materialization | VFS creation protocol | backend 接收 admitted description |
| directory backend operation | `InodeOps::make_node` | VFS 持 function capability，不访问 backend private state |
| inode kind | immutable `Inode::ty()` | stat/getdents/open/mount 只读取 |
| filesystem-backed character/block numeric `rdev` | filesystem-neutral inode metadata identity | backend 持久/驻留；consumer 以 inode kind 选择 namespace |
| typed char/block lookup key | 对应 char/block registry domain | consumer 从已验证 kind + numeric pair 临时构造 |
| ext4 inode/dirent transaction 与 on-disk encoding | ext4 | VFS 持返回的 `InodeRef` |
| ramfs inode/dirent transaction 与 resident state | ramfs | VFS 持返回的 `InodeRef` |
| block source registry lookup | mount admission / block registry | make-node 不参与 |
| FIFO/device/socket data plane | 各自后续领域 owner | make-node 不持 capability |

## 身份与能力模型

- make-node description 是 operation-local、filesystem-neutral capability；callback 返回后不得作为第二套
  behavior truth 保存在 syscall/VFS 层。
- `InodeRef` 标识 backend 已提交的 inode；dentry materialization 必须使用 callback 返回的同一 identity。
- `InodeType` 与 category-neutral numeric `rdev` 是两项正交的长期 metadata identity；provider handle、
  pathname、FileOps identity、typed registry key 与 raw packed integer都不能替代或合并它们。
- `InodeType` 是唯一 category truth；generic inode 不得缓存 `Char/Block` tag。typed key 只在确定 namespace 的
  registry call 生命周期内存在。
- unknown/unregistered device number 是合法 namespace identity，不代表 open capability。
- current open file description、fd flags、`O_NONBLOCK` 与 access mode 不进入 make-node request。

## 线性化点

- backend-local make-node的正常执行可见性边界是final inode metadata完成后进入parent directory-entry
  publication；同一backend锁覆盖metadata formation、publication call与local reference release，VFS随后按既有
  common-create协议materialize同一identity。这是runtime serialization，不是crash durability point。
- syscall success只能在backend commit与既有dentry handoff完成后返回；不得绕过handoff直接返回success。
- backend publication后的既有cache/dentry失败窗口不由本RFC承诺新的跨owner rollback；Stage 2不得扩大该窗口或
  增加touch/mkdir不存在的新fallible step。跨owner完整原子化与lwext4 strict failure/crash atomicity分别由登记
  问题和accepted limitation承接。
- ext4 reload 后的 kind/numeric `rdev` 来自 on-disk inode；不能由旧 dentry、旧 cache 或 provider registry
  重建。
- mount kind admission 在线读取 `Inode::ty()` 后、构造 `BlockDevNum` / block registry lookup 前完成；
  non-block result 在此确定为 `ENOTBLK`。

## 生命周期与 cleanup 规则

- 可在backend publication前判定的失败不得创建dirent；已分配但仍未链接的inode与backend-private resource必须
  尝试回收并传播cleanup失败。不得声称能够回滚lwext4内部已经partial dirty的directory block。
- successful unlink 只终止 namespace link；本 revision 不建立需要随 unlink 取消的 FIFO/device/socket state。
- ramfs resident node 随 ramfs inode/link lifetime 结束；ext4 on-disk node 由 ext4 unlink/eviction 规则管理。
- make-node request、credential snapshot 与 temporary decode state 不得逃逸出一次 operation。
- 本 revision 不创建、读取或缓存 process umask state；requested permission bits 直接随 operation-local request
  进入 backend。后续统一 umask cutover 前不得让任一局部 mask 成为并列 truth。
- unsupported open 不得通过 panic 代替错误 cleanup。

## 禁止退化项

- 不得实现 FIFO-only、char-only、ext4-only 或只消除 `ENOSYS` 的较弱 target。
- 不得把 unsupported kind 静默创建为 regular inode。
- 不得只在 cache 保存 `rdev` 而让 ext4 reload 丢失。
- 不得让 generic inode 同时保存 kind 与 typed Char/Block `rdev` category，或保留 raw packing escape。
- 不得按 filesystem name/pathname/LTP case 在 syscall 层分发。
- 不得让 backend 访问 task、fd table、raw Linux mode/capability 或 provider registry。
- 不得在 `mknodat` 内实现局部 umask、读取当前 `sys_umask` stub，或把 task/fs-state 与其它 create call sites
  拉入本 revision。
- 不得从 FileOps、provider type 或 path 反推并覆盖 inode kind/`rdev`。
- 不得把 success-but-open-panics 当成诚实 unsupported behavior。
- 不得以 named FIFO/device open/socket endpoint 复杂度为理由缩减 make-node creation target。
- 不得新建无真实长期 consumer 的 test app/validation facade。

## 完成证据

- `VFS-FILE-KIND-001` 经 source audit 证明保持 Preserve，无第二份 kind truth。
- `DEVICE-NUMBER-BASELINE-EXTRACTION` 已只提取 live 16/16 current rule；未提前发布 12/20 target。
- `DEVICE-NUMBER-001` 已在独立`DEVICE-NUMBER-CUTOVER`完成12/20 Refine，保持char/block namespace、
  TTY/device endpoint号码与publication lifecycle；后续独立gate已把make-node stage解析为Ready但未激活。
- `VFS-MAKE-NODE-001` 与 `VFS-SPECIAL-NODE-RDEV-001` 已在同一 `VFS-MAKE-NODE-CUTOVER` 进入 current contract。
- `VFS-MOUNT-ADMISSION-002` 已在同一 cutover 完成 `ENOTBLK` Refine。
- RV64/LA64 `mknodat(33)`、libc `mknod()` / `mknodat()`、node-kind/capability/dirfd/error matrix 有明确证据；
  permission proof 明确验证 requested bits 不经 process umask 屏蔽，不把该结果宣称为完整 Linux mode parity。
- ext4 与 ramfs 覆盖全部 in-target node kind；ext4 reload proof 覆盖 kind/permission/owner/`rdev`。
- stat/statx/getdents/unlink、regular ordinary I/O、special-node explicit unsupported open 与 mount `ENOTBLK`
  均有与 claim 相称的验证。
- partial create、duplicate、RO mount、bad path/dirfd、provider miss independence 与 no-panic source audit 闭合。
- source/lock-order证据证明final metadata先于publication且正常并发不可见中间态；可预先判定的失败无node、
  未链接inode执行cleanup。既有common-create窗口与lwext4 strict failure/crash atomicity的范围、无退化audit和
  独立disposition明确记录，不要求本RFC实现跨owner或lwext4 redesign。
- errno proof 覆盖 `ENOTDIR`、`EROFS`、backend `EPERM`、FIFO `EOPNOTSUPP`、device/socket `ENXIO`、
  mount `ENOTBLK` 与 block provider-miss `ENOENT` 的 owner boundary。
- capability proof 覆盖普通 character/block node 与 character 0:0，确认 R2 不存在 whiteout bypass。
- focused LTP、临时 `user-test` probe、KUnit、build、RV64/LA64 runtime 按实际运行状态分别记录；未运行项保持
  Not Run。
- 临时 validation probe 已删除，production code 不依赖测试路径、case name 或执行顺序。
- named FIFO data plane、device-node provider open、pathname socket endpoint 与 legacy `readdir` 明确保留在本
  revision 之外，不能由 make-node closure 冒充完成。
- process umask、`umask(2)` state ownership 与其它创建类调用点明确留给独立后续工作；current limitation 只有
  在共同 owner、全部 call-site cutover 与跨创建路径验证闭合后才能退出。
