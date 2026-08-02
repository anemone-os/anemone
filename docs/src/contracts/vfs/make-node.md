# VFS Creation 与 Make Node 当前契约

**Contract ID：** `VFS-CREATION` / `VFS-MAKE-NODE`
**状态：** Active
**Owner：** user-thread kernel creation operations / context-free VFS creation primitives / filesystem-backed special-node identity
**参与领域：** `openat` / `mkdirat` / `mknodat` / `symlinkat` syscall adapter、task filesystem context、VFS inode creation、ext4、ramfs、stat and mount consumers
**覆盖范围：** user-thread named-object creation的current-context admission/formation与context-free VFS handoff、filesystem-backed regular/FIFO/character/block/socket node creation、final metadata handoff、ext4/ramfs representation 与 special-node numeric `rdev`
**不覆盖：** POSIX default ACL、named FIFO/device/socket data plane、device provider resolution、legacy `readdir`、lwext4 任意 I/O failure/crash atomicity、既有 common-create cache/dentry publication window
**实现位置：** `anemone-kernel/src/fs/api/{creation.rs,openat.rs,mkdirat.rs,mknodat.rs,symlinkat.rs}`、`anemone-kernel/src/fs/vfs/ops.rs`、`anemone-kernel/src/fs/{inode,ext4,ramfs}`、`anemone-kernel/src/task/{fs.rs,credentials/cap.rs}`、`anemone-kernel/crates/anemos/lwext4-rust`、`anemone-rs/src/{sys,os}/linux/fs.rs`
**依赖：** `VFS-FILE-KIND-001`、`DEVICE-NUMBER-001`
**Pending Successor：** None
**最后核验：** 2026-08-02

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| raw Linux mode / flags / `dev_t` / dirfd | syscall adapter | kernel operation只收到normalized internal input | Linux ABI 与错误分类 |
| process umask | task filesystem context | kernel creation policy持有一次operation-local snapshot；VFS/backend只收到调整后的permission | 唯一mask truth与fork/`CLONE_FS`/exec生命周期 |
| current fsuid/fsgid/group/capability view | task credentials | kernel creation policy持有一次operation-local snapshot | pathname/DAC与inode formation使用同一调用者视图 |
| parent lookup、user-visible mount/DAC admission与final creation facts | 对应`kernel_*` user-thread operation | VFS primitive只收到显式parent/name/permission/uid/gid或完整description | process-context policy、errno ordering与namespace primitive的handoff |
| mount writable invariant recheck、backend/dentry invariant | context-free VFS creation primitive | backend只收到final semantic description并返回同一inode | internal caller不能绕过mount约束；common-create dispatch与materialization |
| inode allocation、representation 与 dirent publication | ext4 / ramfs backend | VFS materialize callback 返回的同一 inode | backend-local create transaction |
| filesystem-backed numeric `rdev` | inode metadata；ext4/ramfs 分别拥有持久/resident representation | stat/mount consumer 只读投影 | special-node identity；不表示 provider 存在 |
| character/block category | immutable `Inode::ty()` | registry boundary 构造 typed key | 防止 numeric identity 成为第二份 kind truth |

## VFS-CREATION-001 — Current-task creation policy 止于 kernel operation

**规则：** syscall adapter只读取用户参数并把raw Linux mode、flags、dirfd与`dev_t`规范化为内部输入。实际服务
用户线程的`kernel_*` creation operation读取一次operation-local credential与umask snapshot，并用同一个
`FsPermChecker`完成pathname search、directory DAC、capability admission与final inode formation。`kernel_*`可以
调用一个VFS operation，也可以组合多个context-free VFS primitive；命名不要求一对一转发。

umask只对真正创建对象的regular file、tmpfile、directory与make-node permission应用一次；打开已有对象不得应用，
symlink不得应用。non-directory SGID admission必须先检查调用者requested mode中的`S_ISGID|S_IXGRP`、parent SGID、
group membership与`CAP_FSETID`，再应用umask，避免umask清除group-execute后错误保留SGID。uid使用fsuid；gid在
parent SGID时继承parent gid，否则使用fsgid；directory与symlink保持各自既有special-bit语义。

creation-facing VFS primitive只消费显式parent/name/final permission/uid/gid/`MakeNodeDescription`，不得读取current
task、task filesystem context、current credentials、fd table、user pointer或raw Linux ABI。它仍拥有基于显式
mount/path/inode输入的writable、backend dispatch与dentry materialization不变量。kernel-internal pathname creator用
`vfs_*_as_root`表达exact root owner，或直接提交显式owner；其结果不受偶然current task的umask/credentials影响。

**违反表现：** creation-facing VFS helper调用`get_current_task()`、`FsPermChecker::for_current_fs()`或
`Task::mask_creation_perm()`；syscall与kernel/VFS两层重复应用umask；pathname lookup与formation使用不同credential
snapshot；umask使无权调用者保留SGID；internal creator因当前task变化得到不同owner/mode；或为满足命名形式增加
不拥有VFS invariant的转发层。

