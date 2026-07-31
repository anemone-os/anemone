# POSIX Record Lock 实施计划

**状态：** R0 implementation plan / Stage 0 Closed / Stage 1 Outline
**适用修订：** R0
**最后更新：** 2026-07-31
**父 RFC：** [RFC-20260731-posix-record-lock](./index.md)
**目标与不变量：** [POSIX Record Lock 目标和不变量](./invariants.md)
**开放问题：** [Tracking Issues](./tracking-issues.md) 当前无 active Apollyon / Keter
**事务日志：** [2026-07-31 POSIX Record Lock](../../devlog/transactions/2026-07-31-posix-record-lock.md)
**Contract Cutover：** prospective `POSIX-LOCK-CUTOVER`；当前 Not Cut Over

本文从 2026-07-31 的 live source 解析首个可执行阶段，并为后续阶段保留滚动 resolution gate。它是 R0 的
canonical implementation plan。R0 acceptance、transaction bootstrap 与开发者明确的 Active authorization 已
作为三个独立事件完成；Stage 0现已关闭，其后的resolution gate仍未授权。

## 实施原则

- Stage 0 先建立 file-table sharing episode 的显式 participation truth 与 opaque holder capability；不实现
  range、close-to-VFS cleanup、`fcntl` ABI 或 blocking wait，也不执行 contract cutover。
- Stage 1 才建立 inode-associated range domain；Stage 2 才组合 native ABI、binding-scoped close/commit
  serialization、wait 与 ordinary signal restart；Stage 3 负责双架构产品证据与原子 cutover。
- 每个阶段按自身条件独立关闭。只有前一阶段 Closed 后，独立的 `N -> N+1 Implementation Resolution Gate`
  才能从 live source 展开下一阶段；future Outline 缺少具体类型、锁、文件和命令不是 finding。
- KUnit 只证明用户态难以稳定命中的 owner/lifecycle topology，并直接经过 production transition；不复制
  test-only episode/refcount 状态机。ABI、errno、range semantics 和 signal replay 由后续 focused userspace、
  LTP 与 runtime 分层证明。
- 实现反馈可以修正保持 target 的 module、stage order、manifest、验证与停止条件；若只能改变 holder、grant、
  lifecycle、wait、ABI 或 acceptance boundary，必须在 cutover 前停止并进入 `Target Renegotiation Gate`。
- `implementation.md` 是 Ready stage 与 resolved manifest 的唯一计划权威；transaction 只记录授权、执行证据、
  finding、路线修正和 closure，不复制一份并列计划。

## 全局受保护边界

- `FILES-POSIX-OWNER-001` 的 behavior identity 只能来自 file-table sharing episode。fd、task/TGID、opened
  description、inode、path、raw pointer 和 ordinary storage refcount 都不能替代 holder。
- `POSIX-LOCK-DOMAIN-001` 要求 inode-associated VFS domain 成为 grant range、mode、conflict 与 wait
  publication 的唯一真相源；Stage 0 holder 和 task file state 不得预存 inode/grant mirror。
- `POSIX-LOCK-LIFECYCLE-001` 的最终协议是 binding-scoped close handoff，不是 holder-wide close epoch，也不是
  opened-description final release。Stage 0 只建立其所需 holder topology，不能预先选择 VFS cleanup API。
- `SCHED-LATCH-001..003`、`SCHED-WAKE-001..004` 保持有效。blocking operation 任一时刻只能有一个 active wait
  round，producer 只提交 recheck notification，不授予 range、不决定 errno，也不等待 waiter 运行。
- `OPENED-DESC-001/002/003`、`OPENED-DESC-RETIRE-001`、`OPENED-DESC-LIVENESS-001` 与
  `FLOCK-DOMAIN/WAIT/LIFECYCLE-001` 全部 Preserve；POSIX holder 不复用 opened-description identity、固定
  final-release hook、flock grant、waiter 或 cleanup state。
- Linux UAPI 只能进入 `anemone-abi` 与 syscall ABI owner；VFS core 不读取用户指针、raw `struct flock` 或
  relative whence。Stage 0 不触碰 UAPI。
- 全部 prospective IDs 在 `POSIX-LOCK-CUTOVER` 前保持 Not Effective。任何 intermediate stage 的成功代码都
  不能单独作为产品 capability 合入或把 current `fcntl` NYI 描述改成已支持。

## Live Source Preflight

本次 resolution 已同步核验到 clean `dev/drc/omega@0dea02d5`，确认以下当前事实：

