# VFS Make Node 实施计划

**状态：** R1 Accepted / Stage 1 Closed；Stage 2 Ready / Not Active
**最后更新：** 2026-08-01
**父 RFC：** [RFC-20260731-vfs-make-node](./index.md)
**目标与不变量：** [VFS Make Node 目标与不变量](./invariants.md)
**当前契约：** [`DEVICE-NUMBER-001`](../../contracts/device/device-number.md#device-number-001--1220-category-neutral-device-number-domain)
已由 Stage 1 cut over为live 12/20 baseline；其余受影响ID见[Contract Impact](./invariants.md#contract-impact)
**事务日志：** [2026-07-31-vfs-make-node](../../devlog/transactions/2026-07-31-vfs-make-node.md)
**Contract Cutover：** `DEVICE-NUMBER-CUTOVER` Effective；`VFS-MAKE-NODE-CUTOVER` Not Cut Over

本文只把 R1 accepted target 转换为可执行顺序、stage write set、验证与停止条件，不重新定义
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
Checkpoint C1，之后按本次`1 -> 2` resolution冻结的C2 merged feature与C3 validation/cutover顺序执行。
内部 checkpoint 可以独立 commit、review 和记录证据，但不能声称部分 `mknodat` capability 或提前 cut over contract；
前一 checkpoint 关闭也不自动授权下一 checkpoint。

成熟度含义：

- `Outline` 只固定目的、依赖、受保护边界与解析触发点；预计文件不是写入授权；
- `Ready` 表示交付、路线、审计、验证、退出条件、cutover 与 manifest 已解析，但仍未获得执行授权；
- `Active` 需要 accepted RFC、transaction preflight 与用户明确授权；
- `Closed` 需要 stage 自己的 review、验证、write-back 与 cutover 全部闭合。

Stage 1 已先独立 Closed。2026-08-01 另行运行的只读`1 -> 2 Implementation Resolution Gate`只把Stage 2从
Outline解析为Ready；该gate不属于Stage 1 closure，也没有启动Stage 2。

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
  在本地先完成最终metadata，再提交dirent并回滚提交前失败；VFS按既有协议materialize callback返回的同一inode。
  既有backend commit到inode-cache/dentry materialization窗口若需新的跨owner transaction protocol，只登记为
  开放问题；Stage 2不得比touch/mkdir引入更弱失败路径，但不负责该架构改造。
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
  C1 module split的一一移动行不计作新增行为代码，但transaction必须单独记录其diff规模。

## 4. 阶段路线图

| Stage                                | 成熟度             | 单一交付                                                                                     | Contract Cutover        | 解析触发点                                        |
| ------------------------------------ | ------------------ | -------------------------------------------------------------------------------------------- | ----------------------- | ------------------------------------------------- |
| Stage 1 — Device-number prerequisite | Closed             | 12/20 category-neutral numeric domain 与全部既有 consumer 迁移                               | `DEVICE-NUMBER-CUTOVER` Effective | entry gate、R0 acceptance、transaction 与独立授权 |
| Stage 2 — Make-node vertical slice   | Ready / Not Active | RV64/LA64 `mknodat` 到 ext4/ramfs persistence、metadata/open/mount 与用户态 proof 的完整闭环 | `VFS-MAKE-NODE-CUTOVER` Not Cut Over | 2026-08-01 独立 `1 -> 2` resolution已完成；仍需单独activation |

Stage 1 不交付 make-node syscall；Stage 2 不重新打开 device-number namespace。两次 cutover 各自保持旧 contract
直到相应 stage 完整通过，不能把 Stage 1 的 build/KUnit 证据写成 make-node implementation proof。

## 5. Stage 1 Closed — Device-number prerequisite

### 5.1 成熟度、前置条件与受保护边界

本阶段于 2026-08-01 经独立 R0 复审、transaction preflight 与用户的 Stage 1 唯一 GOAL 激活，并在同日完成
实现、验证、独立终审与`DEVICE-NUMBER-CUTOVER`后关闭。该closure只覆盖第5.5节frozen manifest；不授权
Stage 2或`1 -> 2 Implementation Resolution Gate`。

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

2026-08-01 closure满足全部条件：dirty source set恰好为第5.5节十二个路径；12/20 domain、category-neutral
`DeviceNumber`、typed registry boundary、canonical u32 Linux codec、stat/statx/loop与mount consumer迁移完成；
source audit没有旧`DeviceId::{Char, Block, Raw}`、device-number `raw()`、16/16 packing或重复codec。最终RV64/
LA64 release build、format与whitespace通过；RV64 wrapper在near-final tree完成282/282 KUnit、focused LTP 4/4与
正常关机，随后只有u32 codec签名收窄、mount kind检查前移和多余console单值KUnit删除，开发者明确接受无需重跑
QEMU；最终owner audit保持codec位宽常量private，由`device::devnum`拥有public domain bounds。独立终审为
Apollyon/Keter/Euclid/Safe全0。因此`DEVICE-NUMBER-001`原子Refine为
effective 12/20 current contract，Stage 1 Closed。LA64 runtime、`mknodat`与filesystem node matrix保持Not Run；
Stage 2后续已由独立docs-only gate解析为Ready / Not Active；该事实不属于Stage 1 closure，也不反推make-node proof。

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

## 6. `1 -> 2 Implementation Resolution Gate`（Completed / 2026-08-01）

该独立docs-only gate从clean `dev/drc/alpha@c72721dd`开始，读取Stage 1 final diff/review/validation与effective
`DEVICE-NUMBER-001`，并重新审计live syscall registry、capability、common-create、32个`InodeOps` static、
ext4/lwext4 create与reload、ramfs transaction、mount/error mapping、stat/statx/getdents以及LTP/user-test入口。
`fs/mod.rs`为1271行、`fs/inode.rs`为1061行，确认feature前应先做同owner行为保持拆分。

resolution固定以下结论：

- Stage 2只有C1结构拆分、C2合并feature implementation、C3 validation/probe removal/review/cutover三个checkpoint；
  原先可分开的syscall/VFS与ext4/ramfs/mount工作合并为一个C2，不能形成partial capability或partial contract；
- VFS在callback前形成`MakeNodeDescription`，backend-local先写final mode/uid/gid/适用`rdev`再提交dirent；
  ext4/ramfs提交前失败必须回滚本次allocation/resource；
- live common-create已有backend commit到inode-cache/dentry materialization窗口。R1接受：若关闭该窗口需要新跨owner
  transaction/rollback protocol，则登记`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`，不在本RFC解决或阻塞；
  Stage 2仍须复用既有handoff、不得引入比touch/mkdir更弱的failure path；
- syscall、两backend、mount errno、durable userspace wrapper、focused LTP和临时user-test probe的exact route、
  manifest、验证与停止条件均由第7节冻结。

本gate只修改RFC/transaction/register/navigation/devlog文档并运行docs验证；kernel build、KUnit、QEMU、LTP、
RV64/LA64 runtime与Stage 2行为全部Not Run。结果是`Stage 2 Ready / Not Active`；C1仍需用户独立授权。

## 7. Stage 2 Ready — Make-node vertical slice

### 7.1 成熟度、单一交付与受保护边界

本阶段在 Stage 1 关闭后的 12/20 baseline 上，一次交付：RV64/LA64 asm-generic `mknodat(33)`、Linux
mode/capability admission、VFS `InodeOps::make_node` handoff、ext4/ramfs 全 node matrix、kind/owner/permission/
`rdev` persistence、explicit unsupported open、stat/statx/getdents/unlink、regular ordinary I/O 与 mount
`ENOTBLK`。这些不是多个可独立验收的能力，全部共享 `VFS-MAKE-NODE-CUTOVER`。

Stage 2当前是`Ready / Not Active`。Ready不等于授权：C1、C2、C3都未激活，且前一checkpoint关闭不会自动
激活下一checkpoint。只有C3满足全部closure条件后才能执行一次`VFS-MAKE-NODE-CUTOVER`；C1/C2均不得写current
contract或宣称partial make-node capability。

- 保持 R1 的完整 node-kind、dirfd、mode、`dev`、`CAP_MKNOD`（含 char 0:0 无 whiteout 例外）与 errno matrix；
- 保持 R1 继承的 no-umask requested-permission semantics；不得读取 `sys_umask` stub、建立
  mknod-local/task-local mask，或借本阶段扩展全部创建类调用点；
- 保持 ext4 + ramfs acceptance boundary、backend-local final-metadata/dirent commit、提交前rollback、reload与
  provider-independent creation；既有common-create跨cache/dentry窗口只要求无退化并登记，不引入新架构协议；
- backend 不接收 task、fd table、raw Linux mode/dev_t、capability 或 provider handle；
- 不实现 named FIFO、device provider open、pathname socket endpoint、legacy `readdir` 或 overlayfs whiteout；
- 不为单个 LTP pathname、filesystem、provider 或执行顺序增加 production 特判；
- 不把 `sys_umask` stub 扩张为独立全系统项目；requested permission bits 直接进入 existing common-create
  handoff，只复用现有 owner/group policy。若不扩大 task/全部 create surface 就无法形成这一 R1 final metadata，
  命中停止条件并上报，而不是顺带实现 umask。

下面三个checkpoint的顺序、manifest与停止边界均已冻结；任何source/docs路径越界先停止并报告。

### 7.2 Checkpoint C1 — Behavior-preserving VFS/inode module split

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

本checkpoint的source write set只允许：

- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/fs/vfs/{mod.rs,ops.rs}`（new）
- `anemone-kernel/src/fs/inode.rs`（delete）
- `anemone-kernel/src/fs/inode/{mod.rs,ops.rs,metadata.rs,object.rs}`（new）

closure 必须记录：

- 对 old/new symbol 的一一移动审计，以及精确 `rg` 的 module declaration/re-export/visibility sweep；
- `just fmt kernel --check`、`git diff --check`、
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`和
  `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G`；
- review 确认没有 owner/public API/contract delta、没有趁拆分重构相邻代码、现有 inline KUnit 仍由原 owner 编译；
- transaction 中独立的 activation、commit/diff、review 和 closure；`VFS-MAKE-NODE-CUTOVER` 继续 Not Cut Over。

若拆分需要改变 public surface、跨 owner 移动状态或增加共享抽象，checkpoint 立即停止并走 write-set/设计扩展。
checkpoint关闭不自动授权C2；C1不运行QEMU/LTP，不修改current contract，`VFS-MAKE-NODE-CUTOVER`保持Not Cut Over。

### 7.3 Checkpoint C2 — Merged make-node feature implementation

C2把syscall/VFS和ext4/ramfs/mount实现合并为一个checkpoint；任何子集都不是可发布能力。路线固定如下：

1. RV64/LA64增加asm-generic `SYS_MKNODAT = 33`与`fs/api/mknodat.rs`。adapter使用现有`RawAtFd`/`AtFd`规则，
   因而absolute path忽略无效dirfd；独立normalizer处理type bits 0/REG/FIFO/CHR/BLK/SOCK、DIR=`EPERM`、
   LNK/invalid=`EINVAL`、CHR/BLK `CAP_MKNOD`和canonical Linux `dev_t` decode。不得改变credential owner；
   `task/credentials/cap.rs`只读复用。
2. inode owner新增窄`MakeNodeDescription { mode: InodeMode, uid: Uid, gid: Gid, rdev: DeviceId }`和
   `InodeOps::make_node(dir, name, description) -> Result<InodeRef, SysError>`。constructor/assertion保证只有Char/
   Block携带`DeviceId::Number`，其它kind为`None`。VFS在callback前完成parent、writable mount、DAC、requested
   permission和既有gid inheritance；regular `mknodat`也走`make_node`，不复用当前commit后补owner的`touch`。
3. non-directory parent在VFS dispatch前返回`ENOTDIR`。ext4/ramfs以外的directory initializer都显式接入共同窄
   `EPERM`拒绝函数；不得用success stub、filesystem-specific fallback或让pseudo-fs获得可写namespace。
4. lwext4新增backend-local make-node路径：allocate inode后依次写final mode、完整u32 uid/gid与适用rdev，最后
   `add_entry`；commit前任一步失败释放未链接inode及临时resource。C helper修复uid/gid high bits，Rust `FileAttr`
   增加rdev，`inode/attr.rs`使用u32 owner并暴露dev access；ext4 reload/getattr/sync读取并持久化同一owner/rdev。
   FIFO open返回`EOPNOTSUPP`，Char/Block/Socket返回`ENXIO`，不得保留`unimplemented!()`。
5. ramfs在现有write transaction内支持Regular/Fifo/Char/Block/Socket：final metadata在`seed_inode`与parent
   insertion前完成；private `RamfsSpecial`只保存category-neutral `DeviceId`，`Inode::ty()`仍是kind唯一truth。
   rollback沿现有transaction撤销；FIFO open返回`EOPNOTSUPP`，Char/Block/Socket返回`ENXIO`，不得进入
   `unreachable!()`或regular fallback。
6. `SysError`增加system-wide窄`ENOTBLK`语义映射；mount先检查Block kind与有效rdev，再构造`BlockDevNum`查询
   provider。non-block/missing block identity返回`ENOTBLK`，合法号码provider miss保持`ENOENT`。
7. `anemone-rs`增加durable raw与typed `mknodat` wrapper，作为accepted syscall ABI的真实用户态consumer，
   不是validation facade。C2同时准备现有user-test内的临时`vfs_make_node_probe`与focused LTP group；probe和
   profile选择只服务C3验证，必须在cutover前删除/恢复，durable wrapper保留。

C2 KUnit只放在对应semantic owner末尾，覆盖mode/dev/capability normalization、description合法性、ext4/ramfs
提交前rollback与reload/projection、special-open/ENOTBLK边界；不为机械initializer sweep新建tests文件。

C2 closure要求`just fmt kernel --check`、`git diff --check`、双架构release build、initializer/residual-panic/
raw-codec/provider-lookup source audit和完整diff review。C2不运行或声称最终runtime closure，不删除临时probe，
不修改current contract；关闭后C3仍须独立授权。

### 7.4 Checkpoint C3 — Validation, probe removal, final review and cutover

C3先在temporary validation窗口运行证据，再删除probe/恢复profile，最后对exact production tree复验与cutover。

1. focused group临时包含`mknod01`至`mknod09`、`mknodat01`、`mknodat02`与`mount02`，glibc/musl和RV64/LA64
   分别记录PASS/FAIL/TCONF/BROK。不得把全部case机械要求PASS：逐case对照R1 node-kind/dev/dirfd/capability/
   errno/no-umask target分类；依赖named FIFO I/O、legacy ownership/SGID或完整Linux umask的out-of-target失败单独
   记录，不得冒充in-target closure，也不得用其扩大RFC。任一in-target失败阻塞cutover。
2. temporary `vfs_make_node_probe`通过durable wrapper覆盖ext4与ramfs的Regular/Fifo/Char/Block/Socket、requested
   mode/uid/gid/rdev、stat/statx/getdents/unlink、duplicate、bad pointer/path/relative dirfd、absolute-path invalid
   dirfd、RO mount、CAP_MKNOD与char 0:0、regular I/O、special open errno、unknown provider independence、
   ext4 eviction/remount-reload。rollback fault point无法安全注入时可由owner-local KUnit证明，但真实失败后lookup
   无残留至少要有用户态证据。
3. 依次运行：

   ```text
   just fmt kernel --check
   just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
   just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
   ./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/vfs-make-node-stage2-rv64.log
   ./scripts/run-user-test-la64.sh etc/preliminary/images/sdcard-la.img build/vfs-make-node-stage2-la64.log
   ```

   wrapper只写worktree-local runtime disk/log，不直接修改只读master。build通过不替代runtime；两架构runtime均为
   R1 acceptance floor，未运行或infra failure必须如实记录且不得cutover。
4. runtime证据读取完成后，删除临时probe文件与`vfs-make-node` LTP group/registration，恢复`main.rs`和
   `ltp/profile.txt`到长期状态；保留真实ABI consumer的`anemone-rs` wrapper。随后重跑format、双架构release build、
   `git diff --check`与production-source audit，确认无probe、test-path dispatch、provider lookup、raw packed truth、
   panic/success stub或遗漏initializer。
5. final review分别检查owner/API、backend-local commit/rollback、existing common-create handoff无退化、reload、
   ABI/errno、resource cleanup和validation provenance。`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`保持独立Open；
   它不因Stage 2关闭而解决，也不阻塞R1，但任何Stage 2新增failure window仍是finding。

只有全部in-target target、两backend、双架构runtime、错误矩阵、probe删除、final review与docs write-back闭合，
`VFS-MAKE-NODE-CUTOVER`才原子Introduce `VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`并Refine
`VFS-MOUNT-ADMISSION-002`。cutover同一write-back新增`contracts/vfs/make-node.md`、更新mount admission与
navigation/status；任一必要项失败时三项全部Not Cut Over，不把syscall注册、单backend或单架构结果写成partial
contract。

`just test xtask`默认是optional repository-health check；只有实际修改Justfile、`scripts/xtask`或build config时
才升级为mandatory。硬件、final harness、named FIFO/device/socket data plane与legacy `readdir`均Not Run/非目标。

### 7.5 停止条件

出现以下证据时 Stage 2 必须在 cutover 前停止：

- callback 前 final owner/permission 无法由 existing VFS owner 形成，需扩大 shared create contract；
- 需要读取 `sys_umask` stub、建立 task/fs-state mask owner或修改其它创建类调用点才能继续；
- ext4/ramfs无法在dirent commit前完成final metadata与提交前rollback，或需要接受cache-only `rdev`；
- make-node新增比现有touch/mkdir更弱的post-backend failure path。仅当关闭的是既有common-create publication窗口且
  需要不小的跨owner架构改造时，记录/更新开放问题后继续，不把它误判为本RFC必须修复；
- 需要改变 `InodeOps::make_node` owner、node-kind/ABI/errno matrix、ext4+ramfs acceptance boundary；
- 需要 FIFO/device/socket 数据面、通用 provider resolver、opened-description handoff 或新生命周期协议；
- 只能通过缩减合法 Linux encoding、静默创建错误 kind、保留 panic 或长期 validation facade 才能完成。

保持 target 的内部 route、预计文件、验证安排或行数变化可以回写本文后重新 review；target/owner/ABI/contract/
visible semantics/acceptance boundary 变化必须进入 RFC review / `Target Renegotiation Gate`，agent 无权自行批准。

### 7.6 Resolved Write Set Manifest

#### C1 source manifest

C1只允许第7.2节列出的`fs/mod.rs`、`fs/vfs/`与`fs/inode/`old/new path。

#### C2/C3 production manifest

- `anemone-abi/src/syscall/{riscv,loongarch}.rs`
- `anemone-kernel/src/fs/api/{mod.rs,mknodat.rs}`
- C1形成的`anemone-kernel/src/fs/{mod.rs,vfs/{mod.rs,ops.rs},inode/{mod.rs,ops.rs,metadata.rs,object.rs}}`
- `anemone-kernel/src/fs/ext4/{mod.rs,inode.rs,superblock.rs}`
- `anemone-kernel/src/fs/ramfs/{mod.rs,inode.rs}`
- `anemone-kernel/src/{syserror.rs,fs/api/mount/mount.rs}`
- `anemone-kernel/crates/anemos/lwext4-rust/src/{fs.rs,inode/attr.rs}`
- `anemone-kernel/crates/anemos/lwext4-rust/c/lwext4/src/ext4_inode.c`
- `anemone-rs/src/{sys/linux/fs.rs,os/linux/fs.rs}`

新增`InodeOps::make_node`的机械initializer manifest在2026-08-01冻结为以下26个文件（32个static）；其中与上面
semantic owner重复的路径不代表第二次授权：

- `anemone-kernel/src/device/{console.rs,tty/endpoint.rs}`
- `anemone-kernel/src/fs/anonymous/anony_fs.rs`
- `anemone-kernel/src/fs/devfs/inode.rs`
- `anemone-kernel/src/fs/epoll/file.rs`
- `anemone-kernel/src/fs/eventfd/mod.rs`
- `anemone-kernel/src/fs/ext4/inode.rs`
- `anemone-kernel/src/fs/fanotify/file.rs`
- `anemone-kernel/src/fs/pipe.rs`
- `anemone-kernel/src/fs/proc/{meminfo.rs,pde.rs,uptime.rs}`
- `anemone-kernel/src/fs/proc/root/inode.rs`
- `anemone-kernel/src/fs/proc/tgid/{cmdline.rs,cwd.rs,environ.rs,exe.rs,fd.rs,inode.rs,mounts.rs,root.rs,stat.rs,status.rs}`
- `anemone-kernel/src/fs/ramfs/inode.rs`
- `anemone-kernel/src/fs/socket/udp/mod.rs`
- `anemone-kernel/src/fs/timerfd/mod.rs`

`anemone-kernel/src/task/credentials/cap.rs`、`anemone-abi/src/fs.rs`、stat/statx/getdents与wrapper scripts只作为
只读审计/validation input；若实现证据表明必须修改，先走expansion，不能静默扩大manifest。

#### Validation-only temporary manifest

- `anemone-apps/user-test/src/{main.rs,vfs_make_node_probe.rs}`（probe new，C3删除）
- `anemone-apps/user-test/src/ltp/config.rs`
- `anemone-apps/user-test/ltp/{profile.txt,groups/vfs-make-node.txt}`（group new，C3删除）

#### Documentation/cutover manifest

- `docs/src/contracts/vfs/{make-node.md,mount-admission.md,index.md}`（`make-node.md`只在C3 cutover新增）
- `docs/src/{SUMMARY.md,rfcs.md}`
- `docs/src/rfcs/vfs-make-node/{index.md,invariants.md,implementation.md,tracking-issues.md}`
- `docs/src/register/{open-issues.md,current-limitations.md}`
- `docs/src/devlog/transactions/{2026-07-31-vfs-make-node.md,index.md}`
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`

C2/C3在上述stage-wide production manifest内修复review finding不算expansion；新增owner/public API（除已冻结
`InodeOps::make_node`与durable userspace wrapper外）、shared contract、ABI/visible semantics或任何其它路径均先
停止并报告。formatter只可对manifest内目标Rust文件产生改动。

## 8. Write-set expansion 与执行反馈

- Stage 2 Ready/Active越过第7.6节manifest时，先停止并报告原因、候选路径、owner/contract影响和验证计划；
  批准后先更新本文，再由transaction记录批准事实与链接。
- C1关闭不激活C2，C2关闭不激活C3；checkpoint activation、closure与commit/diff证据只写transaction。
- implementation feedback 若只改变保持 target 的路线、manifest、验证、停止条件或顺序，写回本文和 transaction；
  执行事实只写 transaction；accepted target 变化先回 RFC review；effective shared rule 只在获准 cutover 更新。
- probe 计划留在本文；执行结果留在 transaction。除证据包过长且有具体主题外，不新建通用 `probe.md`、
  `feedback.md` 或 `experiments.md`。
- 任一 stage 关闭后都要扫描 RFC header、implementation maturity、transaction、current contract、devlog、register
  和 navigation，避免只更新一处状态。未运行的 architecture/runtime/filesystem proof 始终保留 Not Run。
