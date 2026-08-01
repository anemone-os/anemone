# VFS Make Node 能力边界共识

**状态：** Superseded / RFC Background / Not Normative
**最后更新：** 2026-07-31
**范围：** VFS 引入 `mknod` / `mknodat` 能力前的 target positioning

## 文档目的与边界

本文保存 make-node Draft 形成前后的能力边界推理。当前 normative public target 已折回
[RFC Draft](../index.md) 与 [目标和不变量](../invariants.md)；本文不是 RFC canonical target、current contract、
invariants 或 implementation plan。实施顺序与 write set 已另行写入 [implementation plan](../implementation.md)；
本文本身不授权代码实现、公共文档发布、transaction 建立、contract cutover 或任一实施阶段启动。

Draft 已固定 `InodeOps::make_node` owner surface、完整 12/20 device-number ABI、category-neutral inode number、
ext4+ramfs closure 与 visible errno；implementation plan 已冻结 device-number prerequisite 的首个 Ready write set，
make-node 主阶段仍保持 Outline，精确 Rust 请求类型、函数签名、rollback、manifest 与测试命令待独立 resolution。
后续 RFC review / implementation resolution 仍须重新读取 live source，且 `Ready` 不自动获得执行授权。

## 当前共识结论

这份 RFC 的核心是**提供创建 filesystem node 的能力**，而不是一次性补齐所有 special file 的数据面。

- syscall 边界向用户态提供 `mknod` / `mknodat`；
- VFS 在 `InodeOps` 虚表中新增名为 `make_node` 的 function pointer；
- filesystem 接收已经完成 VFS admission 的窄 node description，并原子建立 namespace entry 与 inode
  metadata；
- 成功创建的节点能够被 lookup、`stat` / `statx`、`getdents64` 和 unlink，并在需要持久化的
  filesystem 上 reload 后保持同一 kind、permission、owner 与 `rdev`；
- regular、FIFO、character、block 与 socket node 的创建不能因其后续 open/data-plane 尚未实现而退化成
  regular file、空壳假成功或只服务某个 LTP setup 的特判。
- syscall 的完整 32-bit Linux encoded device number 由 device-owned 12-bit major + 20-bit minor domain 承接；
  generic inode 只保存 category-neutral number，`Inode::ty()` 是 Char/Block category 的唯一 truth；
- typed `CharDevNum` / `BlockDevNum` 保持 registry-domain separation，只有 consumer boundary 才从同一 numeric
  pair 构造，不重新进入 generic inode；
- 提升前先提取 live 16/16 `DEVICE-NUMBER-001` current baseline，implementation 再独立完成 12/20 cutover；
  make-node 不借此重新拥有 devfs/TTY/block publication 或 provider lifecycle。

这里的“能力不能退让”指 make-node 创建协议本身必须完整、诚实、可由用户态观察并通过真实 filesystem
证明；它不等于 `mknod` 同时拥有 FIFO I/O、设备 provider lookup 或 pathname socket endpoint。
这些是节点被后续 operation 消费时的另一条能力边界。

因此，首版不采用 FIFO-only、只让 syscall 不再返回 `ENOSYS`、或只创建 cache 中临时 inode 的较弱
target。反过来，也不把“所有 special node 创建后立刻可 open 并完成完整数据面”写成 make-node RFC 的
默认 acceptance boundary。

## 核心能力的准确含义

### 内核内部边界

make-node 的 owner surface 与命名现在已经固定：它是新增的 `InodeOps::make_node` function pointer。
目录 inode 的实现负责创建 child，非目录 inode 沿现有虚表惯例返回明确错误。它与现有 `touch`、`mkdir`、
`symlink` 同属 inode/目录创建边界，不上移到 filesystem-type ops，也不另建并列 object-factory trait。

VFS make-node operation 应消费 filesystem-neutral 的窄描述，语义上至少包含：

- immutable inode kind；
- permission bits 以及已经由 VFS 决定的 owner/inheritance 结果；
- 仅对 character / block node 有意义的 category-neutral numeric `rdev` identity。

精确 Rust 请求类型和函数签名留给实现阶段；语义形状是接收目录 inode、leaf name 与窄 node
description，成功时返回新 `InodeRef`。filesystem 回调不应接收 task、fd table、设备 registry、FIFO
endpoint 或 Linux syscall 私有参数，也不应重新解释 capability、umask、parent permission、mount
writable 或 `dirfd` 规则。