- `Task.files_state` 是 `RwLock<Arc<RwLock<FilesState>>>`；`Task::files_state()` 会产生普通临时 `Arc`
  observer，`FdReservation` 也可在 task 之外短期持有同一 storage handle。
- `drain_files_state_handle_if_last_arc()` 以 `Arc::strong_count()` 作为 conservative ownership proxy。
  temporary observer 会改变计数，因此它不能成为 `FILES-POSIX-OWNER-001` 的 participation 或 final-detach truth。
- `CLONE_FILES` 直接共享 table handle；普通 fork 调用 `FilesState::fork()` 复制 fd publication。clone 的多个
  rollback path 已在 sleepable context 调用 `close_all_fds_for_exit()`。
- `unshare_files_state()` 当前总是复制并替换 table，即使 table 没有 semantic sharer；成功 exec 在 `dethread()`
  后只执行 `close_cloexec_fds()`，还没有“仅在真正共享时分离”的 file-table episode operation。
- ordinary exit 已在 deferred disposal 前、interrupt enabled 且可抢占的上下文调用
  `close_all_fds_for_exit()`；`FilesState::Drop` 只断言 published/reserved state 已由显式 cleanup 清空。
- `task/files` 已经按 `mod.rs`、`table.rs`、`descriptor.rs` 与 `opened_description.rs` 目录化。本阶段不重复
  split；新增 `episode.rs` 是把新 lifecycle responsibility 放入同一 owner，而不是移动 public owner surface。
- `fcntl` decoder 已识别 `F_GETLK/F_SETLK/F_SETLKW`，但仍返回 NYI；focused `fcntl.txt` 仍注释
  `fcntl14/fcntl14_64`。这些均不在 Stage 0 写集。
- 新合入的 UDP 路径以 `InodeType::Socket` / `S_IFSOCK` 和 `fs::socket::{api,udp}` 表达独立owner；它由
  `VFS-FILE-KIND-001`自然排除在本RFC的`S_IFREG` admission之外，不进入holder、grant或cleanup协议。
- owner-local syscall API清理已把eventfd、timerfd、iomux、epoll与socket adapter收进各自private child
  `api` module；`fs::api`继续拥有尚未单独解析owner的general/VFS-wide入口。当前`fcntl`与close adapter仍在
  `fs/api/{fcntl.rs,close/**}`，且`task/**`、这两组adapter相对旧`9bc6ab459d57`基线没有语义或布局变化。
  因而Stage 0 owner与manifest不变；首次接入record-lock ABI的resolution gate仍必须按届时live owner判断
  POSIX-specific normalization/helper的module归属，不能从当前general dispatcher位置反推长期owner。
- canonical `run-user-test-{rv64,la64}.sh` wrapper只要求通用KUnit完成标记`All tests passed!`，不再把
  EPOLL/UDP app、固定LTP组合或shutdown序列固化为pretest前置。因此Stage 0可以保持当前bounded KUnit route，
  不需要为record-lock重新嵌入无关app、修改`user-test`选择或把feature-specific marker纳入验收。
- register 中 `ANE-20260531-SCHED-EVENT-WAKE-RUNNABLE-RACE` 仍为 Open，而 current `SCHED-WAKE` 已描述
  stale-safe delivery。该状态差异不阻塞无 wait 的 Stage 0，但必须在 Stage 1 -> 2 wait resolution 中重新核验。

## 阶段成熟度与路线图

| Stage | 成熟度 | 概括目的 | Contract Cutover | 解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 0 | Closed | 建立显式 file-table episode/participation truth、opaque holder 与 fork/share/unshare/exec/exit topology proof | None | 2026-07-31 已独立关闭；已停止 |
| Stage 1 | Outline | 建立 inode-associated normalized range domain与 assignment/conflict/query proof | None | Stage 0 独立关闭后 |
| Stage 2 | Outline | 实现 native ABI、binding-scoped close/commit、blocking wait 与 signal restart vertical slice | None | Stage 1 独立关闭后 |
| Stage 3 | Outline | 完成 focused oracle、LTP、RV64/LA64 runtime、最终 review 与 current-contract cutover | `POSIX-LOCK-CUTOVER` | Stage 2 独立关闭后 |

## Stage 0 Ready：File-table Episode 与 Holder Foundation

### 阶段成熟度与激活前置

Stage 0 当前是 `Closed`。进入 Active 前必须依次满足：

1. Draft target、Contract Impact、proof obligations 与本文 Stage 0 共同通过 review，并被接受为 `R0 /
   Accepted for Implementation`；接受不改变 current contract。
