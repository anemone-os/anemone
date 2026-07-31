# POSIX Record Lock Tracking Issues

**状态：** R0 / Stage 0-1 Closed / Stage 2 Ready / Not Active / no current findings / Not Cut Over
**最后更新：** 2026-07-31
**父 RFC：** [RFC-20260731-posix-record-lock](./index.md)
**事务日志：** [2026-07-31 POSIX Record Lock](../../devlog/transactions/2026-07-31-posix-record-lock.md)

本文只跟踪已经由文档 review 或后续实现反馈确认、并会影响 target、owner/contract boundary、实现顺序、
停止条件或验收判断的 design finding。当前没有 active Apollyon 或 Keter；既有结论保留在 Neutralized 作为
后续 implementation resolution 与 review 的边界依据。

[实施计划](./implementation.md) 已把 Stage 0关闭；后续独立只读resolution gate把Stage 1解析为
Ready / Not Active。开发者随后批准把结构迁移与新domain拆为Checkpoint 1A/1B；该路线修正
不改变target/owner/contract语义，因此不新增tracking finding。1A执行中的嵌套visibility摩擦按获批Route
Correction关闭，未形成target或design finding；1A/1B与Stage 1现均已Closed。1B实现期间唯一路线修正是删除
KUnit对coalesce后具体diagnostic report值的过强断言；R0明确允许任一合法report snapshot，因此不形成finding。
独立`Stage 1 -> 2`resolution gate随后核验live binding/removal、ABI、wait/restart与test wiring，把Stage 2完整拆为
2A/2B并保持Ready / Not Active；随后owner review发现原计划把`fcntl(2)` command-family ABI错误下沉到
`fs::lock`core，形成`KETER-POSIX-LOCK-009`。开发者接受把现有`fcntl.rs`目录化为
`fcntl/{mod.rs,posix_lock.rs}`并保持VFS core为normalized internal owner；该finding已在同一docs-only Route
Correction中neutralize，没有授权实现。
Stage 0实现审查发现的显式detach consumer遗漏与runtime发现的kthread exit遗漏均已neutralize；最终独立review为
Apollyon/Keter/Euclid/Safe全0。
后续 finding 仍按影响
写回 `index.md`、`invariants.md` 或 `implementation.md`，本文只记录 finding 状态与依据。

## Apollyon

None。

## Keter

None。

## Euclid

None。

## Safe

None。

## Neutralized

### KETER-POSIX-LOCK-001：R0 acceptance 被错误放在 implementation resolution 之前

**原问题：** Draft 曾规定先接受 target 为 R0，之后才创建 `implementation.md` 并解析首个 Ready stage。这与
当前 RFC workflow 的接受边界相反：R0 acceptance 必须同时具备 accepted target / contract delta / proof
obligations 与首个完整 Ready stage；`implementation.md` 是该阶段及 resolved manifest 的唯一计划权威。

