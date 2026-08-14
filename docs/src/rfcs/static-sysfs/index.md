# RFC-20260814-static-sysfs

**状态：** Closed
**修订：** R1
**负责人：** doruche
**最后更新：** 2026-08-14
**领域：** fs / sysfs / VFS mount / architecture facts
**影响契约：** `SYSFS-STATIC-001`、`SYSFS-MOUNT-001`（均已通过 `STATIC-SYSFS-CUTOVER` Introduce / Active）
**执行记录：** [2026-08-14 Static sysfs transaction](../../devlog/transactions/2026-08-14-static-sysfs.md)

## 摘要

本 RFC 为 Anemone 引入一个最小、静态、可由用户挂载的 `sysfs`。首版只建立静态目录与只读 ASCII
文本属性框架，并以 `/sys/kernel/address_bits` 和 `/sys/kernel/cpu_byteorder` 两个真实 consumer
完成纵向验证；它不接入现有 `KObject` / `KSet`，也不尝试解决动态注册、删除、重命名、hotplug 或
device model。

本目标有意小于 Linux sysfs，但不是仅接受 mount 的 dormant skeleton。filesystem registration、持久
singleton tree、目录枚举、文本属性读取和两个 consumer 在同一个 `STATIC-SYSFS-CUTOVER` 原子交付，
从而用真实 shell 观察证明框架具备可消费的最小语义。

## 背景

当前 Anemone 已有 canonical no-device filesystem registry、mount admission、persistent singleton procfs
以及多 mount view 所需的 VFS 基础，但没有注册 `sysfs`，也没有 `/sys` 可观察面。现有
`anemone-kernel/src/device/kobject.rs` 仍把 sysfs attribute operations 标为 TODO；其对象模型形成较早，
本 RFC 不把它未经独立 review 的设计当作新 sysfs 的前提。

procfs 的静态部分已经展示了适合当前 VFS 的代码形状：module root、persistent superblock、root
inode/file、静态 entry metadata 和按 consumer 分目录组织。sysfs 可以参考这一职责拆分，但两者的
owner 与可见协议不同：sysfs 不复用 procfs 私有 `ProcDirEntry` / PDE 类型，也不因局部代码相似提前
抽取 generic pseudo-filesystem framework。

Linux 6.6.32 的 `/sys/kernel/address_bits` 与 `/sys/kernel/cpu_byteorder` 是简单、稳定的只读文本
属性；Linux sysfs 文档还把 attribute 定位为通常每文件一个值的 ASCII 文本，并要求读取形状支持
partial read、forward seek 和从 offset 0 重新读取。本 RFC 采用这组窄表面作为兼容参照，不继承 Linux
的 kernfs、kobject lifetime 或动态 attribute 协议。

## 目标

- 注册唯一 canonical name 为 `sysfs` 的 no-device filesystem，使其在 `/proc/filesystems` 中显示为
  `nodev\tsysfs`。
- 建立 process-lifetime persistent singleton superblock 与静态树；多个 mount 是同一棵树的 VFS
  view，unmount 只移除 view，不销毁或重建静态 namespace。
- 在 userspace 启动前完整发布首版拓扑；运行期不增加、删除或重命名 entry。
- 首版 entry 只包含目录与只读 ASCII 文本属性。普通 read、positioned read 与 seek 支持 partial
  consumption，并在每次属性快照生成时从 consumer owner 读取当前值。
- 原子发布 `/sys/kernel/address_bits` 与 `/sys/kernel/cpu_byteorder` 两个真实 consumer；RV64 和
  LA64 的精确内容分别为 `64\n` 与 `little\n`。
- 让 namespace 修改与 attribute write fail closed，不出现看似成功但无语义的操作。

## 非目标

- 修改、替换或扩展 `KObject` / `KSet`，把 kobject 接入 sysfs，或重新设计 device model；
- runtime registration、removal、rename、hotplug、uevent，以及任何动态 namespace lifecycle；
- `/sys/devices`、`/sys/bus`、`/sys/class`、`/sys/block`，以及 symlink、binary attribute、writable
  attribute；