2. 本 public RFC 已完成无语义漂移的 promotion audit，公共导航与双向链接完整。
3. 建立 `docs/src/devlog/transactions/2026-07-31-posix-record-lock.md`。若实际 transaction 使用不同日期或
   slug，必须在启动前同步本文的精确 manifest；路径未同步时不得把占位路径当成授权。
4. 重新读取 live HEAD、dirty state、current contracts、register 和本节 source owners。若 source layout、
   task construction/rollback 或 exec/exit ordering 已漂移，先更新本文并重新 review Stage 0。
5. 开发者明确授权 Stage 0 从 Ready 进入 Active。public Draft、R0 acceptance 或 transaction creation 本身均不
   构成启动授权。

**2026-07-31 activation result：** 五项前置均完成。R0 review 接受 target、Contract Impact、proof obligations
与本阶段；transaction 随后建立，开发者明确授权本轮唯一 GOAL“完成 Stage 0”。实现审查发现 unpublished
task 的显式 detach consumer 超出原七文件 manifest，因此触发停止合同；开发者批准下述精确扩展后以
`goal resume`恢复 Stage 0。扩展只关闭 existing task-construction consumer，不改变 target、owner、public API、
shared contract、ABI、visible semantics、acceptance或验证层级。

### 要证明的组合假设

1. file-table owner 可以显式区分 semantic participant 与临时 storage observer，不再让
   `Arc::strong_count()`、raw pointer identity 或 fd count 决定 share/final teardown 行为。
2. ordinary fork 能复制 table publication并创建新 holder；`CLONE_FILES` 能显式 attach 同一 episode并共享
   holder，而不复制 participant truth 到 task 与 table 两处。
3. unshare 与成功 exec 能在一个 episode-local lifecycle transaction 中区分 unique/shared：unique 保持 holder，
   genuinely shared split 创建 fresh holder且不改变 remaining sharers 的旧 holder。
4. 一个 participant detach 不 drain shared table；final detach exactly once drain published slots，并继续通过
   current opened-description release owner完成既有 cleanup。reserved slot 仍只作为 allocator state清除。
5. holder 可以先作为无 grant 的 opaque capability存在，不携带 inode、range、mode、waiter、report PID 或
   cleanup registry；后续 VFS consumer不需要取得完整 `Task`、`FilesState` 或 private table guard。

### 交付与实现路线

#### Episode 与 participation owner

- 新建 `task/files/episode.rs`，把 sharing episode、participant lifecycle、table storage access 与 opaque
  POSIX holder capability封装在 `task::files` owner内。`FilesState`继续只拥有fd allocator/publication内容，
  不成为 participant 或 holder 的第二真相源。
- `Task` 只持有一份 task-owned participation capability。semantic share 只能经显式 attach产生，fork/split只能
  经 owner API 产生 fresh episode；普通 `Arc` clone、read/write guard、fd lookup和`FdReservation`都只是
  operation/storage capability，不增减 participant truth。
- episode-local lifecycle serialization 同时拥有 live participant count、unique/shared decision 与 terminal
  detach transition。计数只服务 behavior protocol，不能另在 `Task` 缓存 `is_shared`、holder ID 或 generation。
  cheap consistency checks使用常开 `assert!`。
- holder 在 episode创建时建立一次，只提供 opaque clone/equality与后续 cleanup attribution所需的窄
  `pub(crate)` capability。它不暴露 storage refcount、participant count、table guard或可变状态；若实现带
  diagnostic label，字段必须明确只用于 KUnit/log/review且不得反向驱动行为。
- participant detach必须由 sleepable owner path显式执行；`Drop`只能暴露“忘记 detach / 未清空 table”的 bug，
  不得承担 close、final-release或未来 POSIX cleanup。terminal transition先撤销 participation并取出待释放fd，
  释放 opened descriptions继续发生在 episode/table guard外。

具体 private Rust 类型名可以在不改变上述 owner boundary的前提下微调；不得因此把 lifecycle state重新塞回
`Task`，也不得保留 `Arc::strong_count()` behavioral fallback。

#### Fork、share、unshare、exec 与 exit topology

- task/kernel/idle construction 创建一个 live participant、一个空 table episode 与一个 holder。constructor、
  unpublished child setup、publish failure和clone rollback必须最终只有一次显式detach责任。
- ordinary fork从父episode取得同步fd-table snapshot，创建fresh episode/fresh holder并发布child
  participation；不复制父holder，也不继承任何未来grant。`CLONE_FILES`只attach父episode，child与parent
  holder equality成立。
