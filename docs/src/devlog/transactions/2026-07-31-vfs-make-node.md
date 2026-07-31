# 2026-07-31 - VFS Make Node

**Status:** Active / R0 Accepted / Stage 1 Closed / `DEVICE-NUMBER-CUTOVER` Effective / Stage 2 Outline
**Opened:** 2026-08-01；canonical path 于 2026-07-31 public promotion 时预留
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260731-vfs-make-node R0](../../rfcs/vfs-make-node/index.md),
[目标与不变量](../../rfcs/vfs-make-node/invariants.md),
[Stage 1 definition](../../rfcs/vfs-make-node/implementation.md#5-stage-1-closed--device-number-prerequisite)
**Canonical Revision:** R0
**Contract Impact:** Preserve `VFS-FILE-KIND-001`、`TTY-ENDPOINT-001`；Stage 1已在
`DEVICE-NUMBER-CUTOVER` Refine `DEVICE-NUMBER-001`为Effective；`VFS-MAKE-NODE-001`、
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

### 2026-08-01 - Stage 1 closure and `DEVICE-NUMBER-CUTOVER`

**Implementation:** `anemone_abi::fs::linux::dev_t`成为唯一Linux 12/20 packed codec owner，精确API为
`encode(u32, u32) -> u32`与`decode(u32)`，codec位宽常量保持private。`device::devnum`独立拥有common numeric
domain的public bounds；`DeviceNumber`拥有category-neutral
major/minor，`CharDevNum` / `BlockDevNum`只在typed registry boundary包装；`DeviceId`收窄为`None | Number`，
删除Char/Block tag、Raw与行为性raw escape。stat和loop调用canonical codec，statx直接投影结构化parts。

char/block FileOps与mount均先检查immutable inode kind，再从numeric `rdev`构造typed key；mount继续对non-block
返回`EINVAL`，provider miss保持`ENOENT`，没有提前实施Stage 2 `ENOTBLK` Refine。devfs、console、TTY与ext4只把
既有typed endpoint number投影为common number；static major/minor、name、registry、publication/provider
lifecycle与block I/O均未改变。最终source diff恰好覆盖第5.5节十二个冻结路径，没有source expansion。

**Validation:** `just fmt kernel --check`与`git diff --check`通过。RV64/LA64 canonical release preset build在最终
tree通过；sandbox内相同build曾因lwext4 C编译触发`Bad system call`/SIGSYS，完全相同命令在sandbox外成功，因此
分类为environmental。RV64 wrapper绑定preliminary 4 GiB只读master并使用worktree-local运行副本，near-final
tree完成282/282 KUnit、glibc/musl focused LTP共4/4 case与orderly PowerOff。新增device-number domain、typed
conversion、codec boundary、stat/statx与loop codec KUnit均`ok`，既有devfs char/block与TTY mapping KUnit通过。

runtime之后只发生三项局部可预测收紧：codec签名从u64改为冻结的u32 API、mount kind检查移到`get_attr()`前，
以及删除多余的console单值KUnit；production console projection与device domain bounds没有再改。开发者明确判断
无需因此重跑QEMU；最终双架构build、format、whitespace与source audit覆盖这些变化。LA64 runtime、真实
`mknodat`、filesystem node matrix与Stage 2用户态proof均Not Run。

**Review:** 最终独立只读review以最新tree核对canonical RFC/current contract target、全部十二个source diff和
validation provenance，结论Apollyon 0、Keter 0、Euclid 0、Safe 0。review确认codec唯一owner、category single
truth、typed namespace、mount order/errno、loop/stat projection、allocator bounds及existing number/name/
publication/provider行为均满足Stage 1，且无write-set或Stage 2越界。

**Cutover / stop:** `DEVICE-NUMBER-CUTOVER`原子把`DEVICE-NUMBER-001`从历史16/16 baseline Refine为effective
12/20 category-neutral current contract；Stage 1 Closed。RFC仍是R0 Accepted for Implementation，transaction
保持Active只因完整RFC尚有后续stage。Stage 2继续Outline / Not Active，`VFS-MAKE-NODE-CUTOVER`与其三个contract
delta保持Not Cut Over；本轮未运行、未解析、未授权`1 -> 2 Implementation Resolution Gate`，并在此停止。
