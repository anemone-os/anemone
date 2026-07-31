# Device Number Namespace 当前契约

**Contract ID：** `DEVICE-NUMBER`
**状态：** Active
**Owner：** device-number identity protocol；`device::devnum` 拥有 numeric bounds，character/block registry
各自拥有本 namespace 的 membership
**参与领域：** device producers / char registry / block registry / devfs / VFS metadata ABI projection
**覆盖范围：** 当前 16-bit major + 16-bit minor internal domain、char/block typed key separation、endpoint-owned
device number、producer-owned allocation/name 与 registry-derived key
**不覆盖：** Linux `mknodat` 12/20 input target、ordinary-filesystem special-node `rdev`、device-provider resolver、
devfs/TTY publication lifecycle、hotplug/unpublish、provider replacement或 block I/O
**实现位置：** `anemone-kernel/src/device/{devnum.rs,char/mod.rs,block/mod.rs}`、
`anemone-kernel/src/fs/inode.rs`
**依赖：** None
**Pending Successor：** [VFS Make Node R0 `DEVICE-NUMBER-CUTOVER`](../../rfcs/vfs-make-node/index.md)；
尚未生效
**Review Context：** VFS Make Node R0 已接受，Stage 1 Active；本页仍保持 16/16 effective baseline
**最后核验：** 2026-08-01

本页是 2026-07-31 `DEVICE-NUMBER-BASELINE-EXTRACTION` 从 live owner 提取的 current baseline。该 docs-only
提取没有修改代码，也没有把 VFS Make Node R0 的 12/20 target、category-neutral inode number 或
`DEVICE-NUMBER-CUTOVER` 写成 current fact。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| major/minor internal numeric bounds | `device::devnum` | producer 与 ABI projection 使用 checked number | 构造 registry key与内部 identity |
| character endpoint device number | 对应 `CharDev` capability | char registry在注册时读取 immutable value | char namespace registration/lookup |
| block endpoint device number | 对应 `BlockDev` capability | block registry在注册时读取 immutable value | block namespace registration/lookup |
| producer-local number/name policy | concrete device producer | registry只接收结果并校验唯一性 | static major、local minor、canonical name |
| character registry membership | char registry | devfs/open consumer持 typed key或endpoint capability | char provider lookup |
| block registry membership | block registry | mount/devfs/open consumer持 typed key或handle | block provider lookup |
| Linux `stat` / `statx` projection | VFS ABI boundary | 只读消费 inode `DeviceId` snapshot | userspace metadata projection |

## DEVICE-NUMBER-001 — 16/16 typed device-number namespace

**规则：** 当前 internal device-number domain 由 16-bit `MajorNum` 与 16-bit `MinorNum` 组成，构造时必须拒绝
越界值。`CharDevNum` 与 `BlockDevNum` 即使包含相同 numeric pair，也属于两个互不替代的 typed registry key；
char 与 block registry 不共享 key space，也不能仅凭 raw pair 跨 namespace lookup。

每个 `CharDev` / `BlockDev` endpoint capability 的 `devnum()` 是该 endpoint number 的唯一行为真相源。
concrete producer 拥有 static major、producer-local minor allocation 与 canonical name policy；registry 在自己的
锁内从 capability 派生 key，校验 device-number/name 唯一性并保存 membership，但不分配 major/minor、生成名称
或接受调用者提交的并列第二份 device number。

当前 VFS ABI projection 仍通过 `DeviceId::{Char, Block, Raw, None}` 表达 inode `fs_dev` / `rdev` 输入：
`Char` / `Block` 保存对应 typed key，`Raw` 保留既有 ABI-facing raw identity。普通 `stat` 对 typed char/block
number 在 Linux userspace boundary 使用 12-bit major + 20-bit minor `dev_t` layout，并对无法编码的 internal
major fail-fast；`statx` 直接投影分离的 major/minor。这个 ABI codec 与 `DeviceId` shape 是 current baseline，
不是未来 `mknodat` 输入范围、generic inode category ownership或 raw escape 的长期 target 承诺。

**违反表现：** internal constructor 接受超过 16-bit 的 major/minor；char key 可直接用于 block lookup；
registry 与 endpoint各保存一份可漂移 device number；registry重新分配 major/minor或生成 producer name；同一
registry接受重复 number/name；或 typed stat projection静默截断无法表示的 major。

**验证 / Enforcement：** `MajorNum` / `MinorNum` 与 typed key constructor 的 correctness assertion；
char/block registry registration source audit与 duplicate KUnit；producer static number/name source audit；
`stat` / `statx` projection KUnit。2026-07-22 的 closure 还包括 RV64 release build、KUnit与启动到
virtio-block publication的 runtime smoke；MMC endpoint因平台缺失保持 Not Run。

**最初来源：** [device devnum ownership 小迭代](../../devlog/changes/2026-07-22-device-devnum-ownership.md)及其
实现/验证证据。

**当前来源：** 同最初来源；2026-07-31 只将 live 16/16 baseline 提取到 contract 层，没有语义或代码 cutover。

## 相邻边界

- devfs、TTY、console与 concrete block producer可以投影 typed number，但各自 publication record、endpoint
  lifecycle和provider capability仍由原 owner负责。
- mount admission只消费 `DeviceId::Block`并进入 block registry；其 source-kind与 visible errno由
  [`VFS-MOUNT-ADMISSION-002`](../vfs/mount-admission.md#vfs-mount-admission-002--source-kind-owned-admission)
  拥有。
- VFS Make Node R0 接受的 12/20 common domain、category-neutral generic inode number与 raw escape removal
  只有在 `DEVICE-NUMBER-CUTOVER` 完成后才可更新本 ID；R0 acceptance/Stage 1 activation 本身不改变本页规则。
