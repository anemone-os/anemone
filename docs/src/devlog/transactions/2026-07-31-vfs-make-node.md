# 2026-07-31 - VFS Make Node

**Status:** Active / R0 Accepted / Stage 1 Active / `DEVICE-NUMBER-CUTOVER` Not Cut Over
**Opened:** 2026-08-01；canonical path 于 2026-07-31 public promotion 时预留
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260731-vfs-make-node R0](../../rfcs/vfs-make-node/index.md),
[目标与不变量](../../rfcs/vfs-make-node/invariants.md),
[Stage 1 definition](../../rfcs/vfs-make-node/implementation.md#5-stage-1-ready--device-number-prerequisite)
**Canonical Revision:** R0
**Contract Impact:** Preserve `VFS-FILE-KIND-001`、`TTY-ENDPOINT-001`；Stage 1 只在
`DEVICE-NUMBER-CUTOVER` Refine `DEVICE-NUMBER-001`；`VFS-MAKE-NODE-001`、
`VFS-SPECIAL-NODE-RDEV-001` 与 `VFS-MOUNT-ADMISSION-002` 保持 Not Cut Over

## Scope and authorization

用户建立唯一 GOAL：完成 Stage 1，并要求读取 canonical RFC、implementation、tracking issues、register、current
contract 与 transaction，执行前置 gate、frozen write set、review、validation、退出与 write-back；Stage 1
关闭后不得自动进入 `1 -> 2 Implementation Resolution Gate` 或 Stage 2。一个 stage checkpoint 使用一个
`vfs-make-node: ...` commit，最后必须由 subagent 独立 review。

用户随后明确接受本 revision 的 visible no-umask deviation：`mknodat` requested permission bits 不经 process
umask 屏蔽；真正的 umask 以后由独立工作统一覆盖 task/fs-state owner、`umask(2)` 与全部创建类调用点。本授权
不允许 Stage 1 或 Stage 2 顺带读取当前 stub、建立局部 mask 或扩大 task/create source surface。

用户同时授权 R0 acceptance、transaction setup 与 closure 所需的全部 documentation-level write-set expansion。
授权后的 exact public manifest 仍由 canonical
[implementation §5.5](../../rfcs/vfs-make-node/implementation.md#55-resolved-write-set-manifest) 拥有；Stage 1
production source manifest 没有扩大。

## R0 acceptance and Stage 1 activation preflight - 2026-08-01

初次独立只读 review 确认 target 的 device-number、kind/`rdev` single truth、owner split、cutover 与 Stage 2
Outline 自洽，但发现两个 Keter：umask visible semantics 未决，以及 Stage 1 documentation manifest 无法覆盖
R0/transaction/closure。开发者的上述决定分别关闭这两个 acceptance blocker。

Draft repair 把 no-umask semantics 折回 RFC 摘要、目标/非目标、ABI/接受边界、target invariant、Stage 2 protected
boundary/stop condition 与 tracking issue；Linux claim 明确不包含 umask-adjusted permission，并登记具有统一
owner、全部 call-site cutover 与跨创建路径 proof 退出条件的
[current limitation](../../register/current-limitations.md#ane-20260801-vfs-make-node-no-umask)。Stage 1 public
manifest补齐 `SUMMARY.md`、`rfcs.md`、tracking、register、device owner index 与 open-issue backlink。

第二轮独立只读 R0 review 对修订后全文、invariants、implementation 与 tracking 复审，结论为 Apollyon 0、
Keter 0、Euclid 0、Safe 0，R0 可接受。review 同时确认 Stage 1 source manifest 未扩大、Stage 2 仍是具备目的、
依赖、受保护边界和独立解析触发点的 Outline。`git diff --check` 与 `mdbook build docs` 均通过；kernel build、
KUnit、QEMU、LTP在该文档 review 中均 Not Run。

Activation preflight 的 repository baseline 为 `/home/doruche/dev/anemone-dev/worktrees/alpha`、
`dev/drc/alpha@1bf9515c`；进入时 worktree clean，R0 repair 后 dirty set 只包含 canonical RFC 四页和本次获准的
acceptance/transaction/navigation write-back，production source 无 diff。preflight 直接读取 worktree-local root
`kconfig`；相关事实为 `kunit = true`、`fs_ext4 = true`、`max_logical_cpus = 1`，且该文件与
`conf/.defconfig` 不同，未用 defconfig 替代当前配置。

live source 仍是 `MAJOR_BITS = 16`、`MINOR_BITS = 16`、
`DeviceId::{None, Char, Block, Raw}`；Linux 12/20 packing 仍在 `fs/inode.rs`，loop ioctl 仍读取行为性 raw value。
第 5.5 节十二个 production/ABI path 全部存在且无 source diff。register 中
`ANE-20260527-LTP-MKNOD-LEGACY-READDIR` 保持 Open；named FIFO I/O 与 legacy `readdir` 不属于 Stage 1 或本 RFC
closure。

RV64 runtime master 明确绑定为 caller-selected `etc/preliminary/images/sdcard-rv.img`，解析到只读共享资源
`/home/doruche/dev/anemone-dev/shared/preliminary/images/sdcard-rv.img`；检查时是 4 GiB ordinary file、mode
`0644`。end-to-end wrapper 必须复制为 worktree-local runtime image，不得让 QEMU 直接写 master。

R0 因独立 review 接受为 `Accepted for Implementation`。本轮 Stage 1 唯一 GOAL、transaction preflight、live
baseline 与 frozen manifest 均满足，因此 Stage 1 从 Ready 激活为 Active。activation-time contract cutover 为
None；`DEVICE-NUMBER-001` 继续保持 effective 16/16，Stage 2 与 `1 -> 2` gate 未授权。

## Execution log

### 2026-08-01 - Stage 1 activated

**Write-set lock:** production/ABI/KUnit 只允许 canonical §5.5 的十二个 source path；public write-back 只允许同节
列出的 current contract、RFC、transaction、navigation、devlog 与 register surfaces。formatter 对目标 Rust
文件的正常改写可接受；任何其它 source 路径都触发停止/扩展报告。

**Protected boundary:** char/block registry namespace、static endpoint number/name、devfs/TTY publication、provider
lifecycle/lookup、block I/O 与 mount provider-miss semantics 保持；不新增 `mknodat`、filesystem special node、
`ENOTBLK` refine、device-open resolver或 raw escape。

**Validation floor:** `just fmt kernel --check`、`git diff --check`、RV64/LA64 release preset build、绑定上述 master
的 RV64 end-to-end wrapper、source audit、独立 final review 与 `mdbook build docs`。LA64 runtime 与 make-node
syscall proof明确属于 Stage 2，保持 Not Run。