- `close_range(UNSHARE)`调用split-if-shared owner operation。unique participant路径保持现有episode与holder；
  shared路径在受控snapshot上创建fresh episode/fresh holder，再把当前task participation从旧episode转移出去。
  后续close/cloexec动作只作用于调用者最终持有的episode。
- 成功exec在`dethread()`之后、`close_cloexec_fds()`之前调用同一个split-if-shared语义。unique exec保持
  holder；shared exec获得fresh holder且不继承旧grant identity；每个实际关闭的`FD_CLOEXEC` slot仍走当前
  opened-description unpublication/release路径。exec失败不改变episode。
- ordinary exit与所有clone rollback继续在现有sleepable boundary调用显式detach。一个shared participant退出只
  减少participation；final participant负责exactly-once drain。`task/api/exit/mod.rs`只允许把当前
  `close_all_fds_for_exit()`调用替换为episode owner的显式detach入口，不改变既有exit/signal/deferred-disposal
  ordering；若正确性要求移动该boundary或修改其它exit语义，先停止并进入owner/manifest review。
- 删除`drain_files_state_handle_if_last_arc()`及所有behavioral strong-count判断。temporary observer在final
  participant detach前后仍可持有storage lifetime，但不得阻止或触发semantic table cleanup；owner API必须让
  observer只能看到已terminal/不可再publication的状态。

### KUnit 与 source proof

形成最小的owner-local focused KUnit集合，集中放在`episode.rs`并直接执行production API；当前预计覆盖以下
三类topology，但具体case数量由production transition的最小证明义务决定：

1. **fork/share identity：** ordinary fork snapshot得到不同holder；`CLONE_FILES` attach得到相同holder；fd-table
   publication snapshot与holder identity彼此独立。
2. **unique/shared split：** unique unshare/exec route保持holder；genuinely shared unshare/exec route得到fresh
   holder，remaining participant仍持旧holder；不为exec复制一份test-only split状态机。
3. **detach/observer：** 一个sharer detach不drain shared table，final detach只drain一次；在相同transition前
   额外持有多个temporary storage observers不改变结果。若exactly-once只能靠test-only counter或production
   reset hook观测，则不新增该hook，改用真实published-description lifecycle observation与常开assert/source audit。

不为holder getter、identity equality、participant increment/decrement分别创建机械测试。KUnit不能通过
`Arc::strong_count()`断言behavior，也不能捕获panic来代替合法production transition。

### 旁路审计

Stage 0 transaction必须记录完整caller分类，而不是只审查新helper：

- `Task.files_state` 的全部constructor与初始化路径；`files_state()`、reservation和其它temporary handle caller；
- clone的`CLONE_FILES`/ordinary fork、unpublished child setup与每个rollback/late failure出口；
- `close_range(UNSHARE)`、unique/shared split、同一调用中的`CLOEXEC`/close后续动作；
- exec成功/失败、`dethread()`、CLOEXEC ordering；
- ordinary exit、kthread/idle/unpublished task teardown、deferred disposal，以及所有
  `close_all_fds_for_exit()` caller；
- `set_files_state()`、`replace_files_state_handle()`、`FilesState::fork()`、final drain与`Drop` assertion的
  全树caller，证明旧storage-handle API已删除或只剩不参与behavior的窄accessor；
- `OPENED-DESC-*` publication/release与`FileDescOps::final_release`保持原样，没有POSIX cleanup/domain或第二
  terminal observer被偷偷加入。

### 模块边界预检

`task/files`当前目录化已经足够承载本阶段，但现有`table.rs`中的`impl Task`同时混有fd operation与table-handle
lifecycle，不能只新增一层type后原样保留。`table.rs`继续拥有`FilesState` allocator/publication与普通fd slot
operation；新增`episode.rs`拥有sharing episode、participant、holder及Task-facing attach/snapshot/split/detach
orchestration。现有`files_state()`、`set_files_state()`、`replace_files_state_handle()`、
`unshare_files_state()`与`close_all_fds_for_exit()`按实际职责移动、收窄或由episode owner API替代；普通open/get/dup/
close/range operation只保留窄delegation，不取得participant truth。`mod.rs`只做private module wiring与必要re-export，
`task/mod.rs`只保存Task-owned participation字段并完成constructor wiring，clone/exec/exit API模块只编排各自既有
lifecycle boundary。