VFS 负责 pathname/parent resolution、目录权限、mount writable、umask 与 owner/inheritance、node-kind
admission，以及创建 publication 的共同协议。filesystem 负责原子 namespace commit 和本领域的
resident/persistent metadata。创建成功后，immutable `Inode::ty()` 继续是 file kind 的唯一真相源；
device node 的 numeric `rdev` 也必须有唯一、可 reload 的 metadata owner；category 不能在 `rdev` 中再存一份。

这条虚表不是新的通用 object factory。它不创建 opened description，不选择 device provider，不建立
FIFO reader/writer membership，也不接管 Unix socket bind。

### 用户 ABI 边界

首版按 Linux make-node 语义审查下列输入；Draft 已固定 32-bit encoded device number、完整 12/20 范围、
credential gate 与 visible errno：

| `mode` node kind | make-node 创建结果 | 首版默认的后续用户能力 |
| --- | --- | --- |
| type bits 为 0 或 `S_IFREG` | ordinary regular inode | 直接复用现有 regular open/read/write/truncate 能力 |
| `S_IFIFO` | 可持久化/可驻留的 FIFO-kind namespace node | lookup/stat/getdents/unlink；named FIFO open/I/O 不由本 RFC 默认承诺 |
| `S_IFCHR` | 带 numeric `rdev` 的 character-kind namespace node | metadata observation；可自然进入现有 mount source-kind admission |
| `S_IFBLK` | 带 numeric `rdev` 的 block-kind namespace node | metadata observation；不因此承诺按设备号 open provider |
| `S_IFSOCK` | socket-kind namespace node | 只表示 namespace metadata，不等价于已 bind 的 Unix socket endpoint |
| `S_IFDIR`、`S_IFLNK` 或非法 type bits | 不创建 | 由 ABI matrix 给出明确拒绝；目录和 symlink 继续由各自 API 创建 |

character / block node 的创建不以 provider 已注册为前提。设备号 namespace 与 provider lifecycle 仍由设备
领域拥有；make-node 只保存 numeric `rdev` identity。非 root / capability、parent permission、read-only mount、
duplicate name、bad `dirfd`、symlink loop 与 bad user pointer 等错误必须沿现有 owner 边界返回，不能绕过
共同 VFS admission。

最小 errno matrix 是：non-directory parent `ENOTDIR`；read-only mount `EROFS`；writable backend 缺少
make-node operation `EPERM`；named-FIFO data plane 缺失 `EOPNOTSUPP`；ordinary-filesystem device resolver
缺失与未绑定 socket-kind endpoint 均为 `ENXIO`；non-block mount source `ENOTBLK`；合法 block number 的
provider miss 保持当前 `ENOENT`。

## 用户态伴随能力的选择原则

用户态验证是核心交付的一部分，但测试用例需要的所有后续 operation 不是自动进入 target 的理由。首版只
顺带接入满足以下条件的现有能力：

1. consumer 已经存在并拥有自己的语义；
2. make-node 只需交付 kind、`rdev` 或 ordinary inode capability，不需要修改 consumer 的 owner/protocol；
3. 不引入新的共享状态、wait/lifecycle、registry/resolver 或 opened-description handoff；
4. 能直接提高用户态对核心创建能力的证明强度，而不是用另一个大功能替代 make-node 本身的证明。

### 首版应包含的用户态闭环

- 通过真实 `mknod` / `mknodat` syscall 在 in-target filesystem 创建各类节点；
- 使用 `stat` / `statx` 验证 kind、permission、owner 与 `rdev`，使用 `getdents64` 验证 directory type
  projection，并验证 duplicate/error/unlink 路径；
- 对持久 filesystem 验证 inode cache eviction 或 remount/reload 后 metadata 不变；
- 对 `S_IFREG` 节点直接复用 ordinary file data path，证明 make-node 没有制造第二套 regular inode；
- 让 `mount02` 的 character-node source 进入现有 mount admission 并得到 `ENOTBLK`。这只消费 immutable
  kind/`rdev`，不需要 character-device open route，属于自然对接；
- 运行与上述边界对应的 focused `mknod*` / `mknodat*` 用例，但逐项记录测试设施、filesystem 与
  credential 前置条件，不能把未运行或被设施阻塞的项写成 PASS。

首版强制覆盖 ext4 与 ramfs：ext4 证明 on-disk encoding/reload，ramfs 证明同一 VFS handoff 不依赖 ext4
representation。devfs、procfs 等 pseudo filesystem 不转为用户可写 namespace，writable directory backend
缺少 make-node capability 时以 `EPERM` 拒绝。

### 用户态验证载体