- 自动创建或挂载 canonical `/sys`；本 RFC 只提供可挂载 filesystem，mountpoint 由 userspace
  准备；
- loop sysfs、partscan、partition node 或对现有 loop ioctl 限制的收口；
- 修复 generic dynamic pseudo-filesystem positive-dentry freshness / revocation 协议；首版冻结树不
  创建该动态 handoff；
- 修复 generic `chmod` / `chown` / `utimensat` 到 filesystem owner 的 metadata mutation handoff，或
  承诺 mutation 后的 stat/DAC 同步、拒绝 errno 与 persistence；该既有缺口由 register 独立跟踪；
- 提前形成通用 kernfs、通用 dynamic pseudo-filesystem framework，或仅为未来 consumer 预留 public
  registration API。

## Owner 与协议边界

- **Protocol/state owner：** sysfs 拥有静态 namespace topology、boot publication 时的 initial entry
  metadata、inode/superblock、readdir 与只读 attribute file protocol。VFS 继续拥有 pathname
  resolution、dentry/cache、mount view 与当前 generic inode-metadata syscall 路径；R1 不复制这些状态，
  也不闭合 metadata mutation 到 filesystem owner 的既有缺口。
- **Consumer truth：** 每个 attribute 的语义值由对应 consumer owner 唯一拥有。sysfs entry 只保存
  name、kind 和窄只读 getter capability；它在生成一次读取快照时调用 getter，不长期缓存
  `address_bits` 或 `cpu_byteorder` 的第二份状态。
- **Publication / handoff：** boot-time init 先校验完整静态 entry 描述，再通过既有 VFS registration
  取得唯一 canonical `FileSystem` identity，并以该 identity 构造、seed 和安装 singleton superblock。
  registry registration 仍是 `VFS-MOUNT-ADMISSION-004` 所定义的 filesystem-type publication 事实；
  registration 与 singleton 安装共同构成同一个串行、pre-userspace boot transaction，只有 init 成功返回后
  才形成可由 userspace mount 的完整 sysfs 能力。本 RFC 不引入 staged registry API、第二个未注册
  `FileSystem` identity 或 rollback/unregister 协议，也不得让空 filesystem 或部分 consumer 越过该 boot
  transaction 到达 userspace。
- **Mount lifecycle：** mount 取得既有 singleton superblock 并创建 VFS view。一个 view 的 unmount
  不改变其它 view；最后一个 view 的 unmount 也不释放 static inode identity 或 entry topology，之后
  remount 仍观察同一棵树。
- **Failure / cleanup：** non-empty mount data 在建立 view 前返回 `EINVAL`。boot-time 校验、registration、
  构造、seed 或 singleton 安装失败时立即 panic 并在初始 userspace 前终止启动；即使失败发生在 registry
  registration 之后，也不得让系统带着部分 sysfs 继续启动，因此不要求为 boot-fatal 路径新增 unregister
  或局部 rollback。正常 unmount cleanup 由 VFS 回收 view，sysfs 的 process-lifetime tree 无
  teardown/retry 协议。

## ABI 与可见语义

- filesystem type 的 canonical ABI name 是 `sysfs`；它是 no-device filesystem。raw mount source label
  由既有 VFS no-device admission 丢弃，不形成另一份 identity。
- empty mount data 成功，non-empty mount data 返回 `EINVAL`。
- mount 后根目录只承诺 `kernel/`；`kernel/` 首版只承诺 `address_bits` 与 `cpu_byteorder`。boot
  publication / 未发生 metadata mutation 时两个目录的 mode 为 `0555`，两个 attribute 的 mode 为
  `0444`；目录项顺序不是 ABI。R1 不承诺 `chmod` / `chown` / `utimensat` 后的 stat/DAC 同步、拒绝
  errno 或 persistence。
- 两个 attribute 是只读普通文件，内容分别为 `64\n` 与 `little\n`。普通 read 或 pread 每次调用 getter
  生成一份完整文本 snapshot，再按该次操作的 offset 截取；跨多次 read/pread 不承诺锁定同一份
  consumer snapshot，seek 回 offset 0 或 pread offset 0 会重新生成。首批两个 architecture facts 在本
  target 内为 immutable，因此不会产生跨调用拼接不一致。