live全树caller当前只位于`task::files`与本阶段列出的clone/exec/exit路径，因此上述方法名和可见性可以作为
kernel-internal implementation surface调整，不要求为旧storage-handle API保留兼容wrapper。实现前的caller audit
若发现manifest外真实consumer、外部crate依赖或必须保持的public owner surface，先停止并按API/write-set扩展处理；
不能为了守住当前文件集制造第二入口或让旧`Arc<RwLock<FilesState>>`重新成为behavior capability。此次同一owner
拆分不改变syscall ABI、shared contract或target owner，但会有意修正kernel内部lifecycle orchestration。

若实现需要把`episode.rs`继续拆成generic manager/framework、把holder公开给当前无consumer的外部crate，或把
task/scheduler/VFS私有状态移入files owner，说明当前route过宽，先停止而不是继续结构化扩张。

### Resolved Write Set Manifest

Stage 0 Active时允许修改的production source：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/files/mod.rs`
- `anemone-kernel/src/task/files/table.rs`
- `anemone-kernel/src/task/files/episode.rs`（新建；包含owner-local focused KUnit）
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/rt.rs`
- `anemone-kernel/src/sched/class/fair/stride.rs`
- `anemone-kernel/src/sched/request.rs`
- `anemone-kernel/src/sched/api/priority/setpriority.rs`

文档与执行证据write-back：

- `docs/src/rfcs/posix-record-lock/{index.md,invariants.md,implementation.md,tracking-issues.md}`：只记录
  Stage 0 maturity、保持target的实现反馈、实际私有类型/module取舍、finding、validation与closure；target或
  Contract Impact变化必须先停止并进入RFC review，不能借本write set自行批准。
- `docs/src/devlog/transactions/2026-07-31-posix-record-lock.md`（新建）：记录activation baseline、caller
  inventory、checkpoint证据、review finding与closure。

Validation-only输入，不在写集：

- `anemone-kernel/src/task/files/{descriptor.rs,opened_description.rs}`
- `anemone-kernel/src/fs/api/close/**`
- `anemone-kernel/src/fs/api/fcntl.rs`及其实际子模块
- `docs/src/contracts/task/opened-description-lifecycle.md`
- `docs/src/contracts/{scheduler,vfs}/**`
- `docs/src/register/{open-issues.md,current-limitations.md}`
- `anemone-apps/user-test/ltp/profile.txt`、`anemone-apps/user-test/ltp/groups/{fcntl.txt,full.txt}`与固定LTP
  source/xref
- `scripts/run-user-test-rv64.sh`
- `conf/rootfs/pretest-rv64.toml`
- 调用者显式选择的sdcard master；wrapper只读取master并创建`build/runtime/pretest-rv64/`下的运行副本

Stage 0明确不得修改`anemone-abi/**`、`anemone-apps/**`、`anemone-kernel/src/fs/**`、上述六个显式
detach KUnit consumer之外的`anemone-kernel/src/sched/**`、current contracts、register、rootfs/profile或公共RFC target正文。R0 acceptance与
transaction bootstrap自身由独立授权完成，不借Stage 0 source manifest自动执行。

如果更自然且保持target的实现需要扩大上述文件集，执行者必须先停止并提交manifest expansion：说明新增文件、
owner理由、contract/ABI影响、验证变化与批准后的文档/transaction记录点。integrator负责拒绝未批准的physical
write；reviewer负责检查每一行diff都能映射到本阶段交付。

### 可观测性与断言

- 常开`assert!`保护participant count非零attach、single terminal detach、terminal后不可publication、
  table drain后无published/reserved slot，以及holder不因unique replacement变化。
- 不新增用户可见日志、procfs/debugfs、syscall或Kconfig。Stage 0没有unsupported ABI路径需要日志兼容。
- 若临时trace是定位lifecycle race所必需，只能在transaction记录范围与结论，并在Stage 0 closure前删除；
  diagnostic field不得参与share/split/drain决策。

### 验证与 review

按顺序执行，不能并行运行两个architecture build，因为它们共享generated artifacts：

```sh
just fmt kernel --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
```

两个build证明production与KUnit在对应architecture下编译，不冒充runtime。Stage 0还必须用同一RV64 preset执行
一次canonical RV64 QEMU KUnit与正常shutdown run：

```sh
./scripts/run-user-test-rv64.sh <sdcard-image> build/posix-record-lock-stage0-rv64.log
```