**修复：** [RFC 接受边界](./index.md#接受边界)与
[`POSIX-LOCK-RFC-001/002`](./invariants.md#posix-lock-rfc-001--draftr0-acceptance-与-effective-cutover-分离)
已经改为：target review 通过后先创建 Draft `implementation.md`，首个阶段完整达到 Ready 后才进入 R0
acceptance；Ready、R0 acceptance、transaction bootstrap、Active authorization 与 contract cutover继续分离。

**状态：** Neutralized / 2026-07-31 document review；当时尚未创建 `implementation.md`、transaction 或执行授权；
当前 R0、transaction 与 Stage 0 Active authorization 已作为后续独立事件完成。

### KETER-POSIX-LOCK-002：Close cleanup 对 concurrent operation 的约束范围不一致

**原问题：** Draft 正文只禁止依赖已关闭 binding 的 operation 留下 late grant，但 lifecycle invariant 曾扩大为
所有 pre-close invocation 都不得在 cleanup 后提交；这会隐式要求 holder × inode epoch，并错误阻止另一个仍
存活 binding 在 cleanup 后重新取得 lock。

**修复：** [Close handoff 与并发 operation](./index.md#close-handoff-与并发-operation)、
[`POSIX-LOCK-LIFECYCLE-001`](./invariants.md#posix-lock-lifecycle-001--任意相关-fd-close-清除既有-grants-并排除-closed-binding-late-grant)、
capability / linearization boundary 与完成证明已经统一为 binding-scoped 合法结果集合：已关闭 binding 不得留下
late persistent grant；其它 live binding 可以在 cleanup 后独立线性化。Draft 只冻结该结果集合，不冻结检查
时点、锁形状、helper、私有重试或校正路线；具体 serialization protocol 仍由首次同时接入 fd binding 与
domain mutation 的 Ready stage 基于 live source 解析并证明。

**状态：** Neutralized / 2026-07-31 target review；未创建 holder × inode epoch；当前 implementation resolution
未改变该结论；当前 transaction 与执行授权是其后的独立事件。

### KETER-POSIX-LOCK-007：显式 detach contract 遗漏 unpublished task consumers

**原问题：** Stage 0 初始七文件 manifest只覆盖ordinary clone/exec/exit，却遗漏`kthreadd` publish failure与五个
scheduler owner-local KUnit helper。这些路径在`guard.forget()`或publish failure后自然drop unpublished
`Task`；一旦 participation `Drop`只允许暴露missing-detach bug，它们会违反“semantic cleanup不得依赖Drop”的
Stage 0 correctness boundary。仅修已列caller会让production与test construction形成两套生命周期纪律。

**修复：** 完整caller review把新增范围限制为`task/kthread/kthreadd.rs`及五个scheduler KUnit文件。
`FileTableParticipation`增加只用于missing-detach assertion的attached状态；显式`detach(mut self)`先完成语义
withdrawal再撤销该标记，`Drop`只`assert!(!attached)`且绝不清理。kthreadd publish failure与五个helper在
unpublished task可回收前显式调用`detach_files_for_exit()`；clone全部fallible early return同时纳入审计。

**状态：** Neutralized / 2026-07-31 Stage 0 implementation review。原停止合同已触发；开发者批准精确六文件
manifest expansion并以`goal resume`恢复执行。扩展不改变target、owner、public API、shared contract、ABI、
visible semantics、acceptance或contract cutover；实际修复与验证记录在transaction。

### KETER-POSIX-LOCK-008：published kthread exit 未撤销 file-table participation

**原问题：** 首次RV64 canonical run中，三项新topology KUnit均通过，但TTY KUnit停止并join一个正常published
worker后触发participation missing-detach断言。`kthread_exit()`绕过user-process `kernel_exit()`，此前只断言
fd table为空；空slot不等于episode participant已撤销，因此deferred Task disposal仍会依赖natural Drop。

**修复：** `kthread_exit()`在sleepable current-kthread context、topology unpublication与deferred disposal之前
调用同一`detach_files_for_exit()` owner API。该调用同时保留未来kthread实际持有fd时guard-out release所需边界，
不建立第二套kthread-onlytable cleanup。文件已在原Stage 0 resolved manifest内，无需扩展owner或write set。

**状态：** Neutralized / 2026-07-31 RV64 runtime feedback。失败run明确记录为Not PASS；修复后从formatter、
双架构build与canonical RV64 wrapper重跑owning evidence。target、ABI、contract、visible semantics与acceptance不变。

### KETER-POSIX-LOCK-003：Contract Impact 未覆盖相邻 lifecycle / wait 最小闭包

**原问题：** Contract Impact 只列出 grant domain 与部分 opened-description IDs，没有显式保护 fixed
final-release hook、terminal flock retirement、flock wait 与 flock lifecycle，因此不足以约束本次 task-files / VFS
handoff 的完整 Preserve surface。

**修复：** [Contract Impact](./invariants.md#contract-impact)已补入 `OPENED-DESC-003`、
`OPENED-DESC-RETIRE-001`、`FLOCK-WAIT-001` 与 `FLOCK-LIFECYCLE-001`，并要求后续 source / cutover review
证明 POSIX cleanup 不复用 static hook、不扩张 fixed retirement，也不与 flock 共享 grant、waiter、notification 或
cleanup state。具体 helper、调用顺序与模块仍留给 Ready resolution，只保护 current owner 与 effective semantics。

**状态：** Neutralized / 2026-07-31 contract review；Preserve-only，未修改 current contract、代码、transaction
或执行授权。

### KETER-POSIX-LOCK-004：首个 Ready stage 的 foundation 与 close/wait 审计义务冲突

**原问题：** target风险与RFC-local proof obligation曾要求“首个Ready stage”解析binding close/grant commit
serialization并审计active wait全链；live implementation resolution又表明首个安全可执行stage应先在
`task::files`建立显式episode/participant truth，且不得提前接入VFS、syscall或wait。若同时保留两种表述，Stage 0
要么无法成为Ready，要么必须越过最小owner/lifecycle foundation扩张成全功能纵切。

**修复：** [RFC风险与lifecycle边界](./index.md#风险)、
[`POSIX-LOCK-RFC-003`](./invariants.md#posix-lock-rfc-003--wait-core-single-active-规则不可降级)与
[实施计划](./implementation.md)现统一为：Stage 0只解析file-table episode/holder foundation；首次同时接入
fd binding与domain mutation、首次接入blocking wait的Ready stage必须完整承担原close/wait审计义务。当前
Stage 1 -> 2 Resolution Gate已把这些义务列为Stage 2变成Ready的硬前置，未改变target lifecycle、wait或最终
proof boundary。

**状态：** Neutralized / 2026-07-31 implementation resolution；未接受R0、创建transaction、修改current
contract或授权Stage 0 Active。

### KETER-POSIX-LOCK-005：Stage 0 在 KUnit marker 后截断 canonical wrapper

**原问题：** Stage 0验证曾允许在`All tests passed!`后通过QEMU monitor主动结束guest，并忽略wrapper状态。Stage 0
改变的正是file-table participant、clone rollback、exec split与exit/final drain lifecycle；只观察KUnit marker不能
证明init/user-test进入、task teardown与正常shutdown仍可收敛，也把repository-owned wrapper降级成可绕过的命令。

**修复：** [Stage 0验证](./implementation.md#验证与-review)现要求tracked active profile保持无active group、
canonical RV64 wrapper正常返回exit 0，并同时观察全部新增KUnit、`All tests passed!`、init/user-test进入与正常guest
shutdown。profile漂移必须先更新并重新review验证路线；不得用monitor、host timeout或忽略wrapper状态绕过。

**状态：** Neutralized / 2026-07-31 implementation-plan review；只修正文档验证边界，未运行wrapper、修改profile、
建立transaction或授权Stage 0 Active。

### EUCLID-POSIX-LOCK-001：Euclid 被错误提升为 closure blocker

**原问题：** Stage 0与final closure曾要求Apollyon、Keter、Euclid全部为零，使局部耦合、命名或可后续修正的测试
形状也能无限期阻止阶段关闭，与当前review分级不符。

**修复：** [Stage 0退出条件](./implementation.md#stage-0-退出条件)与
[最终停止边界](./implementation.md#最终停止边界)现只让Apollyon/Keter直接阻止closure。Euclid必须有明确
disposition；若实际影响target、owner、ABI、lifecycle或acceptance，则应按真实影响升级为blocking finding。

**状态：** Neutralized / 2026-07-31 implementation-plan review；未降低correctness、target或最终证据要求。

### EUCLID-POSIX-LOCK-002：KUnit 数量上限反向约束证明形状

**原问题：** Stage 0曾把owner-local focused KUnit硬限制为最多三项。三类topology coverage有意义，但case数量不是
correctness boundary；若production transition的最小直接证明自然需要更多case，固定上限会迫使测试合并或漏证。

**修复：** [KUnit与source proof](./implementation.md#kunit-与-source-proof)现要求最小focused集合，并保留三类
topology作为coverage方向；具体case数量由production transition的最小证明义务决定，仍禁止机械getter/equality/
counter测试和test-only状态机。

**状态：** Neutralized / 2026-07-31 implementation-plan review；未增加production test hook或扩大Stage 0 write set。

### EUCLID-POSIX-LOCK-003：task-files facade 与 fd-table storage 命名倒置

**原问题：** Stage 0实现中的`FileTableParticipation`已经承担Task-facing attach/fork/split/detach、fd operation
delegation与holder获取，是完整task files facade；同一实现却把只保存bitmap、reservation与fd slots的内部容器
继续命名为`FilesState`。类型名因此把aggregate与storage的责任宽窄倒置，后续holder/VFS consumer接线会沿用
不自然的owner vocabulary，但当前唯一participant truth、锁与lifecycle行为没有错误。

**修复：** task-owned facade改称`FilesState`，纯allocator/publication container改称private `FileTable`，
`Task.files_participation`改称`Task.files_state`，内部accessor同步使用table vocabulary。`FileTableEpisode`、holder、
observer、participant count、attached诊断字段、detach与opened-description release顺序全部保持不变；current
`OPENED-DESC` contract只同步实现owner名称，不改变effective语义。

**状态：** Neutralized / 2026-07-31 post-Stage 0 engineering audit。开发者明确批准该同owner命名checkpoint与
聚焦commit；在该checkpoint，R0、Stage 0 closure、contract cutover与Stage 1 Outline/Unauthorized状态不变。
验证证据见transaction。

### KETER-POSIX-LOCK-006：Episode 拆分未约束现有 lifecycle orchestration 的归属

**原问题：** Stage 0曾声明`table.rs`只拥有allocator/slot、`episode.rs`拥有sharing/lifecycle/holder，却没有处理
`table.rs`现有`impl Task`中混合的storage accessor、handle replacement、unshare与exit/final drain orchestration。
若只新增episode type并保留旧块，participant truth仍会被旧storage-handle API旁路；若实现时临时移动caller，原
manifest又没有覆盖exit boundary和internal API/visibility调整。

**修复：** [模块边界预检](./implementation.md#模块边界预检)现按owner role而非最终函数名冻结拆分：`table.rs`
拥有`FilesState`与普通slot operation，`episode.rs`拥有participant/holder和attach/snapshot/split/detach
orchestration，`task/mod.rs`只保存Task-owned participation，clone/exec/exit只编排既有lifecycle boundary。live
caller核验把精确文件manifest收口在七个production文件，并把`task/api/exit/mod.rs`从validation-only提升进write
set；最终private类型、方法名和逐函数落点仍是implementation preference。若activation preflight发现manifest外
consumer或必须保持的public owner surface，必须按API/write-set扩展停止处理。

**状态：** Neutralized / 2026-07-31 implementation-plan review；只解析Stage 0文件级manifest与owner角色，未冻结
Rust类型/函数、修改production source、改变target/contract或授权Stage 0 Active。

### KETER-POSIX-LOCK-009：`fcntl` command-family ABI 被错误下沉到 VFS lock core

**原问题：** Stage 2首次Ready plan保留`fs/api/fcntl.rs`作为general syscall dispatcher，却计划新建
`fs/lock/posix/api.rs`承担raw `struct flock` copy、relative-whence normalization、admission与errno/restart映射。
POSIX record lock没有独立syscall；`F_GETLK/F_SETLK/F_SETLKW`属于`fcntl(2)` command family。该落点会让VFS
lock core理解用户指针、raw command和Linux错误语义，并制造一个名为`api`、实际却不是syscall owner的旁路入口。

**修复：** [Stage 2 Ready plan](./implementation.md#stage-2-ready--not-activenative-abiclosecommit-与-blocking-wait)
现在要求Checkpoint 2A把现有`fs/api/fcntl.rs`行为保持地迁入`fs/api/fcntl/mod.rs`，由root继续拥有
`sys_fcntl`、command decode与总dispatch；新建`fs/api/fcntl/posix_lock.rs`承担record-lock command-family的
raw copy、validation/normalization、copyout和errno/restart映射。`fs/lock/posix.rs`只接收normalized internal
operation并拥有grant/conflict/query/assignment/wait/cleanup truth；`fs/lock/mod.rs`与`fs/mod.rs`只逐名
crate-private re-export真实consumer所需的internal operation/cleanup，整个`fs::lock`保持private；`task::files`
close cleanup直接调用该窄VFS API，不绕回`fcntl`。旧`fcntl.rs`与原计划的`fs/lock/posix/api.rs`均不得保留
compatibility入口。

**状态：** Neutralized / 2026-07-31 Stage 2 docs-only Route Correction。开发者明确接受目录化规范；修正只调整
Ready implementation route与resolved manifest，不改变R0 target、state/protocol owner、ABI、visible semantics、
Contract Impact、acceptance或cutover。Stage 2保持Ready / Not Active，未修改production source或运行测试；证据见
[transaction correction](../../devlog/transactions/2026-07-31-posix-record-lock.md#stage-2-fcntlposix-api-owner-correction--2026-07-31)。
