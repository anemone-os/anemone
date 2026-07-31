# POSIX Record Lock 事务日志

**状态：** Active transaction / Stage 0-1 and Checkpoint 2A Closed / Checkpoint 2B Ready / Not Active / Stage 2 not Closed / Not Cut Over
**日期：** 2026-07-31
**负责人：** doruche, Codex
**RFC：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/index.md)
**实施计划：** [Stage 2 — Checkpoint 2A/2B](../../rfcs/posix-record-lock/implementation.md#posix-record-lock-stage-2)
**适用修订：** R0
**Contract Cutover：** semantic `None` through Checkpoint 2A closure；1A只更新locator；`FILES-POSIX-OWNER-001`与全部
`POSIX-LOCK-*`继续 Not Effective

## 边界

本事务执行 POSIX process-associated byte-range record lock R0。初始activation唯一授权是Stage 0：建立
file-table sharing episode、显式participation与opaque holder foundation，证明fork、`CLONE_FILES`、unshare、
成功exec和exit topology；不实现range domain、`fcntl` ABI、close-to-VFS cleanup或blocking wait。

Stage 0关闭及命名校正完成后，开发者另行授权只读`Stage 0 -> Stage 1 Implementation Resolution Gate`。该gate
把Stage 1解析并拆为1A/1B；两者现均已独立关闭，Stage 1 Closed。开发者随后另行授权只读`Stage 1 -> 2`
resolution gate；Stage 2已解析为2A/2B，开发者随后只授权并已关闭Checkpoint 2A。Checkpoint 2B仍为Ready /
Not Active，Stage 2尚未关闭，任何2B/Stage 3实现或semantic contract cutover均未授权。Stage 0/1/2A stacked
candidate仍不是可独立合入或写成current支持的POSIX record-lock capability。

## R0 acceptance 与 activation

2026-07-31独立文档review共同检查public target、Contract Impact、RFC-local proof obligations、Stage 0 Ready
definition、resolved manifest、validation floor与停止条件，没有active Apollyon或Keter。target以R0 /
Accepted for Implementation接受；该事件不更新current contract，也不自动启动Stage 0。

随后建立本transaction，读取`AGENTS.md`、`LOCAL.md`、canonical RFC、implementation、tracking issues、register、
current contracts与live source。activation baseline为`dev/drc/omega@092f44dceaf3`；开发者授权唯一GOAL“完成
Stage 0”，不得自动进入下一gate。

## Stage 0 frozen manifest

初始production source：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/files/mod.rs`
- `anemone-kernel/src/task/files/table.rs`
- `anemone-kernel/src/task/files/episode.rs`（新建；含owner-local focused KUnit）
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`

实现审查随后发现missing-detach contract覆盖不足并触发停止。开发者批准只增加以下consumer：

- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/rt.rs`
- `anemone-kernel/src/sched/class/fair/stride.rs`
- `anemone-kernel/src/sched/request.rs`
- `anemone-kernel/src/sched/api/priority/setpriority.rs`

扩展只让既有unpublished-task construction显式履行detach责任，不改变target、owner、public API、shared
contract、ABI、visible semantics、acceptance或validation floor。开发者以`goal resume`恢复Stage 0。

文档write-back为RFC四页、本transaction、transaction index、当前双周devlog、`SUMMARY.md`与`rfcs.md`。
current contracts、register、rootfs/profile、apps、VFS与UAPI不在写集。

## Caller inventory 与当前路线

- `Task::new_kernel*`与`Task::new_idle`创建一个空episode和一个live participation；per-CPU static idle task是
  permanent participant，不属于natural-drop路径。
- ordinary fork snapshot fd publication并创建fresh episode/holder；`CLONE_FILES`显式attach同一episode。
- `close_range(UNSHARE)`与successful exec都走split-if-shared；unique保持holder，shared创建fresh holder。
- ordinary exit、clone rollback、kthread publish failure与unpublished scheduler KUnit task必须显式detach。
- `FdReservation`只持storage observer；temporary observer不能增减participant或决定final drain。
- participant `Drop`只断言调用者遗漏显式detach，不承担semantic cleanup。

初始实现把`FilesState`保留为allocator/publication owner，把episode/participation/holder和Task-facing lifecycle
orchestration放入`task/files/episode.rs`，并删除behavioral `Arc::strong_count()`路径。focused KUnit覆盖
fork/share holder identity、unique/shared split、final detach与temporary observer三类topology。

## Implementation finding

### KETER-POSIX-LOCK-007 — unpublished task缺少显式detach

独立实现review发现：`kthreadd` publish failure丢弃回收的task；五个scheduler KUnit helper在
`guard.forget()`后直接构造`Arc<Task>`。两类路径都依赖natural Drop，无法与“Drop只暴露missing detach”并存。

处置是为participation增加attached assertion state；`detach(mut self)`完成participant withdrawal后清除标志，
`Drop`只执行常开断言。上述六个caller在task drop前显式detach；clone所有fallible early return同时完整审计。
finding与精确manifest expansion已由开发者批准并在RFC tracker neutralize。

## Validation ledger

activation preflight发现tracked profile为`sys`，而非Ready plan假定的空窗口。该group只含`confstr01`与
`sysconf01`，双libc共四个case；保持tracked profile不变并通过canonical wrapper运行，只作为环境回归，不作为
record-lock或Stage 0 topology证据。wrapper仍要求完整`All tests passed!` marker并自然走到guest shutdown。

- `just fmt kernel --check`：通过。
- RV64 release build：sandbox内`lwext4` C compile以`Bad system call` / exit 159失败；sandbox外完全相同命令
  通过。前者归类为seccomp环境限制，后者是production/KUnit compile evidence。
- LA64 release build：sandbox外canonical命令通过；只证明compile，不冒充LA64 runtime。

待继续填写：canonical RV64 wrapper及三项topology KUnit、caller/source audit、whitespace、mdBook与full-diff
owner/lifecycle/concurrency/resource review。Stage 0不运行record-lock userspace、focused record-lock LTP或
LA64 QEMU；随行`sys`只作环境回归。

### RV64 run 1 — KETER-POSIX-LOCK-008

首次canonical wrapper在启动282项KUnit后确认三项新增episode topology case均为`ok`，随后TTY
`duplicate_identity_is_rejected_until_abort`停止published worker时触发
`file-table participation dropped without explicit detach`。guest进入emergency PowerOff，wrapper因缺少
`All tests passed!`返回1；该run是有效失败证据，不记为PASS，也未进入init/user-test。

source trace确认TTY只通过ordinary `KThreadBuilder`消费kthread core；根因是`kthread_exit()`绕过user-process
`kernel_exit()`且仍只断言empty fd slots，没有撤销episode participation。修复位于原manifest的
`task/api/exit/mod.rs`：在sleepable current-kthread context、topology unpublication与deferred disposal前调用
`detach_files_for_exit()`。不修改TTY、kthread public API、owner或write set。对应finding以
`KETER-POSIX-LOCK-008` neutralize；全部owning evidence从formatter起重跑。

### RV64 run 2 — topology与lifecycle closure

修复后按绑定顺序重跑：`just fmt kernel --check`、RV64 release build与LA64 release build全部通过；两个build
只证明对应architecture production/KUnit compile。随后同一canonical RV64 wrapper正常返回exit 0：

- 282/282 enabled KUnit全部通过；三项新增case分别为
  `posix_holder_fork_and_share_follow_episode_identity`、
  `posix_holder_split_changes_only_a_shared_episode`与
  `final_detach_ignores_storage_observers_and_drains_once`；
- 出现`All tests passed!`，随后init启动user-test并完成competition environment initialization；
- tracked `sys` profile的glibc/musl `confstr01`与`sysconf01`共4/4 case PASS、FAIL/BROK/infra 0；各case内部
  TCONF保持原始分类，只作为环境回归；
- user-test关闭control pipe，System Power依次完成filesystem、network与device shutdown，并进入RV64 machine
  PowerOff；wrapper未使用monitor/timeout截断。

### Caller/source audit

全树`rg`与逐路径review确认：

- `Task`只有一个`files_participation: RwLock<Option<FileTableParticipation>>`；episode inner的`participants`是
  唯一semantic count，`attached`明确只服务missing-detach断言且不驱动behavior；
- task-files/task lifecycle/scheduler范围内behavioral `Arc::strong_count()`为零；temporary observer只持episode
  storage lifetime，不增减participant；
- ordinary fork、`CLONE_FILES`、`close_range(UNSHARE)`、successful exec、ordinary exit、kthread exit、clone
  rollback、kthread publish failure与五个synthetic scheduler Task均经过单一episode owner API；clone的
  `cur_uspace.fork()` early error也先显式detach；
- per-CPU idle Task与BSP/AP bootstrap Task为permanent/published lifecycle；bootstrap publish failure是fatal
  topology invariant，不是可恢复natural-drop cleanup path；
- participation `Drop`只执行常开missing-detach assertion；`FilesState::Drop`只检查explicit drain结果；唯一
  Drop cleanup仍是`FdReservation`的operation-local allocator rollback，不承担participant/final-release语义；
- `FilesState`只拥有allocator/publication，`FdReservation` observer在terminal drain后rollback幂等、commit被
  terminal participant检查拒绝；opened-description publication/release与`FileDescOps::final_release`文件无改动；
- 旧`files_state()` handle replacement、`close_all_fds_for_exit()`、`unshare_files_state()`与behavioral
  strong-count API全树caller为零。

source audit未发现第二participant truth、Drop semantic detach、holder中的grant/inode/wait state、VFS/UAPI
扩张或temporary instrumentation。`git diff --check`通过；new-file、mdBook与最终独立review继续在下节记录。

## Final independent review

实现与全部validation完成后，恰好一位新的独立subagent对完整dirty diff进行只读终审。结论为Apollyon 0、
Keter 0、Euclid 0、Safe 0，Stage 0 closure blocker为0。review独立确认：

- episode inner participant count是唯一behavior truth，attached只服务Drop assertion；
- attach/fork/split/detach线性化、guard-out release与FdReservation terminal behavior闭合；
- clone全部早退、ordinary/kthread exit、kthreadd failure、scheduler synthetic Task、idle/bootstrap分类完整；
- exec顺序为`dethread -> split-if-shared -> CLOEXEC close`且失败路径不改变episode；
- expanded production diff精确落在批准manifest，current contracts、register、VFS、UAPI与profile均无改动；
- RV64 runtime与LA64 compile证据未互相替代，Stage 1与其resolution gate仍Unauthorized。

## Stage 0 closure

最终`git diff --check`、`episode.rs`与本transaction的新文件whitespace检查均无诊断；两条no-index命令只因
文件与`/dev/null`不同返回预期exit 1。`mdbook build docs`通过。formatter、双架构release build与修复后的
RV64 wrapper证据仍为当前最终code diff的owning evidence。

Stage 0成功关闭，contract cutover为`None`；current contracts与register未修改，`FILES-POSIX-OWNER-001`及
全部`POSIX-LOCK-*`继续Not Effective。当前代码只是后续record-lock capability的内部foundation，不能作为
standalone POSIX-lock支持单独合入。Stage 1保持Outline/Unauthorized；本轮没有运行
`Stage 0 -> Stage 1 Implementation Resolution Gate`，唯一合法后续动作是等待开发者独立授权。

## Post-Stage 0 engineering audit and naming alignment — 2026-07-31

Stage 0关闭后的独立软件工程审查确认一个Euclid：`FileTableParticipation`实际已是完整Task files facade，而
`FilesState`只保存allocator/publication，类型名把aggregate与storage的责任宽窄倒置。开发者明确批准按RFC
positioning的原候选方向完成同owner命名checkpoint并提交聚焦commit，不授权进入Stage 0 -> 1 resolution gate。

实现把task-owned facade改称`FilesState`、纯slot container改称files-module private `FileTable`、Task字段改称
`files_state`，并让内部accessor使用table vocabulary。`FileTableEpisode`、episode inner的唯一participant count、
`attached`诊断字段、holder、observer、锁、attach/fork/split/detach、terminal drain与guard-out
opened-description release全部保持原样。current `OPENED-DESC` contract只同步实现owner名称和“每task独立
participation、共享episode-owned table”的既有事实；R0 target、owner、public API、ABI、visible semantics、
acceptance与contract cutover均未改变。

验证结果：

- `just fmt kernel --check`通过；
- RV64 release build首次在sandbox内以既有`lwext4` `Bad system call` / exit 159失败，sandbox外相同canonical
  命令通过；LA64 release build同样通过；两者只作为production/KUnit compile evidence；
- 全树source scan确认production code中旧`FileTableParticipation` / `files_participation`为零，`FilesState`只有
  task-owned facade定义，`FileTable`保持`task::files` private；
- `git diff --check`与`mdbook build docs`通过，完整diff review未发现Apollyon/Keter或额外Euclid。

本checkpoint不重写Stage 0历史manifest或当时的执行事实；implementation中的post-Stage 0 supersession和
`EUCLID-POSIX-LOCK-003`记录当前术语。未运行QEMU、LTP或Stage 1 gate；Stage 1继续Outline/Unauthorized，全部
prospective POSIX-lock contract ID继续Not Effective。

## Stage 0 -> Stage 1 Implementation Resolution Gate — 2026-07-31

开发者在Stage 0及其命名校正均独立关闭后，明确授权本轮只解析Stage 1，不授权实现。preflight从clean
`dev/drc/omega@2268ca1f`读取Stage 0实际diff、本transaction的review/validation、live`PosixLockHolder`与
`FilesState`、VFS`Inode`/`FlockDomain`、R0 target/current contracts、register、fixed Linux/LTP source及
repository runner。authoritative Ready definition与resolved manifest已写入
[implementation.md](../../rfcs/posix-record-lock/implementation.md#stage-1-closedinode-range-domain-与-assignment-proof)，
本节不复制第二份计划。

resolution确认：

- Stage 0 holder只封装独立opaque identity，不持有task/table/inode/grant truth；VFS可通过crate-private窄类型
  消费same-owner comparison，不需要完整Task、`FilesState`或table guard；
- `Inode`已以direct field拥有独立`FlockDomain`。开发者在resolution review中要求把两个advisory-lock family
  收归共同物理namespace；最终路线先把现有`fs::flock`行为保持地迁入纯wiring的`fs::lock::flock`，再把POSIX
  domain落入`fs::lock::posix`。parent不持有state或共同trait/facade，无需backend hook、path registry或generic
  file-lock framework；
- internal range固定为`u64 start + Option<u64> exclusive end`，`None`表达开放EOF；domain以单一
  `SpinLock<Vec<segment>>`做O(n) conflict/query和same-owner range rebuild，report TGID只作diagnostic；
- Stage 1只证明range/domain，不接入raw UAPI、fd-close cleanup、Event/wait、signal或userspace。开发者允许
  `anemone-abi`/`anemone-rs`在必要时进入write set，但本阶段没有真实consumer，二者明确不在resolved manifest；
- ordinary heap OOM维持kernel-fatal边界；不新增capacity policy、Kconfig、second index、wait candidate或
  persistent diagnostic mirror。focused proof由同一production core末尾的inline KUnit承担，不建立独立probe。

code manifest冻结为`task/files/{episode.rs,mod.rs}`、`fs/{mod.rs,inode.rs}`、现有`fs/flock/**` rename source与
新建`fs/lock/{mod.rs,flock/**,posix.rs}`；current`FLOCK`/`OPENED-DESC` contract只同步implementation locator。
完整文档回写面与validation-only输入见authoritative plan。validation floor为sequential formatter、RV64/LA64 release build、
RV64 canonical wrapper、新文件/全diff whitespace、mdBook、source audit及完整owner/domain/concurrency/resource
review；LA64 runtime、record-lock userspace与LTP仍Not Run且不属于Stage 1 closure。

本gate未发现Apollyon/Keter，也未改变R0 target、owner、ABI、visible semantics、Contract Impact或acceptance。
Stage 1现为Ready / Not Active，contract cutover仍为`None`；没有修改code、current contracts、register、profile、
rootfs或test assets。唯一合法后续动作是等待开发者独立授权Stage 1 Active，不能自动实现或进入Stage 1 -> 2 gate。

## Stage 1 checkpoint split correction — 2026-07-31

开发者在resolution review中指出：原Stage 1把行为保持的flock目录迁移与POSIX range-domain新语义放在同一closure
边界，无法独立证明结构迁移，也使失败归属和后续activation过宽。开发者批准保持R0 target的路线修正，把Stage 1
拆为两个独立checkpoint；authoritative定义与逐checkpoint manifest已回写
[implementation.md](../../rfcs/posix-record-lock/implementation.md#stage-1-closedinode-range-domain-与-assignment-proof)。
本节supersede上一条resolution记录中的单一Stage 1 manifest和“下一步激活Stage 1”措辞，不重写当时的解析事实。

- Checkpoint 1A只把现有`fs::flock`100% rename为`fs::lock::flock`，建立纯wiring parent、更新`Inode` import和
  `fs` re-export，并对current `FLOCK`/`OPENED-DESC` contract做locator-only更新。它不得创建POSIX child/state、
  修改flock函数体或改变effective contract语义。
- 1A验证收窄为kernel formatter、代表性RV64 release build、100% rename/旧路径/source audit、whitespace、
  `git diff --check`和mdBook；QEMU、KUnit runtime、LTP、LA64 build、userspace oracle均Not Run且不属于1A closure。
- Checkpoint 1B在1A独立Closed后才可另行激活；它增加`lock::posix`、inode-owned range domain、opaque holder接线与
  focused KUnit，并保留原Stage 1双架构build、RV64 canonical wrapper和完整review floor。

本修正没有执行production code、build或测试，没有修改current contract/register/profile/rootfs/test asset；也未改变
Stage 1总write set、最终validation floor、R0 owner/ABI/visible semantics/Contract Impact/acceptance。当前唯一合法
后续动作是等待开发者独立授权Checkpoint 1A Active；1A closure必须停止，不能自动激活1B。

## Checkpoint 1A activation, stop and route correction approval — 2026-07-31

开发者随后创建并持续推进唯一GOAL“完成Stage 1 Checkpoint 1A”，明确不得进入1B。activation preflight从
`dev/drc/omega@57e34f44`读取`AGENTS.md`、`LOCAL.md`、canonical RFC四页、register、current transaction、
`FLOCK` / `OPENED-DESC` current contracts与live source；dirty state为空，1A frozen manifest与验证/停止合同有效。

首次候选把三个`fs/flock/**`文件逐字移动到`fs/lock/flock/**`，增加纯wiring `lock/mod.rs`，更新`fs/mod.rs`、
`inode.rs`与两个current-contract locator。`just fmt kernel --check`通过；RV64 canonical build在sandbox内首先命中
既有`lwext4` `Bad system call` / SIGSYS，sandbox外相同命令进入Rust compile后以`E0365` / `E0603`失败。根因是
主模块原有`pub(super)`在迁入`fs::lock::flock`后只对新父模块`fs::lock`可见，无法再由parent re-export给
`fs::inode`；原位置中同一声明的父模块就是`fs`。

该失败触发“flock文件必须100% rename”的1A停止条件。候选保持uncommitted并立即停止；独立边界review确认，
compatibility alias、wrapper/facade、保留旧模块或移动`Inode`都会越过1A owner/API/manifest边界，且不存在在主模块
逐字不变时恢复原`fs`内可见范围的合法路线。

向开发者上报的最小Route Correction只允许以下两处改动：

```rust
pub(in crate::fs) struct FlockDomain
pub(in crate::fs) const fn new()
```

开发者以“批准”明确授权该修正并恢复1A执行。authoritative implementation现要求两个API文件继续100% rename，
主模块除上述两处visibility恢复外逐字不变，并增加exact-diff audit。该修正精确恢复迁移前scope，不扩大crate-public
surface，不改变owner、function body、wait/cleanup、ABI、visible semantics、shared-contract规则、acceptance、
resolved write set或validation floor；R0不递增，semantic contract cutover仍为`None`，1B继续Not Active。

## Checkpoint 1A implementation and validation closure — 2026-07-31

恢复执行后，只在moved主模块把`FlockDomain`与`FlockDomain::new()`改为`pub(in crate::fs)`。最终source diff还包括
纯wiring `fs::lock` parent、`fs/mod.rs` root wiring替换、`Inode` import更新，以及两个current-contract locator；
没有POSIX child/state、compatibility alias、共同trait/facade、function-body或opened-description source改动。

验证与审计结果：

- `just fmt kernel --check`通过；
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`在sandbox内再次由`lwext4`
  `Bad system call` / exit 159停止，sandbox外完全相同命令通过；后者证明代表性RV64 production/KUnit compile，
  前者只记为既有seccomp环境限制；
- 两个API文件分别与baseline blob逐字一致；主模块在仅转换两处批准visibility后与baseline逐字一致，因而全部
  flock function body、grant/wait/retirement与syscall registration保持不变；
- production source中旧`fs/flock/**`文件与`fs::flock`引用为零；`fs::lock` parent只声明private `flock` child和
  既有窄re-export，未出现POSIX state或共同owner；
- `task/files/opened_description.rs` diff为零；两个current contract diff只刷新implementation/source-audit
  locator，stable ID、owner、规则、来源和effective状态不变；
- `git diff --check`通过；新`lock/mod.rs`的no-index whitespace check无诊断，其exit 1只表示文件不同于
  `/dev/null`；`mdbook build docs`通过。

Checkpoint 1A因此按独立边界关闭，semantic contract cutover为`None`；`FLOCK-*`与`OPENED-DESC-*` effective规则
保持不变，全部prospective `FILES-POSIX-OWNER-001` / `POSIX-LOCK-*`继续Not Effective。QEMU、KUnit runtime、LTP、
LA64 build与userspace oracle均Not Run且不属于1A closure。Checkpoint 1B现在是Ready / Not Active，必须等待开发者
另行授权；本次执行在1A closure处停止。

完整diff与docs validation完成后曾启动一位新的只读subagent终审；开发者随即明确指示“不需要再review，直接
提交”，该review在形成结论前被中止，因此本transaction不声称存在1A subagent review结果。主执行者对完整source、
contract、write set与状态diff的审查未发现active Apollyon/Keter；本次按开发者最新指令直接进入checkpoint提交。

## Checkpoint 1B activation preflight — 2026-07-31

开发者把“完成Stage 1 Checkpoint 1B”设为本轮唯一GOAL，明确不得自动进入
`Stage 1 -> Stage 2 Implementation Resolution Gate`，并要求最终由一位subagent做只读review。preflight从clean
`dev/drc/omega@a1c29bfa`读取`AGENTS.md`、`LOCAL.md`、canonical RFC四页、register、current contracts、当前
transaction与live source。Checkpoint 1A已经独立Closed；`PosixLockHolder`、`Inode` construction、
`FlockDomain`与KUnit runner相对resolution baseline没有使1B manifest失效，tracked profile仍为`sys`。

本次production write set冻结为`task/files/{episode.rs,mod.rs}`、`fs/{inode.rs,lock/mod.rs}`与新建
`fs/lock/posix.rs`。文档write-back只覆盖RFC四页、本transaction、transaction index、当前双周devlog与
`rfcs.md`；`SUMMARY.md`导航不变。current contracts、register、ABI、apps、profile、rootfs、filesystem backend、
flock与opened-description source均为validation-only或明确不应触碰边界。preflight未发现Apollyon/Keter，1B进入
Active。

## Checkpoint 1B implementation, validation and Stage 1 closure — 2026-07-31

### Implementation shape

`fs::lock::posix`使用`u64 start + Option<u64> end_exclusive`表达absolute half-open/open-ended range，并由
每个`Inode`直接拥有一个`PosixLockDomain`。domain的单一`SpinLock<Vec<PosixLockSegment>>`是
holder/range/mode的唯一持久真相源；不同holder的read segments可重叠，任一write overlap冲突，同一holder
assignment在operation-local vectors中完成replacement、split与same-mode adjacency merge。set先在同一guard内
检查冲突再替换collection，unlock只减去调用holder的范围且无覆盖时幂等；被替换segments在guard释放后drop。

domain只消费Stage 0 opaque `PosixLockHolder`；production constructor仍只在`FileTableEpisode`，本checkpoint仅增加
KUnit-only独立holder factory与crate-private type re-export。`report_tgid`字段明确为可能stale的纯诊断snapshot，
不参与owner、conflict、canonicalization或lifecycle。`InodeRef`只向`fs`内部提供窄domain accessor，hard-link与
repeated lookup自然按inode identity共享domain，不建立path/backend/numeric-inode registry。

五项inline owner-local KUnit精确覆盖resolved proof names：same-owner mixed-mode replacement/split/merge/unlock，
read/read兼容与write conflict/no-partial-mutation，finite/open-ended query边界与真实conflict snapshot，report
TGID不定义owner/coalescing，以及真实VFS hard-link route的domain association。production range mutation caller为
零；Stage 2必须在binding/close/wait协议解析后才能建立真实entry。

### Route correction

首次RV64 canonical runtime中前四项新增KUnit通过，第五项把same-owner adjacent coalesce后的具体
`report_tgid`固定为单一输入值而失败。source与R0都明确该字段不参与behavior，coalesce可保留任一合法diagnostic
snapshot；因此只删除该过强测试断言，改为验证range/mode canonical result与report属于两个合法输入之一。
该保持target的test-composition修正没有修改production code、owner、ABI、visible semantics、acceptance、manifest或
validation floor。失败run不记为PASS；从formatter、双架构build与canonical wrapper完整重跑owning evidence。

### Validation ledger

- `just fmt kernel --check`在最终source通过。
- RV64 release build首次在sandbox内由既有`lwext4` `Bad system call` / SIGSYS停止；sandbox外完全相同的
  canonical命令通过。LA64 release build随后按顺序在sandbox外通过；两者分别只证明对应architecture compile。
- 修正后的`./scripts/run-user-test-rv64.sh <sdcard-image> build/posix-record-lock-stage1-rv64.log`正常exit 0：
  287/287 enabled KUnit全部通过，五项新增POSIX range/domain case均`ok`，并出现`All tests passed!`；init与
  user-test正常进入，tracked `sys` profile的glibc/musl共4/4 case PASS，最后完成System Power正常shutdown。
  随行`sys`只作环境回归，不证明record-lock ABI。
- LA64 QEMU、record-lock userspace oracle与LTP均Not Run；本checkpoint没有ABI产品capability，以上证据不属于
  Stage 1 closure，也不能由build或RV64 runtime替代。

### Source and boundary audit

全树caller scan与逐文件review确认：

- holder production construction仍只有`FileTableEpisode`，KUnit factory保持conditional；raw pointer、refcount、
  TGID、Task、FilesState或opened description均未替代identity；
- 每个`Inode`只有一个POSIX domain，grant/range/mode只存在于其single guarded vector；operation-local rebuild
  vector不形成第二persistent truth；
- `report_tgid`只进入snapshot、split preservation与新assignment输入，不出现在owner/conflict/merge/cleanup
  predicate；segment order不构成外部保证；
- `FlockDomain`函数体、opened-description lifecycle、filesystem backend、`fcntl` NYI与current contracts/register
  相对1A基线diff为零；旧production `fs::flock` path/reference仍为零；
- 没有Event/wait/candidate/cleanup registry、OFD/deadlock/backend hook、capacity constant、Kconfig、第二索引、
  syscall/VFS facade或production range-mutation caller；
- conflict scan与collection replace在同一domain guard内；guard内只做holder identity/range/vector操作，不回调
  外部owner，被替换segments与潜在last holder reference在guard外drop。

`git diff --check`、新`posix.rs`的no-index whitespace、`mdbook build docs`与最终独立review均在下述closure记录
中通过。contract cutover保持`None`；全部prospective `FILES-POSIX-OWNER-001` / `POSIX-LOCK-*`继续Not Effective，
current contract语义与register不变。

### Final independent review and closure

恰好一位新的subagent对完整production/KUnit/docs dirty diff做只读独立终审。初次状态采样指出transaction当时仍
保留1B Ready页首与final-review pending latch，而其它current-status surface已候选写成Closed；这是提交前必须消除的
执行证据一致性Keter。主执行者在同一轮全状态扫描中已把transaction页首/边界同步为1A/1B与Stage 1 Closed，并在
收到真实review结论后用本节替换pending latch。该修正只完成closure evidence write-back，不改变source、target、
contract、ABI、acceptance或验证边界。

review继续独立核对production source、五项KUnit、frozen write set、current-contract/register/ABI/fcntl/flock/
opened-description/backend排除面与runtime claim分层，最终结论为Apollyon 0、Keter 0、Euclid 0、Safe 0，closure
blocker 0。review确认single inode domain/single guarded Vec、holder与diagnostic边界、conflict-before-mutation、
canonicalization/open-ended transform、guard-out last-ref drop及production mutation caller为零均成立；Stage 2能力
没有被提前接线。

最终`git diff --check`通过；新`fs/lock/posix.rs`的`git diff --no-index --check /dev/null ...`无诊断，exit 1只表示
new file与`/dev/null`不同；`mdbook build docs`通过。formatter、顺序双架构release build与修正后的RV64 wrapper
仍对应最终source diff。Checkpoint 1B与Stage 1因此Closed，semantic contract cutover为`None`，current contracts与
register不变，全部prospective ID继续Not Effective。本轮明确停在`Stage 1 -> Stage 2 Implementation Resolution
Gate`前；该gate、Stage 2及任何后续实现均未授权。

## Stage 1 -> Stage 2 Implementation Resolution Gate — 2026-07-31

开发者在Stage 1独立关闭后明确授权只读解析Stage 2，并允许`anemone-abi`、`anemone-rs`与`anemone-apps`在存在
真实consumer时进入write set；KUnit只保留有意义、直接经过production transition且用户态难以稳定证明的协议
组合。本gate从clean `dev/drc/omega@9d1204e0`读取Stage 0/1实际diff与证据、live fd-table/close/dup/exec/exit、
`fcntl`/user-copy、POSIX domain、Event/wait、signal replay、current contracts/register、fixed Linux/LTP source和
repository app/rootfs/runner。authoritative Ready definition与两个checkpoint manifest已写入
[implementation.md](../../rfcs/posix-record-lock/implementation.md#posix-record-lock-stage-2)，
本节只记录解析证据，不复制第二份计划。

resolution确认：

- `FileDesc.published`是per-slot liveness truth；同一个slot object由lookup binding与close共享，fd reuse创建新对象。
  operation-local capability可以在single episode snapshot中组合opaque holder与slot，不需要holder-wide epoch、
  generation registry或完整fd-table guard；`table.rs`因必须返回被unpublish的同一slot object而进入2A写集，
  `opened_description.rs`只因既有KUnit直接消费该removal返回值而允许适配，production语义不变。
- removal owner在private guard内撤销slot，guard外按POSIX holder x inode cleanup、opened-description release排序；
  普通close、dup3 replacement、close_range、CLOEXEC和final episode drain共享该handoff。commit-before-close由cleanup
  删除，close-before-commit由domain内binding recheck拒绝；其它live binding仍可在cleanup后重新赋锁。
- general `fs/api/fcntl.rs`继续唯一dispatch；POSIX raw byte-copy/normalize/admission进入`fs/lock/posix/api.rs`，domain
  owner仍在`posix.rs`。native `struct flock`冻结32-byte/8-byte alignment及`0/2/8/16/24` offsets；未对齐用户地址
  通过byte slice复制，normalization增加精确`EOVERFLOW`carrier。
- Stage 2拆为stacked 2A/2B：2A交付native ABI、`F_GETLK/F_SETLK`、binding、全部close cleanup与nonblocking/query
  userspace纵切；2B在2A Closed后接入`F_SETLKW`、Event predicate recheck、signal interruption与ordinary replay。
  2A不能独立合入或cut over，2A closure不自动激活2B。
- `anemone-rs` raw `fcntl`已存在；typed wrapper按真实consumer分阶段，2A增加query/nonblocking入口，2B再增加
  `fcntl_setlkw`。新app经两份pretest rootfs安装，并由`user-test::run_local_tests()`在competition chroot前执行。
  无需新runner/wrapper；tracked LTP profile与`fcntl.txt`保持不变。
- KUnit不设数量目标，只考虑closed binding late-commit/fd reuse、commit-before-close cleanup和其它live binding
  post-cleanup commit三个deterministic lifecycle endpoint；若需要test-only state/hooks则不建立，对ABI/range/wait/
  signal矩阵只使用focused userspace与source proof。

### Scheduler register closure evidence

旧register项`ANE-20260531-SCHED-EVENT-WAKE-RUNNABLE-RACE`依赖的`try_to_wake_up() -> task_enqueue()`生产路径已经
由后续协议删除。live `Event::prepare_listener()`使用`ActiveWait::begin()`与exact `WakeToken`；`wake_wait()`只允许
current wait identity提交`Waiting -> Runnable`，旧token对新round返回stale；`local_wake_enqueue()`在owner CPU重验
stale/current/already-queued并且不进入旧strict enqueue尾巴。current `SCHED-WAKE-001..004`自2026-07-25起拥有该
effective规则，相关canonical RV64 wrapper完成全部KUnit、signal/wait随行profile与正常shutdown；Stage 1最终wrapper
也未再出现该断言。

该问题因此不是仍active defect，已从`register/open-issues.md`移除；历史症状、原wait-refactor transaction与本节证据
仍可追溯。此处不修改scheduler current contract，也不把单CPU runtime夸大为新的SMP race matrix。activation
preflight若发现live旁路或该断言重新出现，必须恢复register项并把它作为Stage 2 Keter，不能由POSIX waiter规避。

本gate未发现Apollyon/Keter，未修改production source、current contracts、LTP profile/group、rootfs、apps或wrapper。
Stage 2现为Ready / Not Active，contract cutover仍为`None`，全部prospective IDs继续Not Effective。唯一合法后续动作
是等待开发者另行授权Checkpoint 2A Active；不得自动实现2A、激活2B或进入Stage 3。

## Stage 2 fcntl/POSIX API owner correction — 2026-07-31

开发者在Stage 2 Ready plan复核中指出：POSIX record lock没有独立syscall，三个命令属于`fcntl(2)`；同时仓库
模块规范要求现有`fs/api/fcntl.rs`在增加稳定子职责时目录化为`fs/api/fcntl/mod.rs`，而不是把ABI adapter放进
`fs/lock/posix/api.rs`。live source确认`fcntl.rs`本身拥有`#[syscall(SYS_FCNTL)]`、command decode与总dispatch，
原resolution把raw copy/normalization/admission下沉到VFS lock core会形成错误的ABI owner boundary。

该问题登记为`KETER-POSIX-LOCK-009`并由开发者接受以下docs-only Route Correction：

- Checkpoint 2A把现有`fs/api/fcntl.rs`作为迁移来源并删除旧路径；新建`fs/api/fcntl/mod.rs`，行为保持地承接
  全部既有`fcntl`root职责，再接入record-lock command dispatch；
- 新建`fs/api/fcntl/posix_lock.rs`，唯一承担raw `struct flock` byte copy、validation order、position/size
  snapshot、range normalization、query copyout及internal outcome到Linux errno/restart的映射；
- `fs/lock/posix.rs`只拥有normalized range与grant/conflict/query/assignment/wait/cleanup语义，既不读取raw command、
  用户指针、`l_whence`或Linux errno，也不提供第二syscall入口；`fs/lock/mod.rs`只窄re-export internal operation与
  cleanup能力；live privacy复核确认`task::files`不能直接命名private sibling，因此2A同时把`fs/mod.rs`纳入manifest，
  只逐名crate-private re-export真实cross-owner consumer所需能力，`mod lock`本身保持private；
- `task::files`的fd-removal cleanup直接调用VFS POSIX internal API，不绕回`fcntl`subtree。`fcntl`subtree不拥有
  grant/wait truth，VFS core也不反向拥有Linux ABI。

authoritative 2A manifest因此以`fs/api/fcntl.rs`迁移来源、`fs/api/fcntl/{mod.rs,posix_lock.rs}`两个新路径替换
原计划的新建`fs/lock/posix/api.rs`，并补入窄re-export所需的现有`fs/mod.rs`；2B manifest只保留新的两个`fcntl`
路径与`fs/lock/posix.rs`。上节记录的是
首次resolution的历史事实，本节与当前`implementation.md` supersede其中的旧物理路线，不重写原记录。

本修正不改变R0 target、state/protocol owner、Linux-visible ABI、visible semantics、Contract Impact、acceptance、
validation floor或contract cutover，不增加RFC revision。没有修改production source、current contracts、register、
profile、rootfs、apps或wrapper，也没有执行build、QEMU、KUnit或LTP。Stage 2保持Ready / Not Active；Checkpoint 2A
仍须等待开发者独立授权。

docs-only correction的`git diff --check`与新增行tab/尾随空白扫描无诊断，`mdbook build docs`通过；生成HTML确认
`KETER-POSIX-LOCK-009`与本correction锚点及交叉链接均存在。全树路径扫描只保留上节首次resolution历史记录与本轮
显式supersession说明，不再存在把`fs/lock/posix/api.rs`写成当前manifest或有效路线的表述。

## Checkpoint 2A activation preflight and manifest stop — 2026-07-31

开发者把“完成Stage 2 Checkpoint 2A”设为本轮唯一GOAL，明确不得进入2B，并要求checkpoint完成后由一位
subagent做独立review。preflight从clean `dev/drc/omega@6f041f02`读取`AGENTS.md`、`LOCAL.md`、canonical RFC
四页、register、current contracts、本transaction、live Stage 0/1 source与2A validation/runtime owner。Stage 1
仍Closed；2A authoritative delivery、review、validation、停止/退出条件与resolved manifest均以当前
`implementation.md`为准。除`6f041f02`的docs-only owner correction外，production source相对Stage 1 closure
baseline没有漂移；旧scheduler register项与旧wake tail在live register/source中仍为零。

preflight确认`anemone-rs::sys::linux::fs::fcntl()`已经存在，因而2A的record-lock typed wrapper可在已冻结的
`anemone-rs/src/os/linux/fs.rs`内直接消费它。但2A同时明确要求新的`fcntl-test`覆盖代表性
`close_range` cleanup，而live `anemone-rs`既没有raw `close_range` syscall wrapper，也没有可供app调用的typed
wrapper。raw backend所在的`anemone-rs/src/sys/linux/fs.rs`被2A manifest明确列为validation-only；app也不能访问
crate-private `sys` module。若只守现有manifest，唯一剩余路线是在`fcntl-test`内分别手写RV64/LA64 syscall汇编，
这会复制`anemone-rs`的architecture backend、形成app-local ad-hoc旁路，并使双架构wrapper语义不可由唯一owner
维护，因而被拒绝。

该事实在任何production source修改前触发2A的manifest/public-API停止合同。候选最小扩展为：

- 把`anemone-rs/src/sys/linux/fs.rs`从validation-only提升为2A production write set，只增加一个private raw
  `close_range(first, last, flags)` syscall wrapper；
- 在已经属于2A manifest的`anemone-rs/src/os/linux/fs.rs`增加对应typed `close_range` wrapper，让
  `fcntl-test`成为首个真实consumer；不增加兼容语法、app-local syscall或其它raw wrapper；
- 保持kernel owner、R0 ABI/visible semantics、current contracts、acceptance与contract cutover `None`不变；
  public surface只增加现有kernel syscall的窄userspace调用能力，不改变syscall本身；
- validation仍执行2A现有双架构app build、双架构kernel build与RV64 canonical wrapper，并额外source-audit
  `fcntl-test`只经该唯一wrapper进入`close_range`，app内无architecture syscall backend。

批准前Stage 2继续Ready / Not Active，未修改production source、app、rootfs、current contract、register或RFC
target，也未运行build/QEMU/KUnit/LTP。批准后必须先把上述单文件manifest扩展及public-wrapper边界写回
authoritative `implementation.md`，再由本transaction记录批准事实和生效点；若不批准，则2A无法按当前
focused-app exit condition关闭，不能用省略`close_range`覆盖或app-local汇编冒充完成。

## Checkpoint 2A manifest expansion approval and activation — 2026-07-31

开发者明确同意上一节提出的精确扩展。authoritative `implementation.md`已先完成修正：
`anemone-rs/src/sys/linux/fs.rs`从validation-only提升为2A production source，只允许增加private raw
`close_range(first, last, flags)` wrapper；已在manifest内的`anemone-rs/src/os/linux/fs.rs`提供对应typed public
wrapper，且`fcntl-test`必须成为首个真实consumer。app-local architecture syscall assembly、其它raw/typed wrapper、
兼容入口与无真实consumer的API仍不在写集。

本次批准与manifest修正不改变R0 target或revision、kernel syscall ABI、holder/domain/lifecycle owner、visible
semantics、acceptance boundary、current contracts或validation floor；contract cutover仍为`None`，全部prospective
POSIX contract IDs继续Not Effective。activation baseline固定为clean production source
`dev/drc/omega@6f041f02`；此前唯一dirty内容是上一节preflight/stop transaction记录，`git diff --check`无诊断。
Checkpoint 2A现由Ready进入Active。2B仍为Ready / Not Active，2A closure后必须停止，不能自动进入2B或Stage 3。

## Checkpoint 2A focused-app suite split — 2026-07-31

开发者在首轮focused app实现后明确要求suite不能平铺在`main.rs`。authoritative 2A manifest因此增加同一app owner内的
新文件`anemone-apps/fcntl-test/src/posix_record_lock.rs`：`main.rs`只保留suite参数解析与dispatch，全部
`posix-record-lock` case及其owner-local helper移动到该suite module。该结构修正不增加suite、production API、
kernel owner、ABI、contract或acceptance，也不授权2B。

修正前按顺序执行的`just fmt kernel --check`已通过；`just fmt fcntl-test --check`与
`just fmt user-test --check`各自先报告纯formatter diff，随后经repository formatter修正。首次
`just xtask app build fcntl-test --arch riscv64`在下载依赖索引前因sandbox DNS失败，未进入app编译、artifact export
或runtime；suite拆分后必须从formatter开始重跑canonical验证，不能把该环境失败记录为compile evidence。

## Checkpoint 2A implementation, validation and primary review candidate — 2026-07-31

2A candidate按authoritative manifest完成：native 32-byte `Flock` ABI与typed/raw userspace wrapper、`fcntl`
目录化adapter、operation-local holder/slot binding、`F_GETLK/F_SETLK`、全部published-slot removal handoff与
holder x inode cleanup、owner-local KUnit，以及拆分到`src/posix_record_lock.rs`的8-case focused suite均已接线。
`F_SETLKW`仍在general command decode处返回NYI；没有wait/signal/scheduler路径或current-contract cutover。

主执行者沿ABI、owner、lifecycle、concurrency与resource边界审查时发现并修复一项Apollyon：初始normalization把
half-open endpoint也限制为signed `off_t`，因而错误拒绝Linux允许的`l_start = i64::MAX, l_len = 1`。固定
Linux 6.6.32 `flock64_to_posix_lock()`以inclusive end验证该请求合法，只有`start + len - 1`越过`OFFSET_MAX`
才返回`EOVERFLOW`。最终实现允许half-open `i64::MAX + 1`并规范化为等价open-ended range；focused case同时证明
该terminal byte可set/query/unlock且冲突报告`l_len = 0`，而`l_start = i64::MAX, l_len = 2`精确返回
`EOVERFLOW`。修复保持R0 target、ABI owner、domain owner与manifest不变，不是架构摩擦或target renegotiation。

最终source/caller scan确认：

- ordinary close、dup3 replacement、close_range、CLOEXEC与final drain都返回被unpublish的原`Arc<FileDesc>`；
  episode/table guard释放后统一执行POSIX cleanup，再释放opened-description publication ref；
- binding holder与slot在同一episode guard下取得；`FileDesc.published`仍是唯一commit-time liveness truth，fd reuse
  创建新slot object；domain guard内的recheck与guard外cleanup覆盖commit-before-close/close-before-commit集合；
- `task::files`只持holder与slot capability，VFS inode domain仍是唯一grant truth；raw UAPI/relative whence/Linux
  errno只存在于`fs/api/fcntl/posix_lock.rs`，`fs::lock`保持private且不拥有dispatch；
- 旧`fs/api/fcntl.rs`物理路径已删除，existing non-record-lock branches由`fcntl/mod.rs`保持；`F_SETLKW`没有partial
  wait consumer；flock、backend、opened-description final-release、scheduler、signal、current contracts、tracked
  LTP profile/group与wrapper均无diff；
- `fcntl-test/src/main.rs`只做suite dispatch；suite只通过`anemone-rs::os::linux::fs`进入fcntl/close_range，app内
  没有architecture syscall assembly。raw userspace owner除获批的private `close_range` wrapper外没有API扩张。

最终source依次通过：

```text
just fmt kernel --check
just fmt fcntl-test --check
just fmt user-test --check
just xtask app build fcntl-test --arch riscv64
just xtask app build fcntl-test --arch loongarch64
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
```

两个kernel build沿用相同repository command在sandbox外执行；该路线只规避已复现的`lwext4` sandbox SIGSYS，不
改变构建owner。第一次wrapper重跑因非TTY sudo无法读取rootfs materialization凭据，在kernel/QEMU前退出，不计为
runtime证据；随后以TTY重跑同一canonical命令并exit 0：

```text
./scripts/run-user-test-rv64.sh \
  etc/preliminary/images/sdcard-rv.img \
  build/posix-record-lock-stage2a-rv64.log
```

最终日志包含288项KUnit与`All tests passed!`、`POSIXLOCK2A:SUMMARY:PASS:8`、tracked `sys` profile的glibc
2/2与musl 2/2、以及orderly `PowerOff`。随行LTP不证明record-lock；LA64 runtime、focused fcntl LTP、blocking/
signal语义、Stage 3与`POSIX-LOCK-CUTOVER`均Not Run。当前候选没有remaining Apollyon/Keter，仍保持Checkpoint 2A
Active，等待唯一只读subagent终审后才能写回Closed；2B继续Ready / Not Active。

## Checkpoint 2A independent review findings and repair candidate — 2026-07-31

唯一独立只读subagent终审给出Apollyon 1、Keter 1、Euclid 0、Safe 0，因而拒绝关闭2A：

- Apollyon：`sys_fcntl(fd: Fd, cmd: FcntlCmd, ...)`使syscall macro按形参顺序先做fallible fd parse，导致invalid
  fd与invalid/NYI command并存时`EBADF`早于command decode，违反冻结的validation order。修复把arg0改为
  infallible raw `u64` transport；macro先完成`FcntlCmd` decode，函数体再显式执行`Fd::try_from_syscall_arg()`。
  dispatch、Fd类型、public API与其它command branch均不改变；各分支errno映射不变，唯一可见变化是多重非法输入
  按冻结顺序优先返回command-decode errno。
- Keter：suite拆分后，Ready / Not Active的2B manifest仍把focused suite写集指向只负责dispatch的`main.rs`。
  修复只把该路径改为真实suite owner `src/posix_record_lock.rs`，保持2B未激活且不修改任何2B source。

两项修复都位于既有owner与authoritative manifest边界内，不扩大API/shared contract/ABI/acceptance，不接入
blocking/signal路径，也不是架构摩擦或target renegotiation。修复后`just fmt kernel --check`与
`git diff --check`通过；RV64 kernel build在sandbox内按预期停于`lwext4`编译器`Bad system call`，同一repository
command在sandbox外exit 0。2A保持Active，必须由同一独立reviewer复核为无active finding后才可写回Closed；2B仍为
Ready / Not Active。

## Checkpoint 2A independent review and closure — 2026-07-31

同一独立只读reviewer在两项source/manifest修复后复核确认其成立，并指出transaction最初用“errno均不改变”描述
command-first修复不够精确。文档随后改为：各分支errno映射不变，唯一可见变化是多重非法输入按冻结顺序优先返回
command-decode errno。最终复核检查前后worktree status hash一致，明确确认未修改任何文件；disposition为
Apollyon 0、Keter 0、Euclid 0、Safe 0，允许进入2A closure write-back。

Checkpoint 2A退出条件全部满足：native ABI、nonblocking/query、operation-local binding与全部published-slot
removal cleanup闭合；没有temporary hook、manifest外production文件、owner/API/shared-contract/ABI/acceptance扩张，
也没有blocking wait、scheduler/signal或current-contract接线。primary review发现的terminal signed-offset错误与
final review发现的command/fd validation precedence、2B suite-owner manifest问题均已neutralize；没有意料之外的
架构摩擦或target renegotiation。

最终evidence保持分层：formatter、双架构app/kernel build、RV64 canonical wrapper、source/whitespace与mdBook通过；
RV64日志包含288/288 KUnit、`POSIXLOCK2A:SUMMARY:PASS:8`、tracked `sys` profile的glibc 2/2、musl 2/2与正常
`PowerOff`。LA64 runtime、focused fcntl LTP、blocking/signal/restart语义、Stage 3与`POSIX-LOCK-CUTOVER`均Not Run。
Contract cutover继续为`None`，全部prospective IDs保持Not Effective。

Checkpoint 2A现为Closed并在此停止。Checkpoint 2B继续Ready / Not Active，Stage 2尚未关闭；2A candidate不得
standalone合入、发布或写成current POSIX record-lock支持。后续只能由开发者另行授权2B，不得自动进入2B或Stage 3。