`<sdcard-image>`必须由调用者按仓库公开接口显式选择，不能替换成无阶段含义的默认路径。2026-07-31 activation
preflight发现tracked `anemone-apps/user-test/ltp/profile.txt`为`sys`，不是Ready时假定的空窗口；live group只含
`confstr01`与`sysconf01`，由双libc形成四个case。保持profile不变并运行canonical wrapper；这些结果只作环境
回归，不成为record-lock或Stage 0 topology验收。日志必须点名上述三类topology的全部新增case、出现
`All tests passed!`、进入init/user-test并完成正常guest shutdown；canonical wrapper
必须正常返回exit 0，不得在marker后通过QEMU monitor或host timeout提前结束，也不得忽略wrapper状态。rootfs
rebuild、runtime disk副本和日志是ignored build side effects，不修改sdcard master或tracked rootfs/profile。

随后执行：

- `rg` caller audit与source classification，确认不存在behavioral `Arc::strong_count()`或第二participant truth；
- `git diff --check`；新文件另做`git diff --no-index --check /dev/null <file>`；
- 对完整Stage 0 diff做task-files owner/lifecycle/concurrency/resource review，核对attach/split/detach linearization、
  guard-out release、exec ordering、rollback和temporary observer；
- 文档link/status/source path audit。Stage 0不运行record-lock userspace、LTP或LA64 QEMU；这些能力尚不存在。

### Contract cutover、反馈与停止条件

**Contract cutover：** `None`。`FILES-POSIX-OWNER-001`与全部`POSIX-LOCK-*`仍为Not Effective；
`OPENED-DESC-*`和当前task file-state行为继续effective。Stage 0代码只能留在同一transaction分支，等待后续阶段
与最终原子cutover，不能作为POSIX lock支持单独合入。

以下任一情况立即停止Stage 0：

- 显式episode truth只能通过Task与episode各存一份participant/share状态，或仍需storage refcount决定behavior；
- semantic detach、fd drain、opened-description release或未来cleanup必须依赖`Drop`才能正确；
- holder只能由opened description、TGID/PID、fd、inode或raw pointer派生，或需要携带grant/inode/wait state；
- successful exec的split只能通过改变dethread、signal、credential或userspace cutover contract才能完成；
- 需要修改Stage 0 manifest外的task/scheduler/VFS/UAPI/current-contract surface才能证明topology；
- review发现active Apollyon/Keter，或真实实现只能形成弱于`FILES-POSIX-OWNER-001` /
  `POSIX-LOCK-TARGET-003`的语义。

保持target的局部类型/锁/ordering失败在transaction记录Route Correction，在resolved manifest内修正并重跑。
owner、target、ABI、contract或acceptance变化进入RFC review/Target Renegotiation Gate；执行事实只写transaction；
保持target的后续stage/manifest变化回写本文；accepted limitation或开放缺陷进入register/current limitations。

### Stage 0 退出条件

- caller inventory完整，所有semantic creation/share/split/detach路径都经过一个episode owner API；temporary
  observer不影响behavior，`Arc::strong_count()` behavioral依赖为零。
- 三组focused KUnit直接经过production route并在RV64 runner通过；RV64/LA64 build、format、whitespace与
  source audit通过，证据分层记录且未把build冒充runtime。
- full Stage 0 diff通过owner/lifecycle/concurrency review，没有未关闭Apollyon/Keter或manifest越界；Euclid必须有
  明确disposition，只有实际影响target、owner、ABI、lifecycle或acceptance时才升级为blocking finding；temporary
  instrumentation为零。
- transaction记录`contract cutover: None`、current contracts未修改、Stage 0代码不可作为partial feature单独
  合入，并把Stage 0标记Closed。
- Stage 1是否已经解析为Ready不属于Stage 0 closure；关闭后必须停在下一独立resolution gate。

**2026-07-31 closure result：** caller inventory、三项production-route KUnit、RV64/LA64 build、RV64完整
wrapper、whitespace、mdBook与full-diff review全部满足。首次runtime发现published kthread缺少detach，已在原
manifest内修复并从formatter起完整重跑。最终独立review为Apollyon/Keter/Euclid/Safe全0，无temporary
instrumentation或remaining Stage 0 blocker。contract cutover为`None`，current contracts/register未修改；
Stage 0代码不构成standalone POSIX-lock capability。Stage 1保持Outline/Unauthorized，本轮未运行下述gate。

### Post-Stage 0 naming alignment

2026-07-31的独立软件工程审查确认Stage 0 owner/lifecycle模型正确，但private类型名把task-facing aggregate与
纯fd-table storage的责任宽窄倒置：原`FileTableParticipation`实际已经承担完整Task files facade，原
`FilesState`则只保存allocator/publication。开发者批准在进入下一resolution gate前完成同owner、行为保持的
命名校正：task-owned facade改称`FilesState`，episode-owned slot container改称`FileTable`，Task字段相应改为
`files_state`。`FileTableEpisode`、participant唯一真相、holder、observer、锁、detach与guard-out release协议均
保持不变。

