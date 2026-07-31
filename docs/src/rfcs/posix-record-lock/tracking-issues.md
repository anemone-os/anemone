# POSIX Record Lock Tracking Issues

**状态：** Draft / no active Apollyon or Keter
**最后更新：** 2026-07-31
**父 RFC：** [RFC-20260731-posix-record-lock](./index.md)
**事务日志：** None；尚未进入实现。

本文只跟踪已经由文档 review 或后续实现反馈确认、并会影响 target、owner/contract boundary、实现顺序、
停止条件或验收判断的 design finding。当前没有 active Apollyon 或 Keter；既有结论保留在 Neutralized 作为
后续 implementation resolution 与 review 的边界依据。

[实施计划](./implementation.md) 已把 Stage 0 解析为 Ready / Not Active，后续类型/锁/容器/逐文件 write set 与
精确命令继续由滚动 resolution gate 解析；尚未接受 R0、建立 transaction、授权执行或运行验证都是当前 Draft
阶段的预期状态，不构成 tracking issue。若后续 review 发现问题，应先把修复折回 `index.md`、`invariants.md`
或 `implementation.md`，本文只记录 finding 状态与依据。

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
当前 implementation 已创建但仍无 R0 / transaction / 执行授权。

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
未改变该结论，也未创建 transaction 或执行授权。

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
