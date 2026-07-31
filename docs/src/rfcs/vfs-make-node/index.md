# RFC-20260731-vfs-make-node

**状态：** Accepted for Implementation
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-08-01
**领域：** fs / VFS / syscall ABI / ext4 / ramfs
**事务日志：** [2026-07-31-vfs-make-node](../../devlog/transactions/2026-07-31-vfs-make-node.md)
**影响契约：** Preserve `VFS-FILE-KIND-001`、`TTY-ENDPOINT-001`；`DEVICE-NUMBER-001` Refine已Effective；
`VFS-MOUNT-ADMISSION-002` Refine与`VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001` Introduce仍Not Cut Over
**开放问题：** None active；[Tracking Issues](./tracking-issues.md) 保留本轮四个 Keter、两个 Euclid 的
Neutralized 历史
**下一步：** [Stage 1](./implementation.md#5-stage-1-closed--device-number-prerequisite)与
`DEVICE-NUMBER-CUTOVER`已关闭；当前停止，Stage 2保持Outline / Not Active，不自动进入`1 -> 2` gate

> 本目录自 2026-07-31 起是 `vfs-make-node` 提案与 target 的公共 canonical source。2026-08-01 的独立复审
> 接受 R0；Stage 1 已关闭并使`DEVICE-NUMBER-001`生效，其余target contract仍须在各自cutover gate后才生效。

## 摘要

本 RFC 为 Anemone 增加完整的 filesystem node 创建能力。当前 RV64 与 LA64 使用 asm-generic syscall ABI，
因此 canonical kernel entry 是 `mknodat(2)`；libc `mknod(2)` 通过 `mknodat(AT_FDCWD, ...)` 获得同一用户
能力，不为当前架构虚构 legacy `mknod` syscall number。

VFS 在 `InodeOps` 虚表中新增 `make_node` function pointer。syscall adapter 负责 Linux `mode`、`dev_t`
与 `dirfd` admission；VFS 负责 pathname、parent、mount writable、目录权限与共同创建策略；ext4 和 ramfs
负责原子建立 namespace entry 与真实 inode metadata。成功结果必须能被 lookup、`stat` / `statx`、
`getdents64` 和 unlink；ext4 必须在 reload 后恢复同一 kind、permission、owner 与适用的 `rdev`，ramfs
必须在 resident lifetime 内保持同一 identity。

R0 明确接受一项 Linux 可见偏差：本 revision 的 `mknodat` 不应用 process umask，最终 permission 直接使用
调用者请求的 permission bits。真正的 umask 需要 task/fs-state owner 与全部创建类调用点共同接入，必须由后续
独立工作统一实现；本 RFC 不为 `mknodat` 建立局部 mask、读取 `sys_umask` stub，或借 Stage 2 扩大 task/create
surface。因此下文的 Linux compatibility claim 只覆盖 node-kind、`dev_t`、dirfd、capability 与 errno matrix，
不包含 umask-adjusted permission semantics。

本 RFC 不把 named FIFO 数据面、按设备号 open provider 或 pathname socket endpoint 混入 make-node 核心。
首版只顺带接入不改变既有 owner 的现有能力：regular node 复用 ordinary regular-file behavior，device node
向 metadata ABI 投影真实 `rdev`，character node 作为 mount source 时由现有 mount admission 返回
`ENOTBLK`。用户态证明优先使用现有 LTP runner；不新增长期 test app。

为了让 Linux `mknodat` 的设备号 ABI 在自然边界闭合，本 RFC 接受一个先于 make-node 的 device-owned
prerequisite：内部公共数值域扩展为 12-bit major + 20-bit minor，通用 inode metadata 只保存
category-neutral `DeviceNumber`；`Inode::ty()` 单独决定 character/block namespace，只有进入 char/block
registry 时才转换为 `CharDevNum` / `BlockDevNum`。这会调整此前临时的 16/16 `DeviceId` 表示，但不改变
devfs/TTY/block endpoint 的 publication owner、既有号码或 provider lifecycle。

## 背景

当前 VFS 已有大部分 make-node 所需基础：

- `InodeType` 已区分 Regular、Fifo、Char、Block、Socket 等真实 UAPI kind；
- [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)
  已规定 immutable `Inode::ty()` 是唯一 file-type truth；
- `InodeStat`、Linux `stat` / `statx` projection 与 `DeviceId` 已能表达 `rdev`，但
  `DeviceId::Char/Block` 与 `Inode::ty()` 重复保存 category，`DeviceId::Raw` 又绕过结构化设备号；
- `device::devnum` 当前使用 16-bit major + 16-bit minor，而 Linux `mknodat` 的 32-bit encoded device
  number 可表示 12-bit major + 20-bit minor；现有最大 static major 2048 仍在新范围内；
- `GeneralMinorAllocator` 当前是单调 counter + sparse `BTreeSet` reserve，不会因 minor domain 扩展到 20 bit
  直接分配一张百万项 bitmap；
- VFS 已有 parent resolution、writable-mount gate、directory permission check、owner/group initialization 与
  dentry materialization；
- ext4 能分配多种 on-disk inode type，底层 lwext4 已有 `ext4_inode_get_dev()` /
  `ext4_inode_set_dev()`，Rust wrapper 暴露该 adapter 是自然实现面，不需要发明第二种 on-disk identity；
- ramfs 已有 resident inode/dirent transaction；
- mount admission 当前通过 `DeviceId::Block` 区分 block source并取得 provider，这会让 `rdev` category
  成为 `Inode::ty()` 之外的第二份行为 truth；
- anonymous pipe 已提供 pipe read/write/readiness 数据面，但只支持创建时同时返回固定 rx/tx endpoint。

缺口也很集中：

- `InodeOps` 尚无 make-node operation；
- RV64/LA64 syscall registry 尚无 asm-generic `mknodat(33)` handler；
- ext4/ramfs 不能通过统一 VFS 边界创建 FIFO、character、block 与 socket namespace node；
- ext4 与 ramfs 的 ordinary inode attributes 当前都把 `rdev` 报告为 `None`；
- ext4 special-node open 存在 `unimplemented!()`，ramfs 对非 ordinary kind 的 open 进入 `unreachable!()`；
- mount 对 non-block source 当前返回 `EINVAL`，而 Linux/LTP 对 character node source 要求 `ENOTBLK`；
- `sys_umask` 当前仍是 stub；R0 接受 `mknodat` 不应用 process umask。真正的 mask state 与所有创建类调用点
  由后续独立工作统一收口，本 RFC 只复用现有 owner/group 与 create admission，不读取该 stub 或复制局部 mask。

活动登记册中的
[`ANE-20260527-LTP-MKNOD-LEGACY-READDIR`](../../register/open-issues.md#ane-20260527-ltp-mknod-legacy-readdir)
把 `read03` 的 `mknod(S_IFIFO)` setup 与 legacy `readdir` 缺口记录在同一旧条目中。本 RFC 只收口
make-node 能力；legacy `readdir` 不属于本 target。`read03` / `write04` 在 node 创建之后还要求完整 named
FIFO open/I/O，因此也不能仅凭 make-node closure 直接关闭整个旧条目。

Linux node-kind、`dev` 与 capability admission 以固定公共参考
`xref:linux-6.6.32:fs/namei.c#do_mknodat`、`xref:linux-6.6.32:fs/namei.c#may_mknod` 和
`xref:linux-6.6.32:fs/namei.c#vfs_mknod` 为 ABI 证据；上游实现形状不定义 Anemone 内部 owner 或类型。

## 目标

- 在 RV64 与 LA64 注册 asm-generic `mknodat(33)` syscall，使 libc `mknod()` 与 `mknodat()` 共享同一
  kernel capability。
- 固定新增 `InodeOps::make_node` function pointer，作为 directory inode 创建非目录 filesystem node 的
  backend owner surface。
- 支持 type bits 为 0、`S_IFREG`、`S_IFIFO`、`S_IFCHR`、`S_IFBLK` 与 `S_IFSOCK` 的 Linux
  make-node admission；`S_IFDIR` 返回 `EPERM`，`S_IFLNK` 与其它非法 type bits 返回 `EINVAL`。
- 明确本 revision 不应用 process umask；ext4/ramfs 的最终 permission bits 使用 `mode` 中请求的 permission
  bits，不能读取 `sys_umask` stub、task-local mask 或建立 mknod-local mask state。
- 对 character/block creation 执行 `CAP_MKNOD` gate；R0 不采用 Linux `WHITEOUT_DEV` 的 capability 例外，
  character 0:0 仍要求 `CAP_MKNOD`；regular/FIFO/socket 不借此要求设备创建 capability。
- 将 syscall 的 32-bit encoded device number 完整 decode 为 12-bit major + 20-bit minor；`dev` 只对
  character/block node 形成 category-neutral `DeviceNumber`，其它支持 kind 忽略 `dev`。
- 保持 char/block registry namespace 分离：`Inode::ty()` 是 category 的唯一 truth，registry consumer
  boundary 才把同一 numeric pair 转换为 `CharDevNum` / `BlockDevNum`。
- 让 ext4 与 ramfs 都成为首版 in-target writable filesystem；devfs、procfs 与其它 pseudo filesystem
  不转为用户可写 namespace。
- 使 ext4/ramfs 创建结果具有真实 immutable kind、permission、owner 与适用的 `rdev`；ext4 在
  eviction/reload 或 remount/reload 后保持这些属性。
- 保证失败不发布半初始化 namespace entry；成功不依赖 device provider 已注册，也不把 provider handle
  保存进普通 filesystem inode。
- 让 regular node 直接获得现有 ordinary regular-file behavior，不建立第二套 regular backend。
- 让未纳入数据面的 special node open 明确返回错误，不 panic、不退化成 regular file。
- 复用现有 mount admission：character node 作为 block-backed filesystem source 时返回 `ENOTBLK`，不进入
  block registry lookup。
- 通过现有 `user-test` runner 的 focused LTP 与必要的临时 probe 提供真实用户态证据，不新增长期 test app、
  test target 或 validation facade。

## 非目标

- 不实现 named FIFO 的 open rendezvous、reader/writer membership、blocking/nonblocking wait、buffer、
  EOF/`EPIPE`、poll/readiness、signal cancellation 或 unlink-while-open lifecycle。
- 不新增普通 filesystem `kind + rdev -> provider` resolver，不实现 character/block/TTY/console device node
  的成功 open。
- 不实现 pathname Unix socket 的 bind/connect、endpoint identity、message data plane 或 cleanup。
- 除共同数值域从临时 16/16 扩展为 12/20、移除通用 inode 中重复的 category/Raw 表示外，不改变 devfs
  publication registry、producer-owned allocation policy、hotplug、unpublish、provider replacement 或既有
  endpoint 号码。
- 不实现 overlayfs whiteout identity、copy-up/rename whiteout protocol，或为 character 0:0 绕过
  `CAP_MKNOD`。
- 不实现 legacy `readdir` syscall，也不把它与 `mknodat` 共用 closure claim。
- 不实现 process umask state、`umask(2)` 行为或其它创建类 syscall 的 mask 接入；这需要 task/fs-state owner
  与全部 create call sites，由后续独立工作统一完成。
- 不把 current create policy 尚未统一具备的其它 Linux corner case 复制成 mknod-local 特判；如果 common VFS
  owner 无法满足本 revision 的 owner/group/permission target，必须在 implementation resolution 上报
  shared-surface expansion。
- 不为未来 filesystem、device class 或特殊文件数据面建立通用 factory/framework。
- 不新建专用 `mknod-test` app，或把临时 `user-test` probe 沉淀成 production API。

## 文档地图

RFC target：

- [目标和不变量](./invariants.md)
- [Implementation plan](./implementation.md)：一个 device-number prerequisite + 一个 make-node 主实现阶段；
  Stage 1 Closed，Stage 2 Outline / Not Active
- [Tracking Issues](./tracking-issues.md)：保存当前影响 target / implementation readiness 的 finding
- [No-umask accepted limitation](../../register/current-limitations.md#ane-20260801-vfs-make-node-no-umask)：
  本 revision 的 requested permission bits 不经 process umask 屏蔽
- [背景材料索引](./backgrounds/index.md)：历史定位材料，不是 accepted target 或 current contract

Current contracts：

- [`VFS-FILE-KIND-001`](../../contracts/vfs/file-kind.md#vfs-file-kind-001--inode-kind-是唯一-file-type-truth)：
  Preserve；immutable inode kind 继续是唯一 file-type truth。
- [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md#device-number-001--1220-category-neutral-device-number-domain)：
  Refine已由`DEVICE-NUMBER-CUTOVER`生效；当前contract记录12/20 common domain、category-neutral generic inode
  number、typed registry boundary与canonical Linux codec，Stage 2只读复用该baseline。
- [`TTY-ENDPOINT-001`](../../contracts/tty/data-plane.md#tty-endpoint-001--endpoint-publication是稳定的单向transaction)：
  Preserve；`ttyS<N>` major 4/minor `64+N` 与 console 5:1 均不改变。
- [`VFS-MOUNT-ADMISSION-002`](../../contracts/vfs/mount-admission.md#vfs-mount-admission-002--source-kind-owned-admission)：
  Refine；保持 source-kind owner，仅补齐 non-block source 的 `ENOTBLK` visible errno。
- `VFS-MAKE-NODE-001`：Proposed Introduce；尚未 cut over。
- `VFS-SPECIAL-NODE-RDEV-001`：Proposed Introduce；只覆盖 filesystem-backed special node 的
  `rdev` persistence/projection，不重新拥有 devfs publication 或 provider lifecycle。

公共外部源码证据：

- `xref:linux-6.6.32:fs/namei.c#do_mknodat`
- `xref:linux-6.6.32:fs/namei.c#may_mknod`
- `xref:linux-6.6.32:fs/namei.c#vfs_mknod`

## 修订记录

| 修订 | 日期 | 状态 | 语义变化 | Review / 事务 |
| --- | --- | --- | --- | --- |
| R0 | 2026-08-01 | Accepted for Implementation | 初始 accepted target：12/20 device-number prerequisite、canonical `mknodat`、ext4+ramfs make-node closure；明确 requested permission bits 不应用 process umask | [独立 R0 复审与 activation](../../devlog/transactions/2026-07-31-vfs-make-node.md#r0-acceptance-and-stage-1-activation-preflight---2026-08-01) |

## 方案

### Syscall ABI 与 canonical entry

当前两种目标架构使用 asm-generic syscall ABI，只注册 `SYS_MKNODAT = 33`。syscall adapter 接收
`dirfd`、pathname、Linux mode 与 raw `dev_t`，完成用户指针读取、type admission、`CAP_MKNOD`、绝对/相对
路径与 `AT_FDCWD` 规则，然后把 filesystem-neutral 创建请求交给 VFS。用户程序调用 `mknod()` 时由 libc
走同一 `mknodat(AT_FDCWD, ...)` 入口；本 RFC 不增加当前架构 ABI 不存在的 direct legacy syscall。

ABI matrix 固定如下：

| Type bits | 创建语义 | `dev` | capability |
| --- | --- | --- | --- |
| 0 / `S_IFREG` | empty ordinary regular node | ignored | ordinary create admission |
| `S_IFIFO` | FIFO-kind namespace node | ignored | ordinary create admission |
| `S_IFCHR` | character device node | decode 为 12/20 `DeviceNumber`；category 来自 kind | effective `CAP_MKNOD`；character 0:0 无 whiteout 例外 |
| `S_IFBLK` | block device node | decode 为 12/20 `DeviceNumber`；category 来自 kind | effective `CAP_MKNOD` |
| `S_IFSOCK` | socket-kind namespace node | ignored | ordinary create admission |
| `S_IFDIR` | reject，不创建 | 不消费 | `EPERM` |
| `S_IFLNK` / 其它非法 type bits | reject，不创建 | 不消费 | `EINVAL` |

Linux `vfs_mknod()` 为内部 whiteout 路径保留 `S_IFCHR + WHITEOUT_DEV` capability 例外；本 R0 没有
overlayfs/whiteout owner 与 lifecycle，因此不接受该例外。用户态 character 0:0 与其它 character node 一样，
缺少 effective `CAP_MKNOD` 时返回 `EPERM`，不得把它解释成 whiteout 或无特权 special node。

pathname、duplicate name、bad pointer、bad relative `dirfd`、non-directory `dirfd`、symlink loop、read-only
mount 与 parent DAC failure 由现有 syscall/VFS owner 返回 Linux-compatible errno。validation order 的精确代码
形状属于 implementation resolution，但不能用 backend error 或成功空操作绕过 ABI admission。

permission bits 不采用 Linux 的 umask-adjusted 结果：syscall adapter/VFS 把调用者请求的 permission bits 原样
纳入最终 node description，backend 持久/驻留保存该结果。该偏差必须保持可见且可测试；不得让当前
`sys_umask` stub 的返回值偶然驱动行为，也不得在本 RFC 内补一份 task-local 或 mknod-local umask owner。

syscall 的 `dev` 参数按 Linux 32-bit encoded device number 解释；全部 bit pattern 都能 round-trip 为
12-bit major + 20-bit minor。Linux packed layout 只存在于 `mknodat` decode、`stat`/`statx` projection、loop
ioctl 与 ext4 on-disk codec 等明确边界，不能成为通用 inode/registry 的内部 raw packing，也不能以新的
`DeviceId::Raw`-like escape variant 代替结构化 identity。

### VFS 与 filesystem handoff

`InodeOps` 新增 `make_node` function pointer。语义上它接收 directory inode、leaf name 与已经由 VFS
admission 的窄 node description，返回完整 `InodeRef`。description 只表达 kind、permission/owner 结果与
适用的 `rdev`；它不携带 task、fd table、Linux raw mode、open flags、FIFO endpoint 或 device provider。

精确 Rust 类型、参数排列和 regular node 是否在 VFS 内复用 `touch` 属于 implementation preference；但
`InodeOps::make_node` 的落点与命名、filesystem-neutral request、以及 filesystem 不重新解释 syscall policy
是 accepted owner boundary。

VFS 在 callback 前完成 parent resolution、writable-mount gate、directory permission 与共同 create policy，
并在 callback 成功后只 materialize callback 返回的同一 inode。filesystem callback 必须在其 creation
boundary 内完成 inode allocation、最终 metadata 与 directory entry 的原子 publication；具体使用 transaction
还是显式 rollback 属于 implementation preference。失败时不留下可 lookup node，成功时不得要求 VFS 猜测
backend-private type 或回填第二份 `rdev` truth。

### Kind、`rdev` 与 backend coverage

`Inode::ty()` 继续唯一拥有 immutable kind。通用 inode metadata 对 character/block node 只保存一个
immutable、category-neutral `DeviceNumber(major, minor)`；它不再用 `DeviceId::Char/Block` 复制 node kind。
exact representation 可以是 `Option<DeviceNumber>` 或只含 `None/Number` 的 wrapper，但不能增加 raw escape
variant。`stat`、`statx`、mount admission 与未来另行接受的 resolver 读取同一 numeric truth，并以
`Inode::ty()` 选择 namespace。make-node 不查询 provider registry，未知或尚未注册的 device number 也可以
形成 namespace node。

`CharDevNum` / `BlockDevNum` 继续作为两个 registry domain 的独立 key；devfs 从 typed endpoint capability
投影 numeric pair并验证 node kind，一般 filesystem 则在 consumer boundary 根据 inode kind 构造对应 key。
二者都不得让 registry key 反向成为通用 inode 的第二份 category truth。

ext4 必须编码并 reload Regular、Fifo、Char、Block 与 Socket kind，以及 character/block numeric `rdev`。
ramfs 必须在其 resident inode lifetime 内持有同一 kind/`rdev`。两者都必须支持 lookup、stat/statx、
getdents64 与 unlink。pseudo filesystem 的 directory inode ops 以 `EPERM` 拒绝 make-node，不获得可写
make-node registry；non-directory parent 在 namei/VFS 边界以 `ENOTDIR` 拒绝。

### 后续 operation 边界

Regular node 使用现有 regular file operations。FIFO、character、block 与 socket node 的创建成功只承诺
namespace/metadata 能力；本 revision 不承诺对应数据面。visible errno 固定如下：

| 场景 | errno | Owner / 原因 |
| --- | --- | --- |
| parent 不是目录 | `ENOTDIR` | namei/VFS parent admission |
| target mount 只读 | `EROFS` | VFS mount writable admission |
| writable directory backend 没有 make-node capability | `EPERM` | 对齐 Linux 缺少 mknod inode operation 的拒绝 |
| 已创建 FIFO node，但 named-FIFO data plane 尚未实现 | `EOPNOTSUPP` | 明确表示 operation/data plane 不受支持 |
| 已创建 character/block ordinary-filesystem node，但尚无 device resolver | `ENXIO` | identity 合法但没有可连接 provider route |
| 已创建但未绑定 endpoint 的 socket-kind node | `ENXIO` | namespace metadata 不等价于 socket endpoint |
| block-backed mount source 不是 block inode | `ENOTBLK` | 在 registry lookup 前由 mount source-kind admission 决定 |
| source 是合法 block inode、但号码没有注册 provider | 保持当前 provider-miss errno（当前为 `ENOENT`） | 不与 kind mismatch 合并 |

实现必须消除 `unimplemented!()` / `unreachable!()`、错误 provider binding 与 regular-file fallback。为
`ENOTBLK` 增加 system-wide semantic error 到 errno 的窄映射属于本 target 的必要实现面；内部 variant 命名
仍是 implementation preference。

mount admission 是唯一首版 companion integration。block-backed filesystem source 先检查
`inode.ty() == Block`，再读取 numeric `rdev` 并在 registry boundary 构造 `BlockDevNum`；character、regular、
FIFO、socket 或无 `rdev` source 在 registry lookup 前返回 `ENOTBLK`。合法 block number 才进入现有 block
registry lookup。该变化不把 mount owner 移入 make-node，也不产生 device-open resolver。

### 用户态证明

首选证据是现有 `user-test` runner 下 focused `mknod*`、`mknodat*` 与 `mount02` LTP。LTP 未覆盖的精确
`st_rdev` / `statx` projection、ramfs coverage 或 ext4 reload 可以通过临时修改现有 `user-test` 加入最小
probe；probe 在对应 cutover closure 前删除，执行逻辑、观察结果与删除事实进入 transaction evidence。

KUnit 与 source audit 应覆盖 mode/`dev_t` codec、capability gate、backend rollback、kind/rdev projection 与
禁止 panic，但不能替代至少一条真实用户态 syscall 路径。首个 prerequisite 的命令、manifest 与停止条件已在
[implementation plan](./implementation.md) 冻结；make-node 主阶段的精确 profile、probe 和命令由独立
`1 -> 2 Implementation Resolution Gate` 在 Stage 1 关闭后解析。

## Contract Impact

规范性contract delta、owner与cutover条件见[目标和不变量](./invariants.md#contract-impact)。公开提升gate最初从
live source docs-only提取16/16 `DEVICE-NUMBER-001` baseline；2026-08-01独立`DEVICE-NUMBER-CUTOVER`已完成12/20
normalization、通用inode category-neutral number与既有consumer迁移。最终
`VFS-MAKE-NODE-CUTOVER` 再原子 Introduce filesystem-backed make-node/`rdev` 并 Refine mount `ENOTBLK`。
任一 gate 失败时不得提前把 target 写入 current contract。

## 接受边界

接受本 RFC 意味着以下 target 被固定，但不自动授权实现：

- RV64/LA64 canonical `mknodat(33)` 与上述 mode/capability matrix，包括 R0 不采用 whiteout 例外；
- 本 revision 的 requested permission bits 不经 process umask 屏蔽；这是已接受的可见限制，Linux mode
  compatibility claim 不覆盖 umask-adjusted permission，后续由独立 umask 工作统一退出；
- 完整 Linux 32-bit device-number ABI、12-bit major + 20-bit minor internal numeric domain，以及 packed
  representation 只留在 ABI/on-disk boundary；
- `Inode::ty()` 唯一决定 Char/Block category，通用 inode `rdev` 只保存 category-neutral number，typed
  `CharDevNum` / `BlockDevNum` 只作为 registry-domain key；
- `InodeOps::make_node` owner surface；
- ext4 + ramfs backend coverage；
- immutable kind/`rdev`、atomic create/rollback 与 ext4 reload；
- regular behavior、metadata ABI 与 mount `ENOTBLK` companion integration；
- FIFO/device/socket 数据面不在本 revision；
- pseudo-fs make-node、unsupported special-node open、mount kind mismatch 与 provider miss 的上述 errno matrix；
- 不新增长期 test app。

以下内容是 implementation preference，可在保持 target 时由后续 Ready stage 解析：

- node description 的 Rust 类型名、字段布局与参数排列；
- category-neutral representation 采用 `Option<DeviceNumber>`、`DeviceId::{None, Number}` 或等价窄 wrapper；
- ext4/ramfs 中 numeric `rdev` 的 private storage shape，以及 typed registry key 的 conversion helper 形状；
- regular node 内部复用 `touch` 还是统一经过 `make_node`；
- ext4/lwext4 adapter、ramfs private type、锁与 transaction 的具体代码形状；
- KUnit、focused LTP 与临时 `user-test` probe 的精确命令和分组。

以下变化必须回到 RFC review / Target Renegotiation Gate，不能在实现中自然改写：

- 删除任一已接受 node kind，或把 ext4/ramfs 任一 backend 移出首版；
- 改变 `InodeOps::make_node` owner surface，向 filesystem 泄露 task/syscall/provider state；
- 允许 success 后 kind/`rdev` 无法 stat 或 ext4 reload；
- 缩小 12/20 device-number 范围、截断/拒绝合法 Linux encoding、增加 raw escape，或重新让 `rdev`
  category 与 `Inode::ty()` 并列驱动行为；
- 放宽失败原子性，接受 cache-only node、半初始化 dirent 或 open-time panic；
- 把 named FIFO、device provider open 或 pathname socket endpoint 并入本 revision；
- 改变本 revision 已接受的 device-number numeric owner，或进一步改变 devfs publication、mount source、
  provider lifecycle 或 opened-description owner；
- 在本 RFC 内引入 task/fs-state umask owner、读取 `sys_umask` stub 或扩展其它创建类调用点；
- 新增长期 test app/validation facade，或让临时 probe 成为 production dependency。

## 备选方案

### FIFO-only 或只消除 `ENOSYS`

拒绝。它不能形成通用 make-node capability，也无法证明 character/block `rdev`、socket node、backend
reload 或错误边界。单个 LTP setup 成功不能替代完整 node creation target。

### 首版同时完成所有 special-file 数据面

延期。named FIFO 需要 access/status-aware open、reader/writer membership 与 wait/lifecycle；device node open
需要中立 resolver 与 registry/lifetime review；pathname socket 需要 bind/connect owner。三者都超出
make-node 的 namespace/metadata 核心。

### 只支持 ext4

拒绝。`InodeOps::make_node` 是 VFS/backend owner surface；同时覆盖现有普通 writable ramfs 可以证明 API
不依赖 ext4 on-disk 表示，且无需引入新的外部 subsystem。

### filesystem 直接解析 raw `mode` / `dev_t`

拒绝。它会让 Linux ABI、capability 与 type admission 分散到 ext4/ramfs，并制造 backend-specific errno 与
kind truth。filesystem 只接收 VFS semantic description。

### 新增专用 test app

拒绝。现有 `user-test` + LTP 已经提供用户态 syscall runner；coverage 缺口可用有删除 gate 的临时 probe
补齐。长期 app 会增加没有 production consumer 的 artifact，而不会提高核心 target。

## 风险

- ext4 device encoding 若只改 create、不改 reload/getattr，会产生“第一次 stat 正确、eviction 后归零”的
  split truth。控制方式是把 create、reload、stat/statx 与 remount/eviction proof 绑定到同一 cutover。
- device number 若只扩大 allocator 常量、不迁移 stat/loop/ext4/devfs/TTY/block consumer，会形成多种 raw
  packing 和范围判断。控制方式是先用独立 `DEVICE-NUMBER-CUTOVER` 原子迁移既有 consumer，并保留 typed
  registry namespace 与 endpoint 号码。
- ext4/ramfs 当前 special-node open 含 panic 路径。控制方式是在 creation 对用户可见前把所有 in-target kind
  的 unsupported open 改为显式错误，并做 exhaustive source audit。
- owner/group/permission 如果在 backend dirent commit 后才补写，会出现 lookup 可见的半初始化 metadata。
  live common create path 当前在 backend callback 后设置 owner，ext4 owner persistence 也尚未完整接线；
  控制方式是 make-node stage 的 Ready resolution 把 final uid/gid 纳入 callback 前的 semantic description，
  并让 backend 在单一 transaction 中持久提交最终 node，而不是接受 post-commit patch-up。
- `S_IFSOCK` 容易被误读成 pathname socket endpoint。控制方式是 target 只承诺 socket-kind namespace
  metadata，bind/connect 留给 socket owner。
- mount `ENOTBLK` 若在 registry lookup 之后选择，会把 kind error 错分为 provider miss。控制方式是先检查
  `Inode::ty() == Block`，再把 numeric `rdev` 转为 `BlockDevNum` 查询 registry。
- focused LTP 可能被 test-device、credential 或 filesystem setup 阻塞。控制方式是按 case 记录 PASS、FAIL、
  TCONF、BROK 与 Not Run，并用窄临时 probe 只补目标内缺口。

## 收口

R0已接受；Stage 1与`DEVICE-NUMBER-CUTOVER`已关闭，`DEVICE-NUMBER-001`现为effective 12/20 baseline。
`VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`、`VFS-MOUNT-ADMISSION-002` Refine与
`VFS-MAKE-NODE-CUTOVER`仍Not Cut Over。当前必须在此停止，不自动解析或启动Stage 2。

最终 RFC closure 至少需要：

- RV64/LA64 `mknodat` ABI 与 libc `mknod`/`mknodat` 用户路径证据；
- ext4、ramfs 各 node-kind create/stat/getdents/unlink 证据；
- ext4 kind/permission/owner/`rdev` reload 证据；
- invalid mode、CAP_MKNOD（包括 character 0:0 无 whiteout 例外）、bad dirfd/path、duplicate、RO mount 与
  partial-create rollback 证据；
- regular-node ordinary I/O、special-node explicit unsupported open 与 no-panic audit；
- character source `mount02` / `ENOTBLK` 证据；
- focused KUnit/source audit、目标架构 build 与用户态 runtime 分开记录；
- docs-only baseline extraction、`DEVICE-NUMBER-CUTOVER` 与 `VFS-MAKE-NODE-CUTOVER` 分别记录 affected
  contract IDs 的 Effective / Not Cut Over 结果，前一 gate 未关闭不得合并声称后续 closure；
- 活动登记册按真实 closure 拆分或更新：make-node 已关闭不自动表示 named FIFO I/O 或 legacy `readdir` 已关闭。
