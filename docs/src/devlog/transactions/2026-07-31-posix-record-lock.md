# POSIX Record Lock 事务日志

**状态：** Active transaction / Stage 0 Closed / stopped before Stage 1
**日期：** 2026-07-31
**负责人：** doruche, Codex
**RFC：** [RFC-20260731-posix-record-lock R0](../../rfcs/posix-record-lock/index.md)
**实施计划：** [Stage 0 — File-table Episode 与 Holder Foundation](../../rfcs/posix-record-lock/implementation.md#stage-0-readyfile-table-episode-与-holder-foundation)
**适用修订：** R0
**Contract Cutover：** `None` for Stage 0；`FILES-POSIX-OWNER-001`与全部`POSIX-LOCK-*`继续 Not Effective

## 边界

本事务执行 POSIX process-associated byte-range record lock R0，但本轮唯一授权仅为 Stage 0。Stage 0建立
file-table sharing episode、显式participation与opaque holder foundation，证明fork、`CLONE_FILES`、
unshare、成功exec和exit topology；不实现range domain、`fcntl` ABI、close-to-VFS cleanup或blocking wait。

Stage 0关闭后必须停止。`Stage 0 -> Stage 1 Implementation Resolution Gate`、Stage 1及任何contract cutover
均未授权。Stage 0代码不是可独立合入的POSIX record-lock capability；current contracts与register保持不变。

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
