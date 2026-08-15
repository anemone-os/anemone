# 2026-08-14 - Static sysfs

**Status:** Completed / R1 Closed / `STATIC-SYSFS-CUTOVER` Effective
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260814-static-sysfs R1](../../rfcs/static-sysfs/index.md)
**Canonical Revision:** R1
**Contract Impact:** Introduce [`SYSFS-STATIC-001` / `SYSFS-MOUNT-001`](../../contracts/sysfs/static-filesystem.md)

## Scope and boundary

唯一原子gate交付可由userspace挂载的static `sysfs`、persistent singleton tree、`/kernel/address_bits`与
`/kernel/cpu_byteorder`两个真实consumer，以及双架构production-path acceptance。本次不进入dynamic sysfs、kobject/kset、
device model、hotplug、automatic mount、symlink、binary/writable attribute、其它consumer、generic VFS协议或public test API。
实现、validation、current-contract cutover与RFC closure共同关闭`STATIC-SYSFS-CUTOVER`，不存在后续gate。

## Implementation and source audit

- `fs::sysfs`注册canonical no-device `sysfs`并使用`PERSISTENT_SB`；init先验证完整descriptor，再以registration返回的唯一
  `FileSystem` identity构造、seed并安装singleton。boot-fatal failure不能把部分tree带到userspace。
- static entry只保存name、kind、initial mode、inode identity和窄getter。architecture owner提供crate-private
  `address_bits()`与`cpu_byteorder()`；read、pread、seek和stat size snapshot均即时调用getter，没有consumer-value cache。
- root、`kernel/`、两个attribute的initial mode分别为`0555`/`0444`；静态lookup/readdir、普通/positioned read、seek/EOF和
  fail-closed structural mutation/truncate/data write直接由sysfs owner实现，没有复用procfs PDE，也没有抽取generic
  pseudo-fs abstraction。
- VFS audit确认`PERSISTENT_SB`使ordinary unmount和inode shrinker不驱逐backing inode；`InodeRef` drop只减少active refcount，
  不撤销indexed inode。因此multi-view、last-unmount与remount使用同一process-lifetime tree，无新增cleanup owner或镜像状态。
- 定向`static-sysfs-test`是现有syscall/TTY ABI的真实userspace consumer；两个rootfs manifest分别绑定RV64/LA64，没有production
  probe facade。

## R1 target decision - metadata mutation owner handoff

独立review发现generic `fchmod`/`fchown`/`utimensat`直接改写`InodeMeta`，而static sysfs descriptor仍投影initial mode；source
comparison确认static procfs PDE自2026-06-14已有同型descriptor/meta分裂，devfs/devpts/ramfs则以inode metadata作为可变projection
真相。Linux 6.6.32的generic procfs与kernfs也通过filesystem-owned setattr同步owner/inode，而Anemone当前没有该handoff。

维护者决定该系统问题不属于static-sysfs RFC，也不阻塞当前工程。R1因此把`0555`/`0444`限定为未发生metadata mutation时的
initial projection，不承诺post-mutation stat/DAC同步、拒绝errno或persistence；问题登记为
[`ANE-20260814-VFS-METADATA-MUTATION-OWNER-HANDOFF`](../../register/open-issues.md#ane-20260814-vfs-metadata-mutation-owner-handoff)，
由后续独立VFS工作统一解决。本实现未新增VFS hook、filesystem-name特判、sysfs-local workaround或metadata mutation test，
也未把当前缺口伪装成sysfs capability。

## Validation and diagnostic disposition

- `just fmt kernel --check`、`just fmt static-sysfs-test --check`、`git diff --check`通过。
- `just build --preset qemu-virt-rv64-release`与`just build --preset qemu-virt-la64-release`通过；两个validation app与两个
  acceptance rootfs均通过repository-native build。
- RV64 `build/static-sysfs-rv64.log`与LA64 `build/static-sysfs-la64.log`均报告`All tests passed!`，三项sysfs owner-local
  KUnit均通过；随后定向consumer验证精确tree/initial mode/content、partial read、pread、seek/reset、EOF/beyond-EOF、structural
  mutation/truncate/data-write failure、non-empty mount-data `EINVAL`、`nodev\tsysfs` projection、two-view/surviving-view与
  last-unmount/remount，最终均输出`STATIC-SYSFS:PASS`。
- 调试期间remount lookup、两个attribute read和tree lifetime均已由source与临时诊断证实；缺失的末尾输出来自关机在TTY队列
  drain前停止UART。验收程序在shutdown前使用现有`tcsetattr(..., TCSADRAIN, ...)`等待stdout drain后，RV64/LA64均稳定保留
  完整PASS。临时sysfs/fd诊断已删除；drain只保护验收oracle，不改变TTY或sysfs production contract。
- RV64在PASS后完成platform power-off。LA64在PASS和完整orderly filesystem/network/device shutdown后报告
  `no power off handler succeeded, halting the system`；随后仅终止已halt的QEMU。该既有platform末尾行为不作为sysfs失败。

## Review, cutover and closeout

首轮独立subagent review除metadata mutation owner finding外未发现其它问题；该finding形成上述R1维护者决定和register routing。
R1 final-candidate独立subagent review覆盖target/owner/public API边界、init publication、inode/view lifetime、read/pread/seek、
consumer truth、fail-closed ABI、TTY drain oracle和文档cutover，一致结论为无阻塞finding。主代理最终自查发现guest尚未直接发起
truncate，遂为现有acceptance consumer补充真实`ftruncate`失败oracle，并重建rootfs、按RV64 build/runtime再LA64
build/runtime的串行顺序重跑；两架构仍通过全部enabled KUnit并输出完整`STATIC-SYSFS:PASS`。

Architecture Friction Scan未发现本target内的第二份consumer状态、owner穿透、public API扩张、fstype/architecture/test特判、
无退出条件临时桥、隐含cleanup/failure顺序或降低oracle诚实性的处置。descriptor与`InodeMeta`在generic metadata mutation后的
分裂是唯一残余friction；其历史早于sysfs、影响多个pseudo filesystem，已经维护者明确接受为R1 target之外的系统问题并由
register记录具体owner偏差、影响和最小修正方向，当前实现没有为它扩大generic协议/owner surface或建立局部掩盖。因此本
cutover无Keter/Apollyon，且没有需要在本RFC继续修复的Euclid。

`STATIC-SYSFS-CUTOVER`原子使`SYSFS-STATIC-001`与`SYSFS-MOUNT-001`成为Active current contract，并关闭R1 RFC。现有
`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`与`ANE-20260604-IOCTL-LTP-STAGE1-GAPS`保持原状态；新增metadata
mutation owner issue保持Open / Deferred。dynamic sysfs、kobject/kset、device model、hotplug、automatic mount、其它consumer
与generic VFS扩展全部Not Cut Over；没有后续gate被激活。