本节只supersede上文completed Stage 0段落中的private Rust名称，不重写历史manifest或执行事实；该调整不改变
R0 target、owner、public API、shared-contract语义、ABI、visible semantics、acceptance、contract cutover或Stage 1
成熟度。对应Euclid处置与验证证据记录在tracking issue和transaction；Stage 1及其resolution gate仍未授权。

## Stage 0 -> Stage 1 Implementation Resolution Gate

前置条件是Stage 0已按上述review、验证和退出条件独立Closed，并由开发者单独授权本只读gate。gate读取Stage 0
实际diff、transaction evidence、holder/episode实际表示、review findings、live VFS inode/file owners、current
contracts、register与Draft/R0 target，核对以下事项：

- opaque holder如何在不暴露Task/FilesState/table guard的前提下进入VFS；
- local inode-associated identity与现有flock domain/module shape，确认POSIX domain独立且不进入filesystem
  backend，不建立generic file-lock framework；
- normalized half-open/open-ended range的自然表示、overflow boundary、same-owner assignment、split/merge、
  conflict与query snapshot的可测试production API；
- report TGID只作为diagnostic segment field，不反向影响coalesce、holder equality或cleanup；
- Stage 1是否需要最小probe；失败信号、删除/吸收条件以及完整Resolved Write Set Manifest。

gate只把Stage 1解析为Ready/Not Active，不自动实现。若live evidence要求改变holder/domain owner、range target、
flock separation或public contract，停止并进入RFC review，而不是用module preference掩盖target变化。

## Stage 1 Outline：Inode Range Domain 与 Assignment Proof

概括目的：

- 在inode-associated VFS owner下建立独立POSIX record-lock domain，使用Stage 0 opaque holder表达
  `Holder x AbsoluteRange -> Mode`唯一grant truth。
- 证明normalized absolute range上的read/write conflict、same-owner replacement/split/merge、idempotent unlock、
  no-partial-mutation与`F_GETLK`真实conflict snapshot；report TGID仅为允许stale的diagnostic。

前置依赖：Stage 0 Closed；holder capability和participant topology已由真实实现/review证明。

受保护边界：不接入raw UAPI、fd close cleanup、wait/signal或userspace syscall；不与flock共享grant、holder、
waiter或cleanup；不为OFD/deadlock/remote预置owner enum、wait graph或backend hook；contract cutover为`None`。

解析触发点：仅由Stage 0 -> 1 gate从live inode/flock/source/test shape冻结具体module、container、锁、KUnit/probe、
审计、停止/退出条件和逐文件manifest。当前不预先选择`Vec`/tree、O(n) scan、wake source或capacity policy。

## Stage 1 -> Stage 2 Implementation Resolution Gate

Stage 1独立Closed后执行只读preflight，读取range-domain实际diff与proof、Stage 0 episode API、live fd lookup/close/
dup/close_range/exec路径、`fcntl` decoder与UAPI、scheduler wait/signal restart owner、current contracts、register、
focused test assets和固定Linux/LTP source。输出必须完整解析：

- filesystem syscall adapter的live owner边界：当前general `fcntl` dispatch与owner-local private `api`规则如何
  分工，POSIX-specific UAPI copy/normalize落在哪个owner subtree，以及如何避免compatibility re-export、第二入口
  或让物理dispatch位置反向拥有VFS grant state；
- native `struct flock` layout/copy boundary、normalization snapshot、admission/access/errno order与query copyout；
- fd binding capability、close cleanup handoff与range commit如何形成binding-scoped合法结果集合，且不暴露完整
  fd-table guard、不建立holder-wide close epoch；
- check/publish/sleep/recheck、guard-out notification、operation-local waiter cleanup与ordinary restart route；
- close/dup3/close_range/CLOEXEC/exit所有cleanup caller、signal-before/after-commit race matrix、可观测性、focused
  userspace vertical slice和完整Resolved Write Set Manifest；
- `ANE-20260531-SCHED-EVENT-WAKE-RUNNABLE-RACE`与current `SCHED-WAKE`是否仍矛盾：若issue已由现有cutover
  实际关闭，先按register owner完成证据核验；若仍可复现或无法证明stale-safe，则作为Stage 2 Keter停止条件，
  不能用POSIX waiter绕过wait core。