- `SEEK_SET` / `SEEK_CUR` / `SEEK_END` 允许形成任意不溢出的非负 offset，包括 EOF 之后；从 EOF 或
  EOF 之后 read/pread 返回 `0`。负结果和 offset 计算溢出按既有 VFS seek 失败约定处理，本 RFC 不新增
  精确 errno 保证。
- 创建目录或其它 node、link/unlink、rename、truncate 和 attribute write 均不得成功。具体 errno
  复用当前 VFS 与只读 pseudo-filesystem 的既有 fail-closed 约定；除 mount-data `EINVAL` 外，本 RFC
  不新增 Linux errno 保证。
- 本 RFC 不声称兼容 Linux sysfs 的 kobject、kernfs、dynamic lifetime、PAGE_SIZE show buffer、poll
  notification 或 writable attribute ABI。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `SYSFS-STATIC-001` | Introduce | [Active](../../contracts/sysfs/static-filesystem.md#sysfs-static-001--冻结-namespace-与只读-text-attribute) | 冻结 static tree；目录和只读 ASCII 文本属性；consumer owner 通过窄 getter 提供唯一真相 | `STATIC-SYSFS-CUTOVER`（Effective） |
| `SYSFS-MOUNT-001` | Introduce | [Active](../../contracts/sysfs/static-filesystem.md#sysfs-mount-001--canonical-type与persistent-singleton-lifetime) | canonical `sysfs` no-device type；persistent singleton superblock；multi-view 与 last-unmount/remount lifetime | `STATIC-SYSFS-CUTOVER`（Effective） |

### Dependencies

- [`VFS-MOUNT-ADMISSION-001`](../../contracts/vfs/mount-admission.md#vfs-mount-admission-001--canonical-filesystem-identity)：
  canonical fstype 与 mount/proc projection identity。
- [`VFS-MOUNT-ADMISSION-002`](../../contracts/vfs/mount-admission.md#vfs-mount-admission-002--source-kind-owned-admission)：
  no-device source 与 typed callback handoff。
- [`VFS-MOUNT-ADMISSION-003`](../../contracts/vfs/mount-admission.md#vfs-mount-admission-003--syscall-only-alias-containment)：
  不为 `sysfs` 建立 syscall alias 或 backend-side alias。
- [`VFS-MOUNT-ADMISSION-004`](../../contracts/vfs/mount-admission.md#vfs-mount-admission-004--registered-filesystem-projection)：
  `/proc/filesystems` 从 registry 与 tagged mount operation 派生 `nodev` 展示。
- [`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)：
  dynamic namespace freshness 仍由 VFS 独立拥有；本 RFC 的 frozen topology 不规避、不扩展也不关闭该问题。
- [`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`](../../register/open-issues.md#ane-20260814-vfs-metadata-mutation-owner-handoff)：
  generic metadata syscall 的 owner handoff 与 mutation 后 projection/persistence 由后续独立 VFS 工作
  解决；R1 只证明未 mutation 的 initial metadata。

## Implementation Boundary

- **允许改变：** 新增 static sysfs owner、filesystem registration、persistent singleton tree、root 与
  `/kernel` 目录行为、read-only text attribute protocol、两个 architecture-fact getter 的窄 handoff，
  以及闭合这些职责所需的同 owner module registration、定向测试与 shell validation 支持。
- **必须保持：** 现有 kobject/device model、VFS pathname/dentry/mount owner、mount admission current
  contract、procfs visible behavior、loop/partition semantics，以及“不自动挂载 `/sys`”的 userspace
  边界。不得新增 generic metadata mutation hook、fstype 特判或 sysfs-local workaround，也不得把
  Accepted target 写成 current contract 或当前实现事实。
- **实现提示：** 新代码放在结构化的 `anemone-kernel/src/fs/sysfs/` 目录，参考现有 procfs 按稳定职责
  拆分，例如 module root、root inode/file、static entry、superblock 和 `kernel/` consumer。该目录形状
  是非穷举提示，不是逐文件 write set，也不固定具体 Rust 类型或 helper。不得复用 procfs-owned
  `ProcDirEntry` / PDE；若两套实现只有局部形状相似，保持 owner-local direct code，不提前抽取 generic
  pseudo-fs abstraction。
- **原子交付：** framework 与两个 consumer 共用一次 `STATIC-SYSFS-CUTOVER`；boot transaction 可以按
  既有 VFS owner 顺序先 registration 再安装 singleton，但不得让已经注册却没有完整 consumer 的
  dormant sysfs 到达 userspace，也不得把某一个 architecture 的 partial activation 作为 cutover。
- **停止条件：** 若实现需要 sysfs-owned runtime namespace/attribute mutation、kobject integration、
  public registration API、generic dentry freshness 或 metadata-mutation handoff 修改、自动挂载、
  writable/binary attribute、symlink、其它首批 consumer，或要改变 owner/handoff/failure/cleanup、ABI、
  contract delta、acceptance、architecture coverage / validation claim，则在 cutover 前停止并回到 RFC
  review / Target Renegotiation。

## Acceptance 与 Validation

R1 target、Implementation Boundary与contract delta已在唯一`STATIC-SYSFS-CUTOVER`中实现并验证；以下closure
obligation均已满足：

- source audit 证明 boot transaction 先校验完整描述，只使用 registration 返回的唯一 canonical
  `FileSystem` identity 构造和 seed singleton，并在 init 返回前安装完整 topology；attribute value 不被
  sysfs 缓存为第二真相，代码没有 staged registry、未注册平行 identity、`KObject` / `KSet` 接入或
  procfs private entry 复用；
- repository-native RV64 与 LA64 release build 均通过；
- RV64 与 LA64 各进入一次短 shell，显式准备 mountpoint 并 mount `sysfs`，确认 `ls /sys`、
  `ls /sys/kernel`，以及两个文件的精确内容分别为 `64\n`、`little\n`；
- 两个架构均用实际 read/seek consumer 验证 partial read、pread、seek 回零重新读取、EOF 与 EOF 后
  seek/read，而不只用一次 `cat`；
- 两个架构均建立第二个 mount view，确认内容一致；unmount 一个 view 后另一 view 仍可读；最后一个
  view unmount 后 remount，内容和 namespace 仍可读；
- 两个架构均在未执行 metadata mutation 时确认目录 mode 为 `0555`、attribute mode 为 `0444`，确认
  mkdir/node creation 与 attribute write失败，并确认 `/proc/filesystems` 包含精确的
  `nodev\tsysfs` registry projection；
- non-empty mount data 的 `EINVAL` 由 owner-local test 或等价的定向 guest probe 证明。

定向guest只证明本 RFC 的 static mount/read-only surface；它不证明 kobject integration、dynamic sysfs、device model、
hotplug、loop sysfs、generic dentry revocation或metadata mutation后的owner/projection/persistence。实际双架构build/runtime、
KUnit、source audit、R1决定与文档证据见[transaction](../../devlog/transactions/2026-08-14-static-sysfs.md)。

## 风险与反馈

- **静态框架被误当成未来动态框架。** 控制方式是不给出 runtime registration API，并把 kobject、
  symlink 与 removal lifecycle 保持为明确非目标；未来动态需求按当时 live owner model 独立分级。
- **为复用 procfs 而泄漏 owner。** 控制方式是只参考目录职责形状，保留 sysfs-local entry protocol；
  generic 抽取必须有多个真实 consumer 的共同语义与独立边界，不能由本 RFC 顺手完成。
- **persistent singleton 与 VFS view 生命周期错配。** 失败信号是最后一个 unmount 后 inode/tree identity
  被清空、其它 view 被影响或 remount 内容丢失；multi-view/unmount/remount 是 cutover 必需证据。
- **把 boot transaction 误建模为 VFS 两阶段 publication。** 失败信号是新增 staged registry API、构造
  未注册的平行 `FileSystem` identity，或为了 boot-fatal 初始化失败引入 unregister/rollback；实现应使用
  registration 返回的唯一 identity，并在初始 userspace 前完成 singleton 安装或终止启动。
- **attribute getter 形成缓存或宽依赖。** 失败信号是 sysfs 长期保存 consumer value、读取依赖完整
  architecture/device object，或 consumer 反向依赖 sysfs private type；实现必须收敛为窄 getter。
- **只验证 `cat` 掩盖 file-position 错误。** partial read、pread、seek 与 EOF 必须由实际 shell helper
  或小型 user test 覆盖。
- **generic metadata mutation 缺少 filesystem owner handoff。** static procfs 已有 descriptor/meta
  分裂，sysfs 也是受影响 consumer。R1 按维护者决定不在本 RFC 建立局部修复；失败信号、owner、影响与
  系统退出条件由 register 的独立 VFS issue 拥有，不降低本轮 initial metadata oracle。

## 文档与证据

- current mount baseline：[VFS Mount Admission 当前契约](../../contracts/vfs/mount-admission.md)。
- 明确排除的 dynamic VFS issue：
  [`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)。
- 明确排除的 metadata mutation owner issue：
  [`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`](../../register/open-issues.md#ane-20260814-vfs-metadata-mutation-owner-handoff)。
- 明确不由本 RFC 关闭的 loop limitation：
  [`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`](../../register/current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)。
- commit / PR / transaction：[2026-08-14 Static sysfs transaction](../../devlog/transactions/2026-08-14-static-sysfs.md)；
  commit由本RFC原子closure提交拥有。
- 外部源码证据：`xref:linux-6.6.32:kernel/ksysfs.c#ksysfs_init`、
  `xref:linux-6.6.32:kernel/ksysfs.c#address_bits_show`、
  `xref:linux-6.6.32:kernel/ksysfs.c#cpu_byteorder_show`、
  `xref:linux-6.6.32:fs/sysfs/file.c#sysfs_kf_seq_show`、
  `xref:linux-6.6.32:Documentation/filesystems/sysfs.rst#Attributes`。

## 修订记录

| 修订 | 日期 | 变化 | 证据 |
| --- | --- | --- | --- |
| R0 | 2026-08-14 | 接受最小 static sysfs target；明确既有 VFS registration 到 singleton 安装的 pre-userspace boot transaction、每次 read/pread snapshot、EOF 后 seek/read 与 `0555` / `0444` mode。 | RFC review；`git diff --check`；`mdbook build docs` |
| R1 | 2026-08-14 | 接受 generic metadata mutation owner handoff 不属于本 RFC；`0555` / `0444` 限定为未发生 metadata mutation 的 initial projection，post-mutation stat/DAC/errno/persistence 进入 register 独立系统解决，不阻塞 static mount/read-only cutover。 | 维护者决定；live sysfs/procfs/devfs/devpts/VFS 与 Linux procfs/kernfs source audit |

## Closure

R1已在唯一`STATIC-SYSFS-CUTOVER`中原子关闭。实现交付canonical no-device `sysfs`、persistent singleton static tree、
目录/只读text attribute protocol以及`address_bits`/`cpu_byteorder`两个consumer；没有扩展public API或generic VFS contract。
RV64与LA64 release build、全部enabled KUnit及定向guest acceptance通过；两架构均输出`STATIC-SYSFS:PASS`。LA64只在PASS与
完整orderly shutdown后因既有platform没有成功power-off handler进入halt，该末尾处置不削弱sysfs acceptance。

[`SYSFS-STATIC-001` / `SYSFS-MOUNT-001`](../../contracts/sysfs/static-filesystem.md)已成为Active current contract。register新增
`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`并保持Open / Deferred；dynamic positive-dentry issue与loop limitation也未
关闭。dynamic sysfs、KObject/KSet、device model、hotplug、自动挂载、symlink、binary/writable attribute、其它consumer和
generic pseudo-fs/VFS扩展均Not Cut Over。metadata owner缺口按维护者R1决定不阻塞本target，且未形成sysfs-local workaround。
最终独立review与Architecture Friction Scan的证据及处置记录在transaction/提交中；不存在下一gate。本页从此冻结，后续工作
必须从live source、current contract和register建立新的Implementation Boundary。
