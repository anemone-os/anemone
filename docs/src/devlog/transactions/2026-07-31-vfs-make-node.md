# 2026-07-31 - VFS Make Node

**Status:** Completed / R2 Closed / Stage 1-2 Closed / C1-C3 Closed / Both Cutovers Effective
**Opened:** 2026-08-01；canonical path 于 2026-07-31 public promotion 时预留
**Owner:** doruche, Codex
**Canonical Plan:** [RFC-20260731-vfs-make-node R2](../../rfcs/vfs-make-node/index.md),
[目标与不变量](../../rfcs/vfs-make-node/invariants.md),
[Stage 2 plan and closure](../../rfcs/vfs-make-node/implementation.md#7-stage-2-closed--make-node-vertical-slice)
**Canonical Revision:** R2
**Contract Impact:** Preserve `VFS-FILE-KIND-001`、`TTY-ENDPOINT-001`；Stage 1已在
`DEVICE-NUMBER-CUTOVER` Refine `DEVICE-NUMBER-001`为Effective；Stage 2已在`VFS-MAKE-NODE-CUTOVER`
Introduce `VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`并Refine `VFS-MOUNT-ADMISSION-002`为Effective

> 本transaction只记录R0-R2 branch-local执行事实。外层合流后R3复用既有task filesystem-context umask
> owner；该修订不改写本页当时接受、验证和cutover的no-umask历史。

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
[当时的no-umask limitation；合流后由umask小迭代关闭](../changes/2026-07-27-umask-file-creation-mask.md)。Stage 1 public
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

## Stage 1 -> Stage 2 Implementation Resolution Gate - 2026-08-01

**Authorization / baseline:** Stage 1独立关闭后，开发者另行授权只解析Stage 2并落文档；进入时baseline为clean
`dev/drc/alpha@c72721dd`。开发者要求把原计划中分开的syscall/VFS与ext4/ramfs/mount feature checkpoint合并，
最终只保留C1 behavior-preserving module split、C2 merged feature implementation、C3 validation/probe removal/
review/cutover。Ready不构成activation，任一checkpoint关闭不自动进入下一checkpoint。

**R1 decision:** live common-create在backend commit之后仍有inode-cache/dentry materialization窗口。开发者明确：
若完整原子化做不到，或需要不小的架构变动，则作为既有问题记录，本RFC不负责解决。该决定改变acceptance boundary，
因此RFC升为R1：ext4/ramfs仍必须backend-local先写final mode/uid/gid/适用`rdev`、再commit dirent并回滚commit前
allocation/resource；existing common-create handoff不得因make-node退化。需要新跨owner transaction protocol的
既有窗口登记为`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`，不阻塞R1。

**Live resolution evidence:** `fs/mod.rs`为1271行、`fs/inode.rs`为1061行，C1固定同owner目录化拆分并保持全部
re-export/visibility/signature/behavior。RV64/LA64 syscall table均缺`mknodat(33)`；`Capability::MKNOD`与
`RawAtFd`/`AtFd`规则可只读复用。resolution当时把`InodeOps` static误记为32个；C2 live sweep更正为feature前
34个、仍分布于同一组26个文件，路径manifest没有变化。
common create当前在backend return后补owner，故C2固定VFS callback前形成`MakeNodeDescription`，regular mknodat
也走backend `make_node`。

lwext4 live create在`add_entry`后才set mode，owner high bits与rdev reload尚未完整接线；C2要求backend-local
pre-dirent final metadata、free-unlinked-inode rollback、u32 uid/gid、rdev persistence/reload与explicit special
open error。ramfs在现有write transaction内扩展五类node并以private category-neutral rdev保持identity。mount在
registry lookup前区分`ENOTBLK`，合法Block provider miss保持`ENOENT`。durable `anemone-rs` mknodat wrapper作为
真实ABI consumer保留；focused LTP group与existing user-test临时probe在C3验证后删除/恢复。

**Validation plan / disposition:** authoritative命令、LTP case分类、temporary probe coverage、双架构runtime、
source audit、probe exit与single cutover见
[Stage 2 plan](../../rfcs/vfs-make-node/implementation.md#7-stage-2-closed--make-node-vertical-slice)。本resolution
只运行`git diff --check`、`mdbook build docs`与状态一致性扫描；kernel format/build、KUnit、QEMU、LTP、RV64/
LA64 runtime、ext4/ramfs behavior与hardware均Not Run。current contracts未修改，`VFS-MAKE-NODE-CUTOVER`及三个
remaining delta保持Not Cut Over。

**Result / stop:** R1 Accepted for Implementation；Stage 2 Ready / Not Active。C1、C2、C3均未激活，当前停止。

## Stage 2 execution log

### 2026-08-01 - Checkpoint C1 activation and closure

**Authorization / preflight:** 开发者建立新的唯一 GOAL，明确授权依次完成 Stage 2 C1、C2，并要求每个
checkpoint 独立 commit、前一 checkpoint 关闭后不自动进入下一 gate，最终由 subagent review。C1 从 clean
`dev/drc/alpha@a09d7d66` 激活；进入时 canonical R1、implementation、tracking issues、register、current
transaction 与 live source 已重新读取，`kconfig` 和 `conf/.defconfig` 均保持 `kunit = true`、
`fs_ext4 = true`、`max_logical_cpus = 1`。C1 write-set 锁定
[§7.2](../../rfcs/vfs-make-node/implementation.md#72-checkpoint-c1--behavior-preserving-vfsinode-module-split)
列出的 `fs/mod.rs`、`fs/vfs/` 与旧/新 `fs/inode*`；activation-time contract cutover 为 None。

**Implementation / diff:** `fs/mod.rs` 只保留 module declaration、原稳定 re-export 与 filesystem-driver
init；global VFS singleton/mount/filesystem registry 移入 `fs/vfs/mod.rs`，`PathResolution`、全部 VFS
operation 与原 inline KUnit 移入 `fs/vfs/ops.rs`。`fs/inode.rs` 按冻结角色拆为
`inode/{mod.rs,ops.rs,metadata.rs,object.rs}`；原先相对 `fs` 生效的 `pub(super)` 在多一层 module 后机械写为
`pub(in crate::fs)`，有效 visibility 不变。未新增 function pointer、request type、helper abstraction 或
production behavior；source dirty set 恰好是 C1 manifest。移动规模为删除两个旧 flat 文件共 2252 行，并在
七个新/保留文件中一一重放相同职责；该规模只来自目录化移动。

**Audit / review:** old/new public type、impl、KUnit 名称集合及 `crate::fs::*` /
`crate::fs::inode::*` re-export sweep 一致；全部 16 个原 VFS KUnit 留在 `vfs/ops.rs`，2 个 stat KUnit 留在
`inode/metadata.rs`。review 确认状态 owner、调用方向、函数签名、public surface 与 shared contract 均未改变，
没有相邻重构或 C2 feature wiring；Apollyon 0、Keter 0、Euclid 0、Safe 0。

**Validation:** `just fmt kernel --check` 与 `git diff --check` 通过；RV64、LA64 canonical release preset
build 均通过。首次 sandbox 内 RV64 build 在 lwext4 C 编译触发既知 `Bad system call` / SIGSYS；完全相同命令
在 sandbox 外进入 Rust 编译并暴露、修正 module-depth visibility 后通过，故该首次失败分类为环境限制而非
kernel defect。C1 按合同未运行 QEMU/LTP；make-node syscall/backend/runtime proof 均 Not Run。

**Closure / stop:** C1 Closed；`VFS-MAKE-NODE-CUTOVER` 继续 Not Cut Over，current contract 未修改。
C2 保持 Not Active，C1 closure 本身不构成 C2 activation。

### 2026-08-01 - Checkpoint C2 activation, Review Hold and closure

**Authorization / baseline:** 同一开发者GOAL在C1独立关闭后明确授权继续完成C2，但不授权C3。C2从clean
`dev/drc/alpha@1115be0d`激活；重新核对R1 target、§7.3路线、§7.5停止条件与§7.6 production/temporary/docs
manifest后，没有发现需改变owner、ABI、node matrix、errno、acceptance或shared contract的证据。activation-time
contract cutover为None；C3 runtime、probe removal、final contract review/cutover继续未授权。

**Implementation:** RV64/LA64 syscall registry增加asm-generic `mknodat(33)`，adapter以`RawAtFd`保留absolute
path忽略invalid dirfd，完成`0/REG/FIFO/CHR/BLK/SOCK`、DIR=`EPERM`、LNK/invalid=`EINVAL`、CHR/BLK
`CAP_MKNOD`（包括char 0:0）与canonical Linux `dev_t` decode。VFS在backend callback前完成writable mount、DAC、
requested permission及既有uid/gid inheritance并形成窄`MakeNodeDescription`；regular也走同一`make_node` route，
未读取umask stub或修改其它create caller。

ext4/lwext4在`add_entry`前写final kind/mode、完整u32 uid/gid与适用rdev；dirent失败释放未链接inode，释放成功后
清除dirty以避免`Drop`把已释放slot写回。reload/getattr/sync复用同一owner与canonical rdev codec；FIFO open为
`EOPNOTSUPP`，Char/Block/Socket为`ENXIO`。ramfs在既有write transaction中先完成final metadata，再seed inode并
插入dirent；duplicate在allocation/publication前拒绝，post-allocation路径没有新增fallible external resource。
Regular/Fifo/Char/Block/Socket均有稳定resident identity与explicit open error。mount先检查Block kind与numeric
rdev，non-block、null source和missing identity为`ENOTBLK`，合法number的provider miss仍为`ENOENT`。

durable `anemone-rs`保留raw syscall、typed `AtFd`/`Path` wrapper与unsafe pointer form。validation-only manifest已
准备并保留temporary `vfs_make_node_probe`、`vfs-make-node` focused LTP group及profile/registration；probe覆盖
ext4/ramfs node matrix、metadata/stat/statx/getdents/unlink、regular I/O、special open、relative/absolute dirfd、
bad pointer、duplicate、RO mount、CAP_MKNOD、char 0:0、provider independence与ext4 overlong-name precommit
admission/no-dirent路径。它们只编译、未执行，必须由C3运行后删除/恢复；未形成production dispatch或长期API。

**Initializer / source audit:** resolution的32-static计数发生live drift：C2 activation前实际为34个；C2在已授权
`fs/ramfs/inode.rs`内增加一个special-node vtable，最终35个static全部恰有一个`make_node`。只有ext4与ramfs
directory使用真实callback，其余33个均使用共同`EPERM`拒绝函数；26文件manifest与owner集合不变。ext4/ramfs
special open无`unimplemented!()`或错误的ordinary fallback；make-node production route没有provider lookup；
Linux packed device codec仍只由`anemone_abi::fs::linux::dev_t`拥有，ext4 helper只调用该codec。mount顺序保持
kind -> rdev -> provider，C2 source/validation dirty paths全部位于§7.6 manifest。

**KUnit / review:** owner-local KUnit只完成编译，覆盖mode/dev/capability normalization、完整kind matrix与char 0:0、
`MakeNodeDescription`合法性、ext4 canonical rdev projection与special-open errno、ramfs final metadata/duplicate
admission/rdev projection/special-open、mount Block/rdev admission；未运行，不能作为rollback/reload runtime proof。
pre-closure review先修正ext4 special vtable绕过`ext4_open`、missing-rdev mount仍为`EINVAL`、free-unlinked后dirty
inode可能写回三个finding。最终独立subagent review随后报告一个Apollyon、三个Keter和一个Euclid：R1 strict
backend atomicity超出lwext4自然能力；overlong-name probe只能证明admission/no-dirent；mode高于Linux `umode_t`
位宽应截断而非`EINVAL`；文档提前声称C2 Closed/committed/no findings；ext4 socket注释已过时。C2因此进入
Review Hold，没有checkpoint commit。

**Validation:** `just fmt kernel --check`、`just fmt user-test --check`与`git diff --check`通过。最新production tree的
RV64、LA64 canonical release preset build均通过并编译KUnit；首次sandbox内RV64仍在lwext4 C编译触发既知
`Bad system call`/SIGSYS，相同命令在sandbox外通过，分类为environmental。temporary user-test分别通过
`just app build --arch riscv64 user-test`与`just app build --arch loongarch64 user-test`。C2按合同未运行QEMU、
KUnit runtime、LTP或probe；ext4/ramfs真实runtime、reload、failure-path与双架构acceptance均Not Run，留给C3。

**Hold / stop:** C2未关闭、未提交。temporary probe/profile仍在树中，current contract未修改，
`VFS-MAKE-NODE-CUTOVER`及三个remaining delta全部Not Cut Over；C3保持Not Active。target/acceptance finding
触发下述R2 Target Renegotiation Gate；不得运行C3、删除probe、恢复profile或声称partial/runtime capability。

## R2 Target Renegotiation and C2 Review Hold - 2026-08-01

**Evidence:** ext4 make-node已经在同一`fs_lock`内先形成final mode/uid/gid/rdev，再调用lwext4 `add_entry`；local
child reference也在释放锁前Drop，因此正常并发lookup不能观察中间状态。成功路径的owner/rdev persistence与reload
route同样存在。但lwext4以dirty inode reference、directory block mutation和lazy block-cache writeback组合create，
Rust wrapper没有跨child inode与parent dirent的journal/rollback handle，无法在合理工程量内承诺任意内部I/O
failure、crash或power-loss下物理全有或全无。R1 strict backend atomicity因此不是当前实现能够诚实关闭的claim。

**Developer decision / R2:** 开发者接受reduced但自洽的R2 target：保留final metadata先于publication、同一backend
锁下的normal-runtime serialization、成功normal sync/reload、pre-publication rejection、未链接inodecleanup、
no-panic与existing common-create无退化；明确不保证lwext4任意I/O failure/crash atomicity。修复责任归属后续
lwext4及`lwext4-rust`改进事务，不要求本RFC临时改造lwext4。未来若其它target guarantee仍无法在合理工程量内
承诺，可以再次提出Target Renegotiation，但实现/agent无权静默妥协或把较弱能力写成原target closure。

**Natural-shape audit:** C2 diff没有forced flush、双阶段truth、fault-injection production hook、伪journal或第二套
publication protocol。metadata-before-publication、同锁调用、name边界validation、`free_unlinked` cleanup、u32
owner和rdev persistence都是自然正确性/错误抵抗结构，保留。overlong-name probe只声明admission/no-dirent，不能
作为allocation rollback proof；strict lwext4 failure/crash gap登记为
[accepted limitation](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)。

**Other review dispositions:** syscall normalizer按Linux 16-bit `umode_t`先截断mode高位，再做kind/permission解析；
相关KUnit改为验证高位被忽略。ext4 socket stale comment删除。R2 RFC、invariants、implementation、tracking、
register、transaction、RFC index与双周devlog同步改写；在最终format/build/docs与同一subagent复审通过前，C2继续
Review Hold且没有commit。QEMU、KUnit runtime、LTP、probe、真实ext4/ramfs runtime/reload/failure path均Not Run。

**Final review:** 同一只读subagent对R2 canonical target、accepted limitation、全部production/temporary/docs diff
与validation provenance复审，结论Apollyon 0、Keter 0、Safe 0；唯一Euclid指出temporary probe注释仍把256-byte
component误称为backend rejection、RFC用户态proof仍泛称backend rollback。两处均收窄为namei
pre-publication rejection与unlinked-inode cleanup后关闭。review确认35个`InodeOps` static位于冻结26文件，只有
ext4/ramfs directory使用真实callback；无forced flush、伪journal、双状态、fault hook、proof-only production seam、
provider lookup、raw codec重复或write-set扩张。R2 owner/exit condition与未来Target Renegotiation边界自洽。

**Final validation:** 最新tree通过`just fmt kernel --check`、`just fmt user-test --check`、`git diff --check`、
`mdbook build docs`、RV64/LA64 canonical release preset build和双架构`just app build ... user-test`。三个new file
分别执行`git diff --no-index --check /dev/null <file>`，均只以“与空文件不同”的status 1结束且无whitespace
diagnostic。RV64 sandbox build在lwext4 C编译重现`Bad system call`/SIGSYS，相同命令在sandbox外通过，继续分类为
environmental。一次并行双架构build因共享`build/generated/device-tree/platform.dtb`竞争导致LA64缺文件；RV64完成
后顺序重跑同一LA64 canonical命令通过，故分类为验证编排竞争而非source defect。KUnit只随release build编译；
QEMU、KUnit runtime、LTP、temporary probe、真实ext4/ramfs runtime/reload/failure path均Not Run。

**Closure / stop:** C2以独立`vfs-make-node: implement node creation`checkpoint commit关闭。temporary probe、focused
profile与group按计划保留给C3；current contract未修改，`VFS-MAKE-NODE-CUTOVER`及三个remaining delta全部Not Cut
Over。Stage 2保持Active，C3保持Not Active；本GOAL在此停止，不自动运行C3、删除probe、恢复profile、修改current
contract或声称runtime/partial capability。

### 2026-08-01 - Checkpoint C3 activation and validation preflight

**Authorization / baseline:** 开发者建立新的唯一GOAL，独立授权完成Stage 2 C3，并重申只执行C3的前置条件、
frozen write set、review、validation、退出与write-back，不自动进入任何后续gate。C3从clean
`dev/drc/alpha@132c10d9`激活；进入时重新读取AGENTS/LOCAL、R2 canonical RFC、implementation、tracking、
register、current contracts与本transaction。`kconfig`和`conf/.defconfig`均保持`kunit = true`、
`fs_ext4 = true`、`max_logical_cpus = 1`；RV64/LA64 preliminary master均为4 GiB ordinary file且wrapper只复制到
worktree-local runtime path。activation-time contract cutover为None，三个remaining delta继续Not Cut Over。

**Validation preflight:** temporary focused group、profile、registration与probe均仍位于§7.6 validation-only
manifest；durable`anemone-rs`wrapper与C2 production tree保持clean baseline。preflight发现probe能验证ext4即时
metadata却尚未制造eviction/remount reload；C3在已冻结的`main.rs`与temporary probe内补充chroot前真实
`/dev/vdb` mount -> create -> normal unmount/sync/evict -> remount -> metadata/data复验 -> cleanup路线。该修复不增加
production hook、public API、owner或contract surface，probe仍须在runtime取证后完整删除。

**Write-set lock / stop:** C3只允许§7.6 production、validation-only与documentation/cutover manifest；当前预计
只写temporary probe/main、随后删除/恢复全部validation-only path，并在成功时原子更新三个VFS contract delta。
任一in-target失败、双架构runtime缺失、probe无法删除、final review finding或停止条件命中时保持全部Not Cut Over。

**Runtime finding / approved expansion:** 首次RV64 runtime的291项KUnit全部PASS，随后pre-chroot reload probe在
privileged character-node create收到`EPERM`，因此LTP尚未开始。live credential source确认
`Capability::MKNOD`自credentials初始实现以来一直标记`[NYI]`且未进入`IMPLEMENTED`；root的
permitted/effective/bounding均由该集合初始化，所以C2正确的effective-capability gate暴露了既有缺口。开发者明确
批准将`task/credentials/cap.rs`加入C3 manifest，在原credential owner内启用`MKNOD`并验证root projection、
non-root drop及既有capability transition；不改变R2 target、owner、public API或shared contract。修复及双架构
runtime闭合前，三个remaining delta继续Not Cut Over。

### 2026-08-01 - Checkpoint C3 runtime closure and `VFS-MAKE-NODE-CUTOVER`

**Natural bug fixes / write set:** 启用`CAP_MKNOD`后，RV64 focused runtime继续暴露两个保持R2 target的自然缺陷。
lwext4 unlink在child link count归零后无条件调用truncate；其C实现只接受regular/directory/symlink，导致合法FIFO
unlink返回`EINVAL`。wrapper现只对这三类有data语义的inode执行truncate，special inode直接进入既有deletion-time
cleanup。`mknod06`同时证明empty pathname应为`ENOENT`；`mknodat`没有`AT_EMPTY_PATH`，adapter现于capability/backend
admission前完成该分类。两个修复分别位于已冻结的lwext4 wrapper和syscall adapter owner，不改变public API、shared
contract、node matrix或acceptance boundary。开发者进一步明确：此类同owner、保持target/ABI/acceptance的自然bug
修复默认批准扩展；owner transfer、public API/shared contract、visible semantics、acceptance或target变化仍须停止。

**RV64 runtime:** canonical wrapper绑定`etc/preliminary/images/sdcard-rv.img`并只修改worktree-local副本。
最终日志`build/vfs-make-node-stage2-rv64.log`启动293项KUnit并全部通过至用户态；新增
`mknod_is_a_supported_linux_capability`与`root_starts_with_mknod_in_all_limit_sets`均为`ok`。pre-chroot ext4真实
unmount/remount reload probe与chroot后的ext4/ramfs完整probe均PASS。glibc、musl各attempted 12 / passed 11 /
failed 1：`mknod01..09`、`mknodat01..02`全部PASS；唯一`mount02`失败见下述classification。运行到达orderly
filesystem -> network -> device shutdown与PowerOff machine action。

**LA64 runtime:** canonical wrapper绑定`etc/preliminary/images/sdcard-la.img`，得到与RV64相同的293项KUnit、两个
probe PASS以及glibc/musl各11/12 focused PASS。运行到达orderly filesystem -> network -> device shutdown；LA64
没有power-off driver，machine action按预期停在`no power off handler succeeded, halting the system`，随后通过QEMU
monitor手动退出且wrapper返回0。开发者确认该终态是已知平台边界，不是测试缺陷。硬件仍Not Run。

**`mount02` classification:** 赛题LTP case请求`ext2`。既有scoring compatibility bridge在syscall adapter把
`ext2`归一化为no-device `ramfs`，所以character/file/null-source/remount子项没有进入R2的ext4 block-source
admission；glibc/musl及双架构均以相同6 PASS / 6 FAIL结束。修复该alias只为让case变绿会越过本RFC并误改既有
兼容边界，因此不做。temporary probe已在真实ext4 mount path证明regular/FIFO/character/socket source先返回
`ENOTBLK`，有效Block identity但provider缺失返回`ENOENT`；这才是`VFS-MOUNT-ADMISSION-002` Refine的直接证据。

**Probe exit / exact production tree:** runtime取证后删除`vfs_make_node_probe.rs`、focused group与registration，
恢复`main.rs`和长期`profile.txt = sys`。residual search确认user-test/kernel/anemone-rs中没有probe或test-path
dispatch。exact production tree通过`just fmt kernel --check`、`just fmt user-test --check`、`git diff --check`，并
在sandbox外串行通过RV64/LA64 canonical release build；串行执行避免共享generated DTB竞争。source audit确认35个
production `InodeOps` static均有`make_node`initializer，只有ext4/ramfs directory使用真实callback；make-node没有
provider lookup或重复raw codec，special open没有panic/success stub，temporary validation没有production残留。

**Final review / cutover / stop:** owner/API、final-metadata/publication顺序、existing common-create handoff、reload、
ABI/errno、resource cleanup、自然代码形状与validation provenance复审无active Apollyon/Keter/Euclid/Safe finding。
`ANE-20260801-VFS-CREATE-PUBLICATION-ATOMICITY`继续Open；在本R2 branch-local closure时，no-umask与lwext4
strict failure/crash atomicity仍是Active accepted limitations，前者随后由外层合流R3关闭。`VFS-MAKE-NODE-CUTOVER`因此原子Introduce
`VFS-MAKE-NODE-001`、`VFS-SPECIAL-NODE-RDEV-001`并Refine`VFS-MOUNT-ADMISSION-002`。Stage 2、R2 RFC与本
transaction Completed；C3以单一`vfs-make-node: close stage 2`commit关闭，并在此停止，不进入任何后续gate。