解析只能把Stage 2变成Ready/Not Active。若需要nested active wait、producer等待waiter、同步close cancellation、
shared fd-table private lock下回调VFS，或只能改变target race outcomes，停止并进入RFC review。

## Stage 2 Outline：Native ABI、Close/Commit 与 Blocking Wait

概括目的：

- 在native RV64/LA64 ABI owner实现`F_GETLK/F_SETLK/F_SETLKW`与`struct flock`copy/normalize/admission；
  VFS只接收normalized operation、opaque holder与窄binding validation capability。
- 把任意相关fd close的holder x inode cleanup、closed-binding late-grant exclusion、blocking recheck wait与ordinary
  signal restart组合成真实syscall vertical slice，并保留close/signal/commit允许结果集合。

前置依赖：Stage 1 Closed；range domain API与holder topology稳定；wait-core register状态已核验。

受保护边界：不支持compat64命令面、non-regular/O_PATH、OFD/deadlock/mandatory/remote；不改变ordinary I/O；
不让opened-description final release或flock承担POSIX cleanup；不在本阶段cut over current contract。

解析触发点：Stage 1 -> 2 gate冻结精确ABI文件、syscall/files/VFS/wait模块、userspace oracle最小slice、runtime
smoke、停止/退出条件和manifest。Stage 2成功仍不能启用focused LTP组或把NYI contract写成已支持。

## Stage 2 -> Stage 3 Implementation Resolution Gate

Stage 2独立Closed后，读取完整production diff、transaction evidence、ABI/focused oracle结果、LTP固定source与两套
libc case inventory、tracked rootfs/runner入口、双架构build/runtime资产、current contracts/register和全部review
finding。gate必须冻结：

- `fcntl-test`的`posix-record-lock` suite与逐case target/非目标/race-admissible classification；
- focused `fcntl` LTP group中`fcntl14/fcntl14_64`及实际相关subcase，明确deadlock/mandatory/compat排除；
- RV64/LA64相同profile的顺序执行命令、调用者显式选择的sdcard master、日志路径、PASS/FAIL/TCONF/BROK与timeout
  判据、重复/stress次数和Not Run边界；
- full diff独立review、current contract最小闭包、register/current-limitations同步、public navigation与
  `POSIX-LOCK-CUTOVER`原子write set。

只有上述交付、验证、停止/退出条件与manifest完整后，Stage 3才Ready；不得因Stage 2 syscall smoke成功自动进入
runtime或cutover。

## Stage 3 Outline：产品证据、最终 Review 与原子 Cutover

概括目的：

- 用focused oracle证明range、owner topology、close、blocking、signal/restart与race envelope；用focused LTP组
  对照固定source形成兼容证据；分别完成RV64和LA64 end-to-end runtime。
- 对Stage 0基线到最终候选做完整owner/lifecycle/concurrency/resource/ABI review，修正finding并重跑owning
  evidence；最后一次性cut over `FILES-POSIX-OWNER-001`、`POSIX-LOCK-DOMAIN/WAIT/LIFECYCLE-001`及产品能力。

前置依赖：Stage 2 Closed；测试盘、runner与双架构环境可用；没有active Apollyon/Keter；Stage 3完整Ready并另行
取得Active授权。

受保护边界：build、单架构boot、KUnit、focused oracle与LTP互不替代；缺失runtime必须标记Not Run。任何一项
prospective contract都不得提前Effective；失败时全部保持Not Cut Over，不能用静默兼容把target内缺陷降为限制。

解析触发点：仅由Stage 2 -> 3 gate基于live assets冻结精确case、命令、日志、重复次数、cutover文件和closure
write set。本文不在Outline阶段猜测个人测试盘路径或最终LTP分数。

## 最终停止边界

- stage-local implementation preference失败但target可达：留在当前stage修正，记录Route Correction并重跑。
- owner、correctness invariant、ABI、visible semantics、non-goal或acceptance boundary需要变化：停止并进入
  `Target Renegotiation Gate`；未接受的新target前保持Not Cut Over。
- future Outline的module、文件、stage顺序或验证安排因live source自然变化：在对应resolution gate更新本文，
  不把它误报为manifest越界。
- runtime/asset不可用：证据标记Not Run并保持stage/cutover未关闭；build或另一architecture不能替代。
- final review仍有Apollyon/Keter，或current contracts/register无法在同一原子write set自洽：停止closure，先修复
  finding和文档归属。Euclid必须有明确disposition，但不因其分级本身阻止closure；若实际影响target、owner、ABI、
  lifecycle或acceptance，应升级为对应blocking finding后处理。
