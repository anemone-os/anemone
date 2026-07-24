# System Target Model R3 Explicit Inputs

**Status:** Completed
**Date:** 2026-07-24
**Owner:** doruche
**Canonical Revision:** [RFC-20260722-system-target-model R3](../../rfcs/system-target-model/index.md)
**Implementation Authority:** [R3 explicit-input cleanup](../../rfcs/system-target-model/implementation.md#r3-explicit-input-cleanup)
**Previous Transaction:** [R0-R2 implementation history](./2026-07-22-system-target-model.md)
**Contract Impact:** None；[`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md) Preserve

## Scope and authorization

用户反馈R2的local selection -> tracked default preset属于不需要的隐式能力，并明确偏好显式输入；
classic KernelConfig默认继承是唯一保留的配置继承机制。审计发现rootfs type、QEMU CPU和fmt scope存在
同类省略驱动行为。用户接受R3 target并授权实施，随后进一步决定folder容量不需要配置，全部使用
automatic sizing；QEMU BIOS保持optional，因为部分target没有合适BIOS，省略时不传`-bios`。

本事务只有一个atomic checkpoint `R3A explicit-input cleanup`，最终只形成一个commit。R0-R2 transaction
已经Completed，不续写或重开。Final harness、kernel/runtime ABI、Boot Protocol、DT authority/delivery、
QEMU bind语义与physical deployment不在本事务范围。

## R3A activation preflight

**Status:** Closed

Preflight读取live Justfile/help、xtask config/task owner、全部tracked preset/Platform/rootfs manifest、schema、
build skill、R2 RFC/transaction、register/current limitations与`BOOT-PROTOCOL-001`。确认：

- implicit selection仅由`selection` CLI、两份selection file/schema、ignore rule和resolver fallback组成，
  可以删除而不改变BuildPreset或完整low-level tuple；
- KernelConfig的`.defconfig`继承是classic Kconfig行为，保留；
- folder rootfs只需显式`type = "folder"`，容量统一交给`virt-make-fs`自动计算，删除size surface；
- live QEMU 10.0.50提供`la464`，两份LA64 Platform固定该CPU；CPU总是进入normal/DT argv；
- BIOS omission具有不同于CPU default的真实语义，保持`Option`；
- fmt收口为`all | kernel | app`，`all`覆盖standalone xtask，避免显式all仍遗漏build-system源码。

Resolved write set、验证floor与停止条件由canonical implementation R3A段冻结。若LA64显式CPU造成DT drift
或build/runtime不兼容，或必须修改read-only owner，先停止并记录具体影响；不得通过fallback恢复隐式行为。

## Execution log

首次`just fmt all --check`暴露既有generated-source格式缺陷：`kconfig_defs.rs`与`platform_defs.rs`由生成器
稳定写入行尾/末尾空白，导致all scope在检查standalone xtask前失败。Canonical R3A manifest已先更新，
将owner-local `scripts/xtask/src/config/kconfig.rs`加入write set；`platform.rs`原已在内。修复仅清理生成
字符串空白，并由后续显式build重新生成；不手改generated文件作为长期方案。

`just fmt all`在generator修复后首次实际覆盖standalone xtask，并机械格式化少量既存xtask/app drift。
Canonical implementation已登记确切文件；这些变更依仓库formatter policy允许越过原write set，full diff
review仍须证明只有rustfmt形状变化。

## R3A closure

Production/config cleanup、schema/manifests、help/docs/build skill和公共导航已经原子同步。BuildPreset与
classic KernelConfig默认继承保持；自然空collection和optional capability仍只表达absence，不选择替代
policy。Rootfs folder容量固定自动计算，QEMU BIOS omission固定为不发`-bios`。

**Validation:** 最终字节`just xtask-test`为64 passed / 0 failed；bare/partial/mixed system input与
bare/unknown fmt均在side effect前拒绝；显式preset/tuple resolve与全部tracked preset通过。两架构release
preset在沙箱外串行build成功；沙箱内RV64只因lwext4 C compile seccomp SIGSYS失败。一次并发双架构run因
共享generated state作废且未计入证据，随后RV64/LA64均串行重跑成功。四份QEMU DT check通过，LA64
`-cpu la464`且无`-bios`，RV64保留显式BIOS。Rootfs type/size rejection、三份schema、live help、fmt all/
kernel/user-test、wrapper syntax、residual/source、whitespace与mdBook检查通过；mdBook只有既有large
search-index warning。

**Review:** Full diff确认formatter触及的xtask/app文件只有rustfmt机械变化；latest-byte review为
Apollyon 0 / Keter 0 / Euclid 0。Current contract、register/current limitations、R0-R2 transaction、
kernel/runtime behavior与wrapper源码均未修改。

**Boundary:** 真实QEMU guest runtime、rootfs materialization、physical board、LTP与final harness Not Run。
Contract cutover为None，`BOOT-PROTOCOL-001` Preserve。R3A与transaction Completed；最终交付一个commit。