**验证 / Enforcement：** creation policy KUnit覆盖explicit owner/permission、fsuid/fsgid、parent SGID、group与
`CAP_FSETID` matrix、SGID-before-umask顺序和mknod capability；VFS KUnit证明explicit metadata handoff。RV64 327项与
LA64 332项boot KUnit全部通过；两架构temporary mknodat probe均验证requested `06777`在umask `0027`下得到`06750`，
probe随后删除。两架构glibc/musl的`umask01`、`mkdirat01/02`、`mknodat01/02`与`symlink01`通过；LA64及RV64 glibc
`creat09`在ext2/ext3验证无权SGID stripping，剩余运行缺口属于既有ext4 mount/userdb环境或相邻ABI限制。source
audit确认`anemone-kernel/src/fs/vfs`内上述current-context读取为零。

**最初来源：** [VFS/kernel creation boundary小迭代](../../devlog/changes/2026-08-02-vfs-kernel-creation-boundary.md)
及其同一原子cutover source/runtime evidence。

**当前来源：** 同最初来源。

## VFS-MAKE-NODE-001 — Make-node admission 与 backend publication 只有一个 handoff

**依赖：** `VFS-CREATION-001`

**规则：** syscall adapter只拥有Linux `mknodat`参数读取及mode/`dev_t`/dirfd normalization。kernel make-node
operation拥有`CAP_MKNOD`、parent lookup、writable mount、directory DAC、umask/owner/SGID formation；VFS exact
primitive拥有`InodeOps::make_node` dispatch与dentry materialization，并把final kind、permission、uid/gid与适用
numeric `rdev`组成的`MakeNodeDescription`交给backend。VFS/backend不得重新读取task、fd table、raw Linux mode、
capability或device registry。

ext4/ramfs 必须在各自 backend creation boundary 内先形成 final metadata，再串行化进入 dirent publication；
正常并发 lookup 不得观察中间 metadata。可预先判定的失败不得发布 node；确认仍未链接的失败分配必须尝试清理并
传播 cleanup failure。lwext4 任意内部 I/O failure/crash atomicity 与既有 common-create cache/dentry window
分别由已登记 limitation/open issue 拥有，不得用 forced flush、双状态或 validation hook 伪装为本规则的证明。

`mknodat`通过`VFS-CREATION-001`的kernel creation policy读取一次mask snapshot并形成final permission；不得在
syscall adapter、VFS、inode或backend建立第二份mask state。regular node使用普通文件数据路径；FIFO返回
`EOPNOTSUPP`，character/block/socket open返回`ENXIO`，本规则不引入这些special node的数据面。

**违反表现：** backend 解析 raw mode 或查询当前 task/provider；callback 后补写驱动行为的 backend metadata；
publication 前可判定的失败留下可 lookup node；special open panic或落入 regular fallback；或长期保留 proof-only
production seam。

**验证 / Enforcement：** R2 closure包括VFS/backend source与lock-order audit、RV64/LA64各293项boot KUnit、两架构
ext4 remount/reload与ext4/ramfs node-matrix probe，以及两架构glibc/musl各11个`mknod*`/`mknodat*`case。后续
creation-boundary refinement由`VFS-CREATION-001`所列source、KUnit与双架构runtime matrix共同约束。

**最初来源：** Closed [VFS Make Node R2 RFC](../../rfcs/vfs-make-node/index.md) 与
[implementation transaction](../../devlog/transactions/2026-07-31-vfs-make-node.md)。

**当前来源：** R2 cutover与后续独立的[umask文件创建掩码](../../devlog/changes/2026-07-27-umask-file-creation-mask.md)
建立最初baseline；[VFS/kernel creation boundary小迭代](../../devlog/changes/2026-08-02-vfs-kernel-creation-boundary.md)
于2026-08-02 Refine syscall/kernel/VFS handoff并完成runtime复验，不重开已关闭RFC。

## VFS-SPECIAL-NODE-RDEV-001 — Filesystem-backed special-node `rdev` 是单一 numeric truth

**规则：** ext4/ramfs 的 character/block node 在创建时一次确定 category-neutral numeric `rdev`。category 只由
immutable `Inode::ty()` 决定；generic inode metadata 不保存 character/block tag 或 packed Linux `dev_t`。
ext4 将该 number 编码进 on-disk inode并在 unmount/remount reload 后恢复；ramfs 在 resident inode lifetime 内
保持。stat/statx 与 mount admission 读取同一 number，并只在对应 consumer boundary 选择 typed namespace。

`rdev` 不表示 provider handle 或 provider 存在性。make-node 不查询 registry；未知号码仍可形成 namespace
identity。devfs 继续从 device owner 的 typed endpoint capability 投影 number，其 publication、open 与 lifecycle
不由本规则重新拥有。regular/FIFO/socket 的 raw `dev` 输入被 ABI adapter 忽略，不形成隐藏 identity。

**违反表现：** 首次 stat 正确但 reload 后 `rdev=0`；generic metadata 复制 kind category或保存 raw packing；
stat、mount、backend各读不同 value；provider registration 反向改写 inode；或未知 device number 无法创建。

**验证 / Enforcement：** canonical Linux codec/source audit；ext4 remount/reload probe验证 character/block kind、
mode、owner 与 representative `rdev`；ramfs resident matrix、stat/statx/getdents/unlink、unknown-provider creation 与
mount provider-miss separation probe；RV64/LA64 KUnit、release build 与 final owner review。

**最初来源：** Closed [VFS Make Node R2 RFC](../../rfcs/vfs-make-node/index.md) 与
[implementation transaction](../../devlog/transactions/2026-07-31-vfs-make-node.md)。

**当前来源：** 同最初来源；`VFS-MAKE-NODE-CUTOVER` 于 2026-08-01 生效。
