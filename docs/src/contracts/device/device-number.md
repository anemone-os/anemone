# Device Number Namespace 当前契约

**Contract ID：** `DEVICE-NUMBER`
**状态：** Active
**Owner：** device-number identity protocol；`device::devnum` 拥有 numeric bounds，character/block registry
各自拥有本 namespace 的 membership
**参与领域：** device producers / char registry / block registry / devfs / VFS metadata ABI projection
**覆盖范围：** 当前 12-bit major + 20-bit minor internal domain、category-neutral inode number、char/block typed
registry key separation、endpoint-owned device number、producer-owned allocation/name 与 registry-derived key
**不覆盖：** Linux `mknodat` syscall input admission、ordinary-filesystem special-node `rdev`、device-provider resolver、
devfs/TTY publication lifecycle、hotplug/unpublish、provider replacement或 block I/O
**实现位置：** `anemone-kernel/src/device/{devnum.rs,char/mod.rs,block/mod.rs}`、
`anemone-kernel/src/fs/inode/metadata.rs`
**依赖：** None
**Pending Successor：** None；VFS Make Node R2 `DEVICE-NUMBER-CUTOVER` 已生效
**Review Context：** VFS Make Node R2 Closed；Stage 1-2 与 C1-C3 均已关闭
**最后核验：** 2026-08-01

本页最初由 2026-07-31 `DEVICE-NUMBER-BASELINE-EXTRACTION` 从 live owner 提取 16/16 baseline；2026-08-01
`DEVICE-NUMBER-CUTOVER` 随 VFS Make Node R0 Stage 1 原子迁移 code、consumer、review、validation 与 current
contract，以下 12/20/category-neutral 规则现已生效。该 cutover 不实现 `mknodat` 或 filesystem-backed special
node，也不改变 Stage 2 的 Not Active 状态。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| major/minor internal numeric bounds | `device::devnum` | producer 与 ABI codec 使用 checked number | 构造 category-neutral identity 与 typed registry key |
| character endpoint device number | 对应 `CharDev` capability | char registry在注册时读取 immutable value | char namespace registration/lookup |
| block endpoint device number | 对应 `BlockDev` capability | block registry在注册时读取 immutable value | block namespace registration/lookup |
| producer-local number/name policy | concrete device producer | registry只接收结果并校验唯一性 | static major、local minor、canonical name |
| character registry membership | char registry | devfs/open consumer持 typed key或endpoint capability | char provider lookup |
| block registry membership | block registry | mount/devfs/open consumer持 typed key或handle | block provider lookup |
| category-neutral inode `fs_dev` / `rdev` number | generic inode metadata | inode kind单独拥有 Char/Block category；consumer按 kind构造typed key | metadata identity，不参与 registry category裁决 |
| Linux packed `dev_t` codec | `anemone_abi::fs::linux::dev_t` | stat/loop等ABI边界只传 primitive major/minor | 唯一12/20 packed layout owner |
| Linux `stat` / `statx` projection | VFS ABI boundary | 只读消费 inode `DeviceId` snapshot | stat调用canonical codec；statx直接投影major/minor |

<a id="device-number-001--1616-typed-device-number-namespace"></a>

## DEVICE-NUMBER-001 — 12/20 category-neutral device-number domain

**规则：** 当前 internal device-number domain 由 12-bit `MajorNum` 与 20-bit `MinorNum` 组成，构造时必须以
release有效的 correctness assertion拒绝越界值。`DeviceNumber` 只表达 category-neutral numeric pair；
`CharDevNum` 与 `BlockDevNum` 分别包装同一数值形状，即使包含相同 pair，也属于两个互不替代的 typed registry
key。char 与 block registry 不共享 key space，也不能仅凭 numeric pair 跨 namespace lookup。

每个 `CharDev` / `BlockDev` endpoint capability 的 `devnum()` 是该 endpoint number 的唯一行为真相源。
concrete producer 拥有 static major、producer-local minor allocation 与 canonical name policy；registry 在自己的
锁内从 capability 派生 key，校验 device-number/name 唯一性并保存 membership，但不分配 major/minor、生成名称
或接受调用者提交的并列第二份 device number。

通用 inode metadata 通过 `DeviceId::{None, Number(DeviceNumber)}` 表达 `fs_dev` / `rdev`；其中不保存 Char/Block
category或 packed/raw escape，immutable inode kind 是 device-file category 的唯一 truth。char/block FileOps 与
mount consumer必须先检查 inode kind，再从 numeric number构造对应 typed registry key。mount 的 non-block
visible errno与 block provider-miss语义仍由 `VFS-MOUNT-ADMISSION-002` 拥有，本次 cutover 不提前 refine。

Linux 12/20 packed layout只由 `anemone_abi::fs::linux::dev_t::{encode, decode}` 拥有。`stat` 与 loop ioctl 在明确
ABI boundary调用该 codec；`statx` 直接投影结构化 major/minor。generic metadata、registry 与 producer不得保存
packed integer、复制位运算或重新建立行为性 `raw()` escape。

**违反表现：** internal constructor 接受超过 12-bit major或20-bit minor；generic inode保存 Char/Block tag或
packed raw value；char key 可直接用于 block lookup；
registry 与 endpoint各保存一份可漂移 device number；registry重新分配 major/minor或生成 producer name；同一
registry接受重复 number/name；consumer在检查 inode kind前选择 registry；或 ABI boundary复制 codec位运算。

**验证 / Enforcement：** `MajorNum` / `MinorNum` correctness assertion；12/20 boundary、canonical codec
representative/full-boundary round-trip、typed conversion、stat/statx/loop一致性KUnit；char/block registry
registration与consumer-order source audit；producer static number/name audit；RV64/LA64 release build。2026-08-01
RV64 wrapper完成282项KUnit、focused LTP 4/4与正常关机，覆盖现有devfs/TTY/device publication路径；最后的u32
codec签名收窄、private codec constants和mount kind检查前移由双架构build、source audit与开发者接受的无需重跑QEMU判断
闭合。LA64 runtime、`mknodat` 与filesystem node matrix均Not Run。

**最初来源：** [device devnum ownership 小迭代](../../devlog/changes/2026-07-22-device-devnum-ownership.md)及其
实现/验证证据。

**当前来源：** [VFS Make Node R0 Stage 1 transaction](../../devlog/transactions/2026-07-31-vfs-make-node.md) 的
`DEVICE-NUMBER-CUTOVER`；2026-07-31 baseline extraction只保留为历史来源。

## 相邻边界

- devfs、TTY、console与 concrete block producer可以投影 typed number，但各自 publication record、endpoint
  lifecycle和provider capability仍由原 owner负责。
- mount admission先消费 immutable inode kind，再从category-neutral number构造`BlockDevNum`进入block registry；
  其 source-kind与visible errno由
  [`VFS-MOUNT-ADMISSION-002`](../vfs/mount-admission.md#vfs-mount-admission-002--source-kind-owned-admission)
  拥有。
- VFS Make Node R0 的 `DEVICE-NUMBER-CUTOVER` 已完成；Stage 2 只能复用本页effective number/codec边界，不得
  重新打开device-number namespace或把Stage 1证据冒充`mknodat`实现证明。
