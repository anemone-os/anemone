# Static Sysfs 当前契约

**Contract ID：** `SYSFS-STATIC-001` / `SYSFS-MOUNT-001`
**状态：** Active
**Owner：** `fs::sysfs` static namespace、initial entry metadata、inode/superblock 与 attribute file protocol；VFS 继续拥有 pathname、dentry、mount view 与当前 generic inode-metadata syscall path；architecture owner 继续拥有 attribute value
**参与领域：** sysfs / VFS mount / architecture facts / procfs filesystem projection
**覆盖范围：** canonical `sysfs` no-device type、persistent singleton backing、冻结的 `/kernel` namespace、只读 ASCII text attribute 与首批两个 architecture consumer
**不覆盖：** kobject/kset、runtime namespace registration/removal/rename、device model、hotplug、自动 `/sys` mount、symlink、binary/writable attribute、其它 consumer、generic dynamic-dentry freshness或metadata mutation owner handoff
**实现位置：** `anemone-kernel/src/fs/sysfs/`、`anemone-kernel/src/fs/mod.rs`、`anemone-kernel/src/arch/mod.rs`
**依赖：** [`VFS-MOUNT-ADMISSION-001`--`004`](../vfs/mount-admission.md)
**当前来源：** [RFC-20260814-static-sysfs R1](../../rfcs/static-sysfs/index.md) 与 [`STATIC-SYSFS-CUTOVER` transaction](../../devlog/transactions/2026-08-14-static-sysfs.md)
**最后核验：** 2026-08-14

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| static topology、entry kind、initial metadata 与 inode identity | sysfs | VFS view持既有inode/superblock引用 | lookup、readdir、initial stat与open |
| canonical filesystem identity 与 mount view | VFS registry / mount owner | sysfs以registration返回的唯一`FileSystem`构造backing | admission、projection与view lifetime |
| `address_bits` / `cpu_byteorder` | architecture owner | sysfs entry只持窄getter capability，不缓存值 | 每次read/pread/seek snapshot即时读取 |
| process-lifetime singleton backing | sysfs persistent superblock | 各mount持同一`Arc<SuperBlock>` view | multi-view、last-unmount与remount |

## SYSFS-STATIC-001 — 冻结 namespace 与只读 text attribute

**规则：** sysfs 在初始 userspace 前校验并一次 seed 完整静态树；runtime 不增加、删除或重命名 entry。当前根目录只承诺
`kernel/`，其下只承诺 `address_bits` 与 `cpu_byteorder`；boot publication且未发生generic metadata mutation时，目录mode为
`0555`、attribute mode为`0444`，目录顺序不是ABI。两个attribute分别返回ASCII `64\n`与`little\n`。普通read、pread与
seek每次操作都从architecture owner取得完整snapshot，再按offset截取；partial read、回零重读、`SEEK_END`、EOF与EOF后读取
均遵循普通文件位置语义。namespace structural mutation、truncate与attribute data write必须失败，不得silent success。

**违反表现：** userspace观察到部分seed的树；runtime entry增删/rename成功；sysfs缓存consumer value为第二真相；初始mode错误；
read/pread/seek不再调用getter；partial/EOF行为错误；或structural mutation、truncate和data write看似成功。`chmod`/`chown`/
`utimensat`后的projection不由本ID判定，见当前接受边界。

**验证 / Enforcement：** boot-time descriptor assertion；owner-local modes/readdir与read/pread/seek/EOF KUnit；RV64、LA64
定向guest在未执行metadata mutation时核对精确namespace、initial mode、内容、read protocol和fail-closed structural mutation/
truncate/data write。source audit确认未接入`KObject`/`KSet`、procfs private entry、动态registration或public consumer API。

**最初来源 / 当前来源：** [RFC-20260814-static-sysfs R1](../../rfcs/static-sysfs/index.md) 于2026-08-14通过
[`STATIC-SYSFS-CUTOVER`](../../devlog/transactions/2026-08-14-static-sysfs.md)原子引入。

## SYSFS-MOUNT-001 — Canonical type与persistent singleton lifetime

**规则：** `sysfs`是canonical no-device filesystem name，并按registry事实投影为`nodev\tsysfs`。empty mount data成功，
non-empty data在建立view前返回`EINVAL`；raw source label不形成backing identity。init先校验完整tree，再使用registration
返回的唯一`FileSystem` identity构造、seed并安装process-lifetime singleton superblock；任何失败在userspace前boot-fatal，
不建立并行identity、staged registry或rollback协议。所有mount只建立同一backing的VFS view；unmount一个view不影响其它
view，最后一个view unmount也不销毁static inode/tree，随后remount仍观察同一namespace。

**违反表现：** 出现第二个filesystem/backing identity；non-empty data被接受；`/proc/filesystems`缺项或分类错误；多view内容
分叉；一个unmount破坏其它view；last-unmount后inode消失或remount失败；或registered但未完整seed的sysfs到达userspace。

**验证 / Enforcement：** init publication source audit；`PERSISTENT_SB`与VFS inode residency source audit；owner-local singleton/
mount-data KUnit；RV64、LA64 guest分别验证registry projection、two-view isolation、surviving view及last-unmount/remount。

**最初来源 / 当前来源：** [RFC-20260814-static-sysfs R1](../../rfcs/static-sysfs/index.md) 于2026-08-14通过
[`STATIC-SYSFS-CUTOVER`](../../devlog/transactions/2026-08-14-static-sysfs.md)原子引入。

## 当前接受边界

- generic `chmod`/`chown`/`utimensat`到filesystem owner的handoff、post-mutation stat/DAC同步、拒绝errno与persistence由
  [`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`](../../register/open-issues.md#ane-20260814-vfs-metadata-mutation-owner-handoff)
  独立跟踪。本契约不把该缺口写成sysfs能力或永久语义，也不要求单个sysfs建立局部workaround。
- 本契约只证明冻结树；generic dynamic positive-dentry freshness继续由
  [`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)
  独立跟踪。
- `/sys` mountpoint由userspace显式准备和挂载；没有automatic mount contract。
- RV64与LA64 release build、全部enabled KUnit和定向guest acceptance均通过。LA64在完整PASS与orderly shutdown后因既有
  platform缺少成功的power-off handler进入halt；该末尾处置不削弱sysfs evidence，也不属于本契约。