本 RFC 不新增长期 `mknod-test` app、独立测试 target 或只为本 RFC 存在的 validation facade。现有
`user-test` 与 LTP runner 已经是用户态系统调用验证入口，新增 app 不会提高 make-node target 的证明强度，
反而会增加一个没有 production consumer 的长期 artifact。

用户态验证默认按以下顺序取证：

1. 首先通过现有 `user-test` runner 运行 focused `mknod*`、`mknodat*` 与 `mount02` LTP；
2. LTP 没有覆盖的精确 proof obligation，例如 character/block `st_rdev`、`statx` projection 或 ext4
   remount/reload 一致性，可以临时修改现有 `user-test` 加入最小 probe；
3. 临时 probe 只属于 validation input，不建立 production dependency，不反向塑造内核 API，并在
   `VFS-MAKE-NODE-CUTOVER` 关闭前删除；执行逻辑、观察结果与删除事实写入 transaction evidence；
4. focused KUnit 与 source audit 证明 ABI decode、`dev_t` codec、backend rollback 等内核边界，但不替代
   至少一条真实用户态 syscall 路径。

如果后续发现某项回归确实需要长期、可重复的 owner-local coverage，应优先放入真实语义 owner 的 inline
KUnit 或现有 LTP 路径；只有出现独立长期 consumer 和编译边界时，才重新审查是否需要新的 validation
artifact，不能让临时 `user-test` probe 自然沉淀成永久接口。

### 不默认顺带实现的能力

#### Named FIFO data plane

`read03` / `write04` 不只是验证 `mknod`。它们要求 FIFO open 根据 access mode 与 `O_NONBLOCK` 建立
reader/writer membership，多次 open 共享同一 buffer truth，并正确处理 rendezvous、empty/full、EOF、
`EPIPE`、poll/readiness、signal interruption、unlink-while-open 和 final cleanup。

live source 中现有 pipe factory 在创建时直接返回一对固定 rx/tx endpoint；filesystem inode `open` 当前也
不接收 open access/status context。因此 named FIFO 不是把 `InodeType::Fifo` 接到现有 `FileOps` 的局部
胶水，而会新增 open handoff 与 lifecycle protocol。它可以成为后续独立阶段或 follow-up RFC，但不作为
make-node R0 的默认 closure，也不能为通过两个用例把匿名 pipe 的对象形状硬塞进 persistent inode。

#### Device node open route

普通 filesystem 中的 `kind + rdev` 若要 open 成 char、block、TTY 或 console provider，需要中立 resolver、
registry consumer boundary、provider lifetime 与错误语义。现有 devfs publication 能力不能仅因设备号相同
就被解释成普通 filesystem 的通用 resolver。

因此本 RFC 默认只保证 device node 的创建、持久 identity、metadata observation，以及已有 consumer 能在
不改变 owner 的前提下读取 kind/`rdev`。按设备号成功 open 真实 provider 另行解析；未知设备号或当前不支持
的 open 必须返回明确错误，绝不能 panic、退化成 regular file 或绑定错误 provider。

#### Pathname socket endpoint

`mknod(S_IFSOCK)` 只建立 socket-kind namespace node，不建立 socket endpoint、bind relation 或消息数据面。
Unix pathname socket 的 identity、bind/connect、unlink 与 lifecycle 应由 socket owner 的独立设计负责。

## Owner 与 handoff 方向

| 责任 / 状态 | 唯一 Owner 方向 | make-node 交付或消费什么 |
| --- | --- | --- |
| Linux `mode`、`dev_t`、`dirfd` 与 syscall variant 解码 | syscall ABI boundary | 规范化输入，不向 filesystem 泄露 syscall 私有形状 |
| 12/20 numeric device-number domain | device owner | 为 ABI/persistence codec 与 typed registry key 提供同一 numeric pair |
| parent lookup、权限、mount writable、umask/inheritance 与 publication 协议 | VFS make-node owner | 向 filesystem 交付已 admission 的窄 node description |
| inode kind | immutable VFS inode identity | stat/getdents/open 等 consumer 只读取 |
| filesystem-backed device node numeric `rdev` | filesystem-neutral inode metadata；persistent backend 负责落盘/reload | consumer 用 inode kind 选择 Char/Block namespace |
| typed char/block registry key | 各自 registry domain | 从已验证 kind + numeric pair 临时构造，不成为 inode truth |
| namespace entry 与 metadata 的 resident/persistent commit | 对应 filesystem | 成功时返回完整 inode，失败时不发布半成品 |
| regular file data plane | 现有 regular inode/file owner | `S_IFREG` 节点直接复用 |
| mount source-kind admission | 现有 mount owner | 读取 node kind/`rdev`，不取得 make-node ownership |
| FIFO data plane、device provider、pathname socket endpoint | 各自领域 owner | 首版 make-node 不创建、不复制其协议状态 |

