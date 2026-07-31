# VFS Make Node 实施计划

**状态：** R0 Accepted / Stage 1 Active；Stage 2 Outline
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260731-vfs-make-node](./index.md)
**目标与不变量：** [VFS Make Node 目标与不变量](./invariants.md)
**当前契约：** [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md#device-number-001--1616-typed-device-number-namespace)
为 live 16/16 baseline；其余受影响 ID 见 [Contract Impact](./invariants.md#contract-impact)
**事务日志：** [2026-07-31-vfs-make-node](../../devlog/transactions/2026-07-31-vfs-make-node.md)
**Contract Cutover：** `DEVICE-NUMBER-CUTOVER`、`VFS-MAKE-NODE-CUTOVER` 均 Not Cut Over

本文只把 R0 accepted target 转换为可执行顺序、首阶段 write set、验证与停止条件，不重新定义
[`index.md`](./index.md) 和 [`invariants.md`](./invariants.md) 已经拥有的 target、owner、ABI 或 proof
obligations。它是一份窄 RFC 的实施计划：除一个必须先独立关闭的 device-number prerequisite 外，make-node
实现保持一个完整 stage，不再按 syscall、VFS、filesystem backend 或验证拆成多个 contract-bearing stage；
同一 stage 内仍允许为结构拆分、feature wiring、review 与验证建立可独立记录的内部 checkpoint。

## 1. Authority 与窄计划形状

发生冲突时按以下顺序判断：

1. `docs/src/contracts/` 描述已经生效的 shared rules；
2. `index.md` 与 `invariants.md` 描述本 RFC 的 accepted-but-not-effective target；
3. 本文拥有实施顺序、stage maturity、首个 Ready stage、resolved manifest、验证与停止条件；
4. transaction 只记录 preflight、授权、执行事实、review、验证和 cutover，不复制第二份计划；
5. register / current limitations 只记录实际开放缺陷与已接受限制。

本计划只有两个 stage：

- Stage 1 原子完成 16/16 到 12/20 的 device-number owner cutover；
- Stage 2 原子完成 `mknodat`、VFS `make_node`、ext4/ramfs、metadata/open error、mount companion 与用户态证明。

`DEVICE-NUMBER-BASELINE-EXTRACTION` 是已经关闭的 docs-only public-promotion entry gate，不算实现 stage。Stage 2 也不再建立
各自拥有 partial contract cutover 的 syscall/backend/validation 子阶段；这些工作只有合在一起才能交付用户可见能力和
最终 contract cutover。但这不禁止内部 implementation checkpoint：Stage 2 必须先执行一次 behavior-preserving 的
`S2-CKPT-VFS-MODULE-SPLIT`，之后由 `1 -> 2` resolution 冻结 feature wiring、review 与 validation checkpoint 顺序。
内部 checkpoint 可以独立 commit、review 和记录证据，但不能声称部分 `mknodat` capability 或提前 cut over contract；
前一 checkpoint 关闭也不自动授权下一 checkpoint。

成熟度含义：

- `Outline` 只固定目的、依赖、受保护边界与解析触发点；预计文件不是写入授权；
- `Ready` 表示交付、路线、审计、验证、退出条件、cutover 与 manifest 已解析，但仍未获得执行授权；
- `Active` 需要 accepted RFC、transaction preflight 与用户明确授权；
- `Closed` 需要 stage 自己的 review、验证、write-back 与 cutover 全部闭合。

Stage 1 必须先独立 Closed。之后另行运行只读的 `1 -> 2 Implementation Resolution Gate`，只把 Stage 2 从
Outline 解析为 Ready；该 gate 不属于 Stage 1 closure，也不自动启动 Stage 2。

## 2. Public promotion 与 baseline entry gate（Closed / 不计入 Stage）

2026-07-31 已执行 `DEVICE-NUMBER-BASELINE-EXTRACTION`：

1. 重新读取 live `device::devnum`、`DeviceId`、devfs/TTY/char/block/loop/stat/mount consumer，以及
   `ANE-CHG-20260722-device-devnum-ownership`；
2. 在 device-owned current contract 中只提取已经生效的 16-bit major + 16-bit minor、char/block namespace
   分离、producer-owned static number 与 registry ownership；不得把本 RFC 的 12/20 target 提前写成 current fact；
3. 按 public RFC template 提升 `index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md` 与必要
   background，补齐 contract/RFC/register/navigation；
4. 独立 review 接受 R0 后才建立 `docs/src/devlog/transactions/2026-07-31-vfs-make-node.md`；transaction 记录
   Stage 1 Ready 链接和 activation preflight，不复制本节或第 5 节；
5. 再次核对 branch、HEAD、dirty state、register、current contract 与第 5.5 节 manifest，由用户单独授权
   Stage 1。

entry gate 只发布 current baseline 与公开 Draft target，不修改代码，不执行任何 target cutover。它没有接受 R0、
建立 transaction 或激活 Stage 1。若后续 transaction 文件名或 contract 落点与本文预期不同，必须在 Stage 1
激活前先更新本文 manifest；不能把 public promotion 当成隐式 write-set 扩展或执行授权。

本 gate 的文档验证为 `git diff --check`、独立新文件 whitespace check 与 `mdbook build docs`；均通过。
kernel build、QEMU、KUnit 与 LTP 均 Not Run，也不属于本 docs-only gate 的证明范围。

## 3. 全局实施边界

- `Inode::ty()` 始终是 file kind 与 Char/Block category 的唯一 truth；numeric `rdev` 不复制 category。
- `device::devnum` 拥有共同 12/20 numeric domain；typed char/block key 只在 registry boundary 存活。
- syscall adapter 只拥有 Linux raw `mode` / `dev_t` / `dirfd` admission；filesystem 只接收 normalized node
  description。
- `anemone_abi::fs::linux::dev_t` 是 Linux 12/20 packed layout 的唯一 codec owner，只接收/返回 primitive
  major/minor 与 encoded integer；`device::devnum` 继续唯一拥有结构化 numeric domain。stat、loop 与后续
  `mknodat` 只能调用该窄 codec，不复制位运算，也不把 packed value 存入 generic metadata。
- VFS 在 backend callback 前决定 parent、mount/DAC/common-create admission 与 final permission/uid/gid；backend
  原子提交完整 metadata 与 dirent，VFS 只 materialize callback 返回的同一 inode。
- ext4 与 ramfs 都是首版必要 backend。`InodeOps` initializer sweep 只是新增 function pointer 的机械伴随修改，
  不形成独立 owner 或 stage；完整 Rust struct initialization、冻结后的精确 `rg` sweep、双架构 build 与真实
  backend/open 路径共同证明 wiring，不为每个 initializer 建立 KUnit。
- Stage 2 feature code 前先在同一 VFS owner 内目录化拆分 `fs/mod.rs` 与 `fs/inode.rs`；该 checkpoint 只移动现有
  singleton/operation/metadata/object/ops 职责并保持 re-export、可见性和行为，不扩大 public API 或 shared contract。
- regular node 复用 ordinary file behavior；FIFO、device 与 socket node 首版只承诺 namespace/metadata，unsupported
  open 返回 RFC 固定的 errno，不建立数据面。
- 不新增 device-number resolver、named-FIFO protocol、pathname socket endpoint、长期 test app、通用 factory、
  validation facade 或 future filesystem framework。
- build、rootfs 与 QEMU 只走 `just`、`scripts/xtask` 和现有 end-to-end wrapper；validation-only 临时 probe 在
  Stage 2 closure 前删除。
- 实现代码预计保持在两千行以内；行数不是 correctness gate，但明显越过该量级通常表示 owner、rollback、
  data-plane 或测试设施已经扩张，必须停下来复核而不是增加更多形式化 stage 掩盖变化。
  `S2-CKPT-VFS-MODULE-SPLIT` 的一一移动行不计作新增行为代码，但 transaction 必须单独记录其 diff 规模。

## 4. 阶段路线图

| Stage                                | 成熟度             | 单一交付                                                                                     | Contract Cutover        | 解析触发点                                        |
| ------------------------------------ | ------------------ | -------------------------------------------------------------------------------------------- | ----------------------- | ------------------------------------------------- |
| Stage 1 — Device-number prerequisite | Active             | 12/20 category-neutral numeric domain 与全部既有 consumer 迁移                               | `DEVICE-NUMBER-CUTOVER` | entry gate、R0 acceptance、transaction 与独立授权 |
| Stage 2 — Make-node vertical slice   | Outline            | RV64/LA64 `mknodat` 到 ext4/ramfs persistence、metadata/open/mount 与用户态 proof 的完整闭环 | `VFS-MAKE-NODE-CUTOVER` | Stage 1 Closed 后的独立 `1 -> 2` resolution       |

Stage 1 不交付 make-node syscall；Stage 2 不重新打开 device-number namespace。两次 cutover 各自保持旧 contract
直到相应 stage 完整通过，不能把 Stage 1 的 build/KUnit 证据写成 make-node implementation proof。

## 5. Stage 1 Ready — Device-number prerequisite

### 5.1 成熟度、前置条件与受保护边界

本阶段于 2026-08-01 经独立 R0 复审、transaction preflight 与用户的 Stage 1 唯一 GOAL 激活为 `Active`。
本次 activation 只授权第 5.5 节 frozen manifest 和 `DEVICE-NUMBER-CUTOVER`；不授权 Stage 2 或
`1 -> 2 Implementation Resolution Gate`。

激活前必须满足：

- 第 2 节 entry gate 已完成，R0 已由独立 review 接受，16/16 `DEVICE-NUMBER-001` 是可链接的 current baseline；
- transaction 已建立并记录 branch、HEAD、dirty state、current `kconfig`、register 与本节链接；
- live source 仍保持当前 `MAJOR_BITS = 16`、`MINOR_BITS = 16`、`DeviceId::{None, Char, Block, Raw}` baseline；
- 第 5.5 节路径无漂移；若漂移，先重新解析并 review，不边做边扩大 manifest；
- 用户对 Stage 1 给出独立实现授权。

本阶段保护：既有 char/block namespace 分离、static major/minor、endpoint name、devfs/TTY publication、provider
lookup/lifecycle、block I/O 与 mount provider-miss semantics。它不得增加 `mknodat`、filesystem special node、
`ENOTBLK` visible refine 或普通 filesystem device-open route。

### 5.2 冻结交付与内部形状

1. `device::devnum` 定义 category-neutral `DeviceNumber`，由 `MajorNum` 与 `MinorNum` 组成；公共数值域固定为
   12-bit major + 20-bit minor。constructor 使用 correctness `assert!`，不以 `debug_assert!` 接受 release 截断。
2. `CharDevNum` 与 `BlockDevNum` 保持不同类型与不同 registry key。它们只包装/投影同一个
   `DeviceNumber`，提供显式 `number()` / conversion boundary；二者不能进入 generic inode 作为 category truth。
3. `DeviceId` 收窄为 `None` 与 category-neutral number，删除 `Char`、`Block` 和 `Raw` escape。若实现选择直接用
   `Option<DeviceNumber>`，必须证明没有扩大调用面；默认路线保留窄 enum 以减少机械 churn。
4. Linux device-number codec 的唯一 owner 固定为现有 `anemone-abi/src/fs.rs` 中新增的
   `anemone_abi::fs::linux::dev_t` 模块；Stage 1 不新建其它 ABI-owner 文件。其窄 API 固定为
   `encode(major: u32, minor: u32) -> u32` 与 `decode(dev: u32) -> (u32, u32)`：encode 对超出 12-bit
   major / 20-bit minor 的输入使用 correctness assertion，decode 对全部 32-bit pattern 可 round-trip。
   codec 不依赖 kernel `DeviceNumber`、inode 或 registry 类型。
5. Linux device-number projection 只保留在 stat/statx 和 loop ABI 等边界：stat/loop 从结构化 numeric pair 调用
   上述 codec，statx 直接投影 major/minor；后续 Stage 2 `mknodat` 也只能调用同一 decode API。generic metadata
   与 registry 不保存 packed integer，不复制 codec 位运算，也不提供行为性 `raw()` escape。
6. devfs char/block、console 与 TTY publication 从 typed endpoint key 投影 numeric pair，并以 inode kind/assertion
   保证 category；block/char FileOps 在构造 typed lookup key 前检查 immutable inode kind。
7. mount consumer 先以 inode kind 选择 Block namespace，再从 numeric `rdev` 构造 `BlockDevNum`。本阶段只迁移
   truth source，保留当前 non-block visible errno；`ENOTBLK` refine 留在 Stage 2 的原子 cutover。
8. ext4 backing-device `fs_dev`、loop ABI 与 stat/statx 改读同一 numeric truth；现有 static producer、allocator、
   registry 与 endpoint number 不改策略。扩大 minor domain 仍使用当前 sparse allocator，不引入百万项 bitmap。
9. 新增 KUnit 集中覆盖 domain 边界、ABI codec 的全域边界/代表性 pattern round-trip、typed-key conversion 与
   stat/statx/loop 对同一 codec 的投影一致性；devfs/TTY/initializer 等机械 consumer 依靠 assertion、source audit、
   build 与真实 runtime 路径证明，不为它们逐项新增 KUnit，也不新建独立 tests/validation module。

### 5.3 审计与验证

实现与 review 必须执行并分别记录：

1. `just fmt kernel --check` 与 `git diff --check`；
2. `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`；
3. `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`；
4. `./scripts/run-user-test-rv64.sh <caller-selected-rv64-sdcard-image> build/vfs-make-node-device-number-rv64.log`；
   transaction preflight 必须绑定明确的只读 master identity，运行结果要求 boot KUnit 完整通过，并确认既有
   console、TTY、char 与 block publication 没有号码/名称回归；
5. source audit：production 中无 `DeviceId::Char`、`DeviceId::Block`、`DeviceId::Raw`、重复 Linux codec 位运算、
   16/16 packing 或未验证的 typed-key construction；stat 与 loop 均调用唯一 ABI codec，existing producer
   number/name 表与 change-record baseline 一致；
6. review 聚焦 category single truth、ABI round-trip、mount admission order、loop codec、allocator bound、devfs/TTY
   publication 与 provider lookup 未漂移。

`just test xtask` 与本阶段 kernel/device-number 改动没有直接 proof relation；两个 preset build 已通过 repository
xtask 入口。只有修改 `scripts/xtask`、Justfile 或 build configuration owner 时才把它升级为 mandatory gate，否则
可作为显式标注的 optional repository-health check，未运行不阻塞本阶段。

第 4 项只证明 RV64 current endpoint/codec integration，不证明 `mknodat` 或 LA64 runtime。LA64 runtime、完整
filesystem node matrix 与真实 syscall proof 留在 Stage 2；未运行项必须写为 Not Run，不能由双架构 build 替代。

### 5.4 Cutover、退出与停止条件

只有以下条件全部满足，`DEVICE-NUMBER-CUTOVER` 才能把 current `DEVICE-NUMBER-001` 从 16/16 原子 Refine 为
12/20：

- 第 5.2 节全部交付和 KUnit/source audit 完成；
- 双架构 build 与 RV64 boot/KUnit floor 通过；
- existing number/name/publication/provider behavior 无回归；
- review 无未关闭 Apollyon/Keter/Euclid；
- current contract、RFC status、transaction 与双周 devlog 在同一 closure write-back 中一致。

任一项失败时保持 16/16 current contract，Stage 1 不关闭，也不进入 `1 -> 2` resolution。出现以下情况必须
立即停止并上报：需要 raw escape 才能维持 consumer、拒绝/截断合法 Linux encoding、修改 endpoint 号码或
publication/provider lifecycle、合并 char/block registry、引入 device-open resolver，或需要移动 existing owner。

### 5.5 Resolved Write Set Manifest

Production / ABI codec / consumer-local KUnit：

- `anemone-abi/src/fs.rs`
- `anemone-kernel/src/device/devnum.rs`
- `anemone-kernel/src/device/mod.rs`
- `anemone-kernel/src/device/char/devfs.rs`
- `anemone-kernel/src/device/block/devfs.rs`
- `anemone-kernel/src/device/block/loop.rs`
- `anemone-kernel/src/device/console.rs`
- `anemone-kernel/src/device/tty/endpoint.rs`
- `anemone-kernel/src/fs/inode.rs`
- `anemone-kernel/src/fs/devfs/mod.rs`
- `anemone-kernel/src/fs/ext4/inode.rs`
- `anemone-kernel/src/fs/api/mount/mount.rs`

Public write-back（公开路径已由第 2 节 entry gate 建立；Stage 1 未激活前不得写入 implementation closure）：

- `docs/src/contracts/device/device-number.md`
- `docs/src/contracts/device/index.md`
- `docs/src/SUMMARY.md`
- `docs/src/rfcs.md`
- `docs/src/rfcs/vfs-make-node/index.md`
- `docs/src/rfcs/vfs-make-node/invariants.md`
- `docs/src/rfcs/vfs-make-node/implementation.md`
- `docs/src/rfcs/vfs-make-node/tracking-issues.md`
- `docs/src/register/current-limitations.md`
- `docs/src/register/open-issues.md`
- `docs/src/devlog/transactions/2026-07-31-vfs-make-node.md`
- `docs/src/devlog/transactions/index.md`
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`

验证只读输入包括 register、current contracts、`ANE-CHG-20260722-device-devnum-ownership`、全部 device-number
consumer、`conf/.defconfig`、两个 QEMU release preset 与 caller-selected read-only master image。wrapper 创建的
rootfs、运行盘、日志和 build artifact 只属于 validation output，不是 production write set。除 formatter 对目标
Rust 文件的正常改写外，任何其它 source/docs 路径都属于 expansion。

## 6. `1 -> 2 Implementation Resolution Gate`

Stage 1 独立 Closed 后，另行只读检查：

- Stage 1 final diff、review、KUnit/build/runtime evidence 与 current `DEVICE-NUMBER-001`；
- live syscall registry、capability set、common VFS create path、全部 `InodeOps` initializer；
- ext4/lwext4 create、owner/rdev persistence、rollback、reload 与 special open；
- ramfs create/resident metadata/open/unlink；
- mount admission、`SysError` mapping、stat/statx/getdents 与 LTP/user-test harness；
- RFC target、tracking issues、register、module pressure、dirty state 与 exact public transaction path。

该 gate 必须把第 7 节整体解析为一个 Ready stage：冻结内部 checkpoint 顺序、
`S2-CKPT-VFS-MODULE-SPLIT` 的 exact old/new path 与 re-export/visibility 保持条件、node description、
common-create handoff、ext4 rollback、exact manifest、validation profile/probe、两架构命令与 final cutover。
不能只解析 syscall 或 ext4 后就开始编码，也不能跳过结构 checkpoint 继续向两个大文件叠加职责；内部 checkpoint
不把同一 vertical slice 变成新的 contract-bearing stage。

## 7. Stage 2 Outline — Make-node vertical slice

### 7.1 单一目的与依赖

本阶段在 Stage 1 关闭后的 12/20 baseline 上，一次交付：RV64/LA64 asm-generic `mknodat(33)`、Linux
mode/capability admission、VFS `InodeOps::make_node` handoff、ext4/ramfs 全 node matrix、kind/owner/permission/
`rdev` persistence、explicit unsupported open、stat/statx/getdents/unlink、regular ordinary I/O 与 mount
`ENOTBLK`。这些不是多个可独立验收的能力，全部共享 `VFS-MAKE-NODE-CUTOVER`。

Stage 2 当前是 `Outline / Not Active`。本节预计路径、类型和命令都不是写入授权；Stage 1 closure 不能顺带修改
这里列出的 production source。

### 7.2 受保护 target 与禁止扩张

- 保持 R0 的完整 node-kind、dirfd、mode、`dev`、`CAP_MKNOD`（含 char 0:0 无 whiteout 例外）与 errno matrix；
- 保持 R0 明确不应用 process umask 的 requested-permission semantics；不得读取 `sys_umask` stub、建立
  mknod-local/task-local mask，或借本阶段扩展全部创建类调用点；
- 保持 ext4 + ramfs acceptance boundary、atomic publication/reload 和 provider-independent creation；
- backend 不接收 task、fd table、raw Linux mode/dev_t、capability 或 provider handle；
- 不实现 named FIFO、device provider open、pathname socket endpoint、legacy `readdir` 或 overlayfs whiteout；
- 不为单个 LTP pathname、filesystem、provider 或执行顺序增加 production 特判；
- 不把 `sys_umask` stub 扩张为独立全系统项目；requested permission bits 直接进入 existing common-create
  handoff，只复用现有 owner/group policy。若不扩大 task/全部 create surface 就无法形成这一 R0 final metadata，
  命中停止条件并上报，而不是顺带实现 umask。

### 7.3 预计 owner 与文件面

resolution gate 预计检查并收窄以下 implementation surface：

- syscall ABI：`anemone-abi/src/syscall/{riscv,loongarch}.rs`、`anemone-kernel/src/fs/api/{mod.rs,mknodat.rs}`、
  `anemone-kernel/src/task/credentials/cap.rs`；Stage 1 已关闭的 `anemone-abi/src/fs.rs` codec 默认只读复用；
- VFS module split：保留 `anemone-kernel/src/fs/mod.rs`，以
  `anemone-kernel/src/fs/vfs/{mod.rs,ops.rs}` 承接 global VFS owner、`PathResolution`、VFS operations 与其 inline
  KUnit；以 `anemone-kernel/src/fs/inode/{mod.rs,ops.rs,metadata.rs,object.rs}` 替换
  `anemone-kernel/src/fs/inode.rs`，分别承接 module/re-export、vtable/open、metadata/Linux projection 与
  inode/ref/lifecycle；
- VFS creation：上述 split 后的 VFS/inode owner 与必要的 common path/namei helper；
- backend：`anemone-kernel/src/fs/{ext4,ramfs}/`、
  `anemone-kernel/crates/anemos/lwext4-rust/src/{fs.rs,inode/attr.rs}`；
- companion/error：`anemone-kernel/src/{syserror.rs,fs/api/mount/mount.rs}`；
- mechanical initializer sweep：由
  `rg -l 'InodeOps = InodeOps \{' anemone-kernel/src | sort` 在 resolution 当日精确冻结；
- validation-only：现有 `anemone-apps/user-test`、LTP profile、pretest rootfs 与 end-to-end wrapper；临时 probe
  必须在 cutover 前删除，不能进入 production dependency。

预计新增 production 文件为 `fs/api/mknodat.rs` 与上述 VFS/inode split 文件，并删除被目录模块替代的
`fs/inode.rs`；其它变化应落在真实 owner 的现有模块或 inline KUnit。resolution 必须重新核对 live source 后冻结
exact manifest，但不得借拆分移动 owner、扩大 public API 或改变 shared contract。

### 7.4 内部 checkpoint：`S2-CKPT-VFS-MODULE-SPLIT`

这是 Stage 2 Ready 后的第一个 implementation checkpoint，先于 `make_node` function pointer、syscall、backend 或
errno feature wiring。它只做以下 behavior-preserving 移动：

1. `fs/mod.rs` 只保留 module declarations、稳定 re-export 与 filesystem-driver initialization；当前 inline global
   VFS singleton/mount/filesystem-registry owner、`PathResolution` 和 VFS operations 移入
   `fs/vfs/{mod.rs,ops.rs}`；现有 VFS KUnit 留在被测 operation 文件末尾，不另建 `tests.rs`。
2. `fs/inode.rs` 转为 `fs/inode/`：`mod.rs` 只组织并重导出现有 surface，`ops.rs` 保存 `InodeOps`、
   `OpenedFile`、`RenameFlags`，`metadata.rs` 保存 inode identity/mode/permission/stat metadata、Linux projection 与
   stat KUnit，`object.rs` 保存 `Inode` / `InodeRef`、reference/lifecycle 与 local metadata mutation。
3. 所有既有 `crate::fs::*` 与 `crate::fs::inode::*` 可见性、函数签名、re-export、KUnit consumer 和调用方向保持
   不变；checkpoint 不新增 `make_node`、request type、helper abstraction 或 production behavior。

checkpoint closure 必须记录：

- 对 old/new symbol 的一一移动审计，以及精确 `rg` 的 module declaration/re-export/visibility sweep；
- `just fmt kernel --check`、`git diff --check` 和 RV64/LA64 两个 release preset build；
- review 确认没有 owner/public API/contract delta、没有趁拆分重构相邻代码、现有 inline KUnit 仍由原 owner 编译；
- transaction 中独立的 activation、commit/diff、review 和 closure；`VFS-MAKE-NODE-CUTOVER` 继续 Not Cut Over。

若拆分需要改变 public surface、跨 owner 移动状态或增加共享抽象，checkpoint 立即停止并走 write-set/设计扩展。
checkpoint 关闭不自动授权 feature wiring；下一 checkpoint 必须按 Stage 2 resolution 中冻结的顺序另行记录 activation。

### 7.5 Ready resolution 必须闭合的实现问题

1. final node description 的 Rust shape，以及 common VFS create path 如何在 callback 前得到 requested permission
   bits（不应用 process umask）、uid、gid；
2. regular node 复用 `touch` 还是统一进入 backend `make_node`，同时确保只有一个 publication path；
3. lwext4 如何在 `add_entry` 前设置 final mode/owner/适用 `rdev`，每个 fallible step 的 rollback 与 free-inode 顺序；
4. ext4 `ext4_inode_get_dev/set_dev` adapter、reload/getattr codec 与 uid/gid 持久化 proof；
5. ramfs special inode private metadata、FileOps selection 与 unsupported open error；
6. pseudo-fs/non-directory `make_node` function pointer、resolution 当日全部 initializer hit 的机械拒绝值和 no-panic
   exhaustive audit；
7. `SysError -> ENOTBLK` 的 system-wide narrow mapping，以及 mount kind check 与 provider-miss `ENOENT` 分离；
8. focused LTP case/profile、temporary `user-test` probe、ext4 reload/ramfs/statx oracle 和双架构 runtime 命令。

### 7.6 预期验证与单一 cutover

Stage 2 Ready definition 至少应覆盖：

- KUnit 只聚焦 Stage 1 Linux `dev_t` codec 的复用、mode/dev/capability request normalization、backend
  rollback/rdev reload，以及 unsupported-open/errno 错误边界；不为全部 `InodeOps` initializer 或纯字段 wiring
  建立逐项 KUnit；
- `just fmt kernel --check`、`git diff --check` 与 RV64/LA64 release build；完整 Rust struct initializer 会强制每个
  owner 显式填写新字段，resolution 当日冻结的
  `rg -l 'InodeOps = InodeOps \{' anemone-kernel/src | sort` 必须在实现后逐项复扫并分类；
- 两个现有 end-to-end wrapper 的 focused `mknod*`、`mknodat*`、`mount02`，逐架构记录 PASS/FAIL/TCONF/BROK；
- 临时 probe 补齐 ext4/ramfs kind、permission、owner、`st_rdev`/statx、getdents、unlink、duplicate、bad path/dirfd、
  read-only mount、CAP_MKNOD、regular I/O、special open 与 ext4 reload；这些真实 backend/open 路径与双架构 build
  一起构成 mechanical initializer wiring 的行为证明；
- source audit 证明无 provider lookup、raw packed truth、panic、success stub、test-path dispatch 或遗留 probe。

`just test xtask` 默认只是 optional repository-health check：两个 preset build 已经通过 xtask owner。只有本阶段实际
修改 Justfile、`scripts/xtask` 或 build configuration 时才把 focused xtask tests 提升为 mandatory closure evidence；
否则未运行必须如实记录，但不阻塞 Stage 2。

只有全部 target、两 backend、错误矩阵、用户态 proof、review、register disposition 与 docs write-back同时闭合，
`VFS-MAKE-NODE-CUTOVER` 才原子 Introduce `VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001` 并 Refine
`VFS-MOUNT-ADMISSION-002`。任一必要项失败时三项全部 Not Cut Over，不把“syscall 已注册”或单架构 LTP
写成 partial contract。

### 7.7 停止条件

出现以下证据时 Stage 2 必须在 cutover 前停止：

- callback 前 final owner/permission 无法由 existing VFS owner 形成，需扩大 shared create contract；
- 需要读取 `sys_umask` stub、建立 task/fs-state mask owner或修改其它创建类调用点才能继续；
- ext4 无法在失败后避免可 lookup 的半初始化 node，或需要接受 cache-only `rdev`；
- 需要改变 `InodeOps::make_node` owner、node-kind/ABI/errno matrix、ext4+ramfs acceptance boundary；
- 需要 FIFO/device/socket 数据面、通用 provider resolver、opened-description handoff 或新生命周期协议；
- 只能通过缩减合法 Linux encoding、静默创建错误 kind、保留 panic 或长期 validation facade 才能完成。

保持 target 的内部 route、预计文件、验证安排或行数变化可以回写本文后重新 review；target/owner/ABI/contract/
visible semantics/acceptance boundary 变化必须进入 RFC review / `Target Renegotiation Gate`，agent 无权自行批准。

## 8. Write-set expansion 与执行反馈

- Stage 1 Ready/Active 越过第 5.5 节 manifest 时，先停止并报告原因、候选路径、owner/contract 影响和验证计划；
  批准后先更新本文，再由 transaction 记录批准事实与链接。
- Stage 2 仍为 Outline 时，resolution 对预计路径的收窄、扩大、合并或拆分不是 expansion；Stage 2 进入 Ready 后
  才冻结 exact manifest。
- implementation feedback 若只改变保持 target 的路线、manifest、验证、停止条件或顺序，写回本文和 transaction；
  执行事实只写 transaction；accepted target 变化先回 RFC review；effective shared rule 只在获准 cutover 更新。
- probe 计划留在本文；执行结果留在 transaction。除证据包过长且有具体主题外，不新建通用 `probe.md`、
  `feedback.md` 或 `experiments.md`。
- 任一 stage 关闭后都要扫描 RFC header、implementation maturity、transaction、current contract、devlog、register
  和 navigation，避免只更新一处状态。未运行的 architecture/runtime/filesystem proof 始终保留 Not Run。
