# VFS Make Node 当前契约

**Contract ID：** `VFS-MAKE-NODE`
**状态：** Active
**Owner：** VFS make-node protocol / filesystem-backed special-node identity
**参与领域：** `mknodat` syscall adapter / VFS inode creation / ext4 / ramfs / stat and mount consumers
**覆盖范围：** filesystem-backed regular/FIFO/character/block/socket node creation、task filesystem-context umask adjustment、final metadata handoff、ext4/ramfs representation 与 special-node numeric `rdev`
**不覆盖：** POSIX default ACL、named FIFO/device/socket data plane、device provider resolution、legacy `readdir`、lwext4 任意 I/O failure/crash atomicity、既有 common-create cache/dentry publication window
**实现位置：** `anemone-kernel/src/fs/{api/mknodat.rs,vfs/ops.rs,inode,ext4,ramfs}`、`anemone-kernel/src/task/credentials/cap.rs`、`anemone-kernel/crates/anemos/lwext4-rust`、`anemone-rs/src/{sys,os}/linux/fs.rs`
**依赖：** `VFS-FILE-KIND-001`、`DEVICE-NUMBER-001`
**Pending Successor：** None
**最后核验：** 2026-08-01

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| raw Linux mode / `dev_t` / dirfd / capability admission | `mknodat` syscall adapter | VFS 只收到 normalized kind、permission 与 numeric `rdev` | Linux ABI 与错误分类 |
| process umask | task filesystem context | syscall adapter只读取一次mask snapshot；VFS/backend只收到调整后的permission | 与`openat`/`mkdirat`共享唯一mask truth |
| parent、mount/DAC 与 final semantic description | VFS make-node protocol | backend 只收到 `MakeNodeDescription` | common-create admission 与唯一 handoff |
| inode allocation、representation 与 dirent publication | ext4 / ramfs backend | VFS materialize callback 返回的同一 inode | backend-local create transaction |
| filesystem-backed numeric `rdev` | inode metadata；ext4/ramfs 分别拥有持久/resident representation | stat/mount consumer 只读投影 | special-node identity；不表示 provider 存在 |
| character/block category | immutable `Inode::ty()` | registry boundary 构造 typed key | 防止 numeric identity 成为第二份 kind truth |

## VFS-MAKE-NODE-001 — Make-node admission 与 backend publication 只有一个 handoff

**规则：** syscall adapter 只拥有 Linux `mknodat` 参数读取及 mode/`dev_t`/dirfd/`CAP_MKNOD` admission。VFS
唯一拥有 parent resolution、writable mount、directory DAC、共同创建策略与 dentry materialization，并通过
`InodeOps::make_node` 把 final kind、umask-adjusted permission、uid/gid 与适用 numeric `rdev` 交给 backend。backend
不得重新读取 task、fd table、raw Linux mode、capability 或 device registry。

ext4/ramfs 必须在各自 backend creation boundary 内先形成 final metadata，再串行化进入 dirent publication；
正常并发 lookup 不得观察中间 metadata。可预先判定的失败不得发布 node；确认仍未链接的失败分配必须尝试清理并
传播 cleanup failure。lwext4 任意内部 I/O failure/crash atomicity 与既有 common-create cache/dentry window
分别由已登记 limitation/open issue 拥有，不得用 forced flush、双状态或 validation hook 伪装为本规则的证明。

`mknodat`在syscall adapter内复用task filesystem context的唯一umask owner，对normalized requested permission
读取一次mask snapshot并形成final permission；不得在VFS、inode或backend建立第二份mask state。regular node使用
普通文件数据路径；FIFO返回`EOPNOTSUPP`，character/block/socket open返回`ENXIO`，本规则不引入这些special node
的数据面。

**违反表现：** backend 解析 raw mode 或查询当前 task/provider；callback 后补写驱动行为的 backend metadata；
publication 前可判定的失败留下可 lookup node；special open panic或落入 regular fallback；或长期保留 proof-only
production seam。

**验证 / Enforcement：** VFS/backend source 与 lock-order audit；RV64/LA64 各 293 项 boot KUnit；两架构 ext4
remount/reload 与 ext4/ramfs node-matrix probe；两架构 glibc/musl 各 11 个 `mknod*`/`mknodat*` case 全部通过；
删除临时 probe/profile 后的双架构 release build、initializer/residual-panic/provider-lookup audit 与 final review。

**最初来源：** Closed [VFS Make Node R2 RFC](../../rfcs/vfs-make-node/index.md) 与
[implementation transaction](../../devlog/transactions/2026-07-31-vfs-make-node.md)。

**当前来源：** R2 cutover与[umask文件创建掩码](../../devlog/changes/2026-07-27-umask-file-creation-mask.md)；
合流后的R3复用既有task filesystem-context owner，并退出branch-local no-umask limitation。

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