## 失败与 cleanup 边界

- 创建失败不得留下可 lookup 的半初始化 dentry、只在 inode cache 中存在的成功假象，或已经分配但无法
  回收的 backend inode；
- dentry 可见时，kind、permission、owner 与适用的 `rdev` 已经达到对应 filesystem 的 commit 边界；
- backend 不支持 make-node 或某类 node 时必须在 publication 前返回 `EPERM`；non-directory parent 在进入
  backend 前返回 `ENOTDIR`，read-only mount 返回 `EROFS`；
- 对已经成功创建、但不在首版 data-plane target 内的 node，后续 open 必须显式失败而不是 panic；
- named FIFO open 返回 `EOPNOTSUPP`，ordinary-filesystem char/block 与 unbound socket-kind open 返回 `ENXIO`；
- unlink 只处理 namespace/link lifetime，不因此发明 FIFO、device provider 或 socket endpoint cleanup；
- 用户态 focused test 的 cleanup 必须删除其创建节点，但测试顺序和路径不得进入 production dispatch。

## RFC 的复杂度预算

这份 RFC 可以包含先行的 device-number baseline extraction / 12/20 normalization、syscall ABI、VFS
pathname/create admission、`InodeOps::make_node`、ext4 `rdev` persistence、ramfs resident metadata，以及通过
现有 runner/LTP 或临时 `user-test` probe 取得的用户态 metadata/integration proof。device-number prerequisite
必须先独立关闭，不能与 make-node completion 合并声称；两者都不包含新增长期 test app。

下列信号表示工作已经越过该预算，必须在对应 gate 停止并上报，而不是继续扩张首版：

- 为 FIFO 修改 inode/open 接口，使其传播 access/status context 或建立 reader/writer wait protocol；
- 新增按 device number open provider 的通用 resolver，或修改 char/block/TTY/console registry ownership；
- 为 socket node 引入 bind/connect endpoint state；
- 为某个 LTP case 增加 pathname、filesystem name、provider 或执行顺序特判；
- 新增只服务 make-node 的长期 test app、独立 test target 或没有真实 consumer 的 validation facade；
- 需要改变现有 file-kind、opened-description、TTY/device publication 或 mount current contract，而不只是消费
  它们已经提供的 capability。

这些证据可以触发后续阶段解析、follow-up RFC 或 target renegotiation，但不得反向把已接受的 make-node
核心缩成较弱能力。

## 本轮已解析的提升前事项

1. `mode == 0`、`S_IFSOCK`、non-device `dev`、`CAP_MKNOD`、whiteout exception 与核心 errno 已写入 Draft ABI/
   error matrix；
2. Linux `dev` 固定为 32-bit encoded input，完整 12/20 numeric domain 由 device owner 承接；generic inode
   number 不再复制 Char/Block category，packed layout 只留在边界；
3. ext4 与 ramfs 均为首版强制 backend；filesystem-backed `rdev` persistence 使用窄
   `VFS-SPECIAL-NODE-RDEV-001`，不重新拥有 devfs；
4. live common create path 在 callback 后设置 owner、ext4 uid/gid persistence 尚未完整接线，已作为 make-node
   stage Ready resolution 的实现义务：callback 前决定 final owner/inheritance，由 backend transaction 一次提交；
5. 先提取 live 16/16 `DEVICE-NUMBER-001` current baseline，再独立完成 12/20 cutover；TTY endpoint 号码和
   publication contract 保持 Preserve；
6. device-number 的 KUnit、双架构 build 与 RV64 boot floor 已在 implementation plan 的首个 Ready stage 冻结；
   make-node focused LTP、临时 `user-test` probe 与双架构 runtime matrix 留待 Stage 1 关闭后的独立 resolution；
   两者均未获执行授权，未运行项必须保持 Not Run。

## 公开提升结果

上述 target/owner/ABI/error 边界已折回公开 Draft。2026-07-31 的 promotion gate 已完成 device-number live
baseline 的最小 current-contract 提取，并按 public template 建立 RFC 与 navigation；它没有创建 transaction、
激活已经解析的 Stage 1，也没有把 Stage 2 Outline 自动提升为 Ready。

本次只抽取当前修订需要的最小内容并重新核验 live source。本文不自动成为 RFC 正文、current contract 或
并列历史权威；后续讨论必须继续区分 make-node 核心、自然伴随能力与独立 follow-up，不能用单个 LTP 通过
结果扩大或缩小 target。
