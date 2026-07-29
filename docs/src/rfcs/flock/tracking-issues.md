# Flock Tracking Issues

**状态：** Closed / all current findings neutralized
**最后更新：** 2026-07-29
**父 RFC：** [RFC-20260728-flock](./index.md)
**事务日志：** None

本文保留已经影响 flock Draft target、ABI、owner/contract boundary、implementation readiness或acceptance
判断的design finding及其neutralize依据。当前仍是R0前Draft，没有transaction或implementation execution；
2026-07-29 cooperative direction correction直接更新Draft，不形成R1。

同日后续独立implementation-resolution任务已从live source形成新的Stage 0 Ready plan。下文历史状态中的
“No Ready Stage”与“implementation shape未解析”描述的是cooperative correction刚完成时的事实；它们不恢复
旧precise-cancellation plan，也不表示Stage 0已Active。当前authority见[迁移实施计划](./implementation.md)。

## Apollyon

None。

## Keter

None。

## Euclid

None。

## Safe

None。

## Neutralized

### KETER-FLOCK-004：Draft 把 cooperative retirement 扩张成 precise cancellation

**问题：** 2026-07-28 Draft 不只要求final close终结holder并删除grant，还要求retirement-first的在途
operation统一返回non-restartable`EBADF`、不得普通restart重新lookup fd、close返回前完成当前listener cleanup，
并围绕这些结果冻结Stage 1 Ready与精确race proof。这把基础flock能力之外的concurrent-close策略提升成首版
target，迫使`task::files`、VFS flock、wait core与signal restart共同承担强制取消协议。

Linux generic`sys_flock`在fd lookup后持有file reference，并不把另一个线程的close解释为这类精确取消；本地
LTP`flock01/02/03/04/06`也只覆盖基础ABI与conflict，不要求该强语义。该方向扩大了owner、restart carrier、
race oracle与acceptance surface，却没有增加当前阶段主要flock capability。

**决定：** Draft改为cooperative retirement。final close同步提交`Retired`、删除既有grant并提交recheck
notification；waiter在自己的observation point重验liveness/conflict/signal并完成local cleanup。close不等待
waiter运行或physical placement，不精确裁决retirement、signal、restart与ordinary operation。ordinary
`SA_RESTART`继续使用现有syscall replay，不保存原opened-description identity。唯一不可放松的lifecycle结果是
retired holder不遗留持久grant。

**修复位置：** [index的Target Capability与Cooperative retirement](./index.md#cooperative-retirement)、
[FLOCK-TARGET-004](./invariants.md#flock-target-004---abi-validationordinary-restart-与-admissible-race-outcomes)、
[FLOCK-WAIT-001](./invariants.md#flock-wait-001---wait-publicationpredicate-recheck-与-cooperative-progress)、
[FLOCK-LIFECYCLE-001](./invariants.md#flock-lifecycle-001---terminal-retirement-后无持久-grant)与
[Implementation Resolution状态](./implementation.md)。

**状态：** Neutralized / 2026-07-29 Draft review；canonical target已折回cooperative semantics，旧Stage 1-3、
probe与manifest全部失效。后续独立resolution形成的新Stage 0现为Ready / Not Active；仍无R0、transaction、
implementation execution或contract cutover。

### APOLLYON-FLOCK-003：无 grant retirement 可能遗漏 blocked waiter progress

**原问题：** blocked acquire可以已经在目标inode domain发布waiter，却从未持有grant。若final close只设置
`Retired`，并因查不到grant而不提交任何notification，该operation可能在最后一次predicate recheck后永久
睡眠，无法协作式观察retirement。grant relation为空不等于没有wait publication。

**2026-07-28决定：** 原Draft通过无条件domain publish保证retirement-first operation醒来并精确返回
non-restartable`EBADF`，同时把该要求接入Stage 1 Ready。

**2026-07-29 supersession：** 保留recheck notification作为cooperative progress trigger，但删除精确errno、
同步waiter cleanup与Stage 1依赖。notification只使已发布waiter获得重验机会；waiter自己观察liveness、
conflict与signal并清理。close只等待notification submission，不等待task运行。

**修复位置：** [index的Wait / wake](./index.md#wait--wake)、
[OPENED-DESC-RETIRE-001](./invariants.md#opened-desc-retire-001---terminal-episode-固定进入窄-vfs-flock-cleanup)与
[FLOCK-WAIT-001](./invariants.md#flock-wait-001---wait-publicationpredicate-recheck-与-cooperative-progress)。

**状态：** Neutralized / 2026-07-29 Draft review；lost-wake/progress obligation仍是correctness invariant，但
不再构成close-driven precise cancellation。

### KETER-FLOCK-002：Retirement handoff 与 current-contract delta 尚未闭合

**原问题：** current`OPENED-DESC-003`只承认creation-time固定的单`FileDescOps::final_release` hook；flock又
需要terminal retirement主动删除VFS-owned grant。早期Draft没有决定是否修改existing hook，也没有闭合
`task::files`与VFS flock之间的owner、调用方向、completion与failure boundary。

**2026-07-28决定：** Preserve`OPENED-DESC-003`并拟Introduce`OPENED-DESC-RETIRE-001`，把handoff写成
所有参与者logical relation/wait cleanup均在close返回前完成的fixed VFS facade。

**2026-07-29 supersession：** 保留exactly-once、不可失败、不可动态注册的窄VFS flock handoff，但completion
只覆盖该holder的grant cleanup与recheck notification submission。waiter outcome、operation-local cleanup与
physical placement不属于handoff completion。existing static hook随后运行；future relation不得自然加入该
handoff，也不增加callback slot、registration mirror或backend dispatch。

**修复位置：** [index的Cooperative retirement与Cleanup](./index.md#cooperative-retirement)、
[Contract Impact](./invariants.md#contract-impact)、
[OPENED-DESC-RETIRE-001](./invariants.md#opened-desc-retire-001---terminal-episode-固定进入窄-vfs-flock-cleanup)与
[FLOCK-RFC-005](./invariants.md#flock-rfc-005---flock-retirement-handoff-不是扩展-registry)。

**状态：** Neutralized / 2026-07-29 Draft review；current contracts保持unchanged，新的ID在
`FLOCK-CUTOVER`前Not Effective。后续Stage 0已把fixed context/facade路线解析为Ready，但它仍是可由实现证据
修正的pre-cutover preference，不改变本finding的contract结论。

### KETER-FLOCK-001：并发 final close 与在途 flock 的 ABI 结果未定义

**原问题：** 早期Draft禁止retirement后的晚到persistent grant，但未定义fd lookup成功后，最后一个published
alias与当前acquire/conversion/unlock或blocked wait并发关闭时的用户结果、wait cancellation与restart分类。

**2026-07-28决定：** 原Draft选择两类精确有序结果：operation-first返回普通结果；retirement-first统一返回
non-restartable`EBADF`，不得重新lookup可能复用的fd number。

**2026-07-29 supersession：** 新target明确admissible outcome而不定义唯一winner。普通domain outcome先完成可
返回普通结果；commit前观察`Retired`返回`EBADF`；signal先完成wait outcome则走普通`EINTR` / restart。
ordinary restart重放syscall并重新lookup fd，不保存原identity。所有路径共同受`FLOCK-LIFECYCLE-001`约束：
final close后retired holder不得遗留persistent grant。

**修复位置：** [index的ABI、等待与并发final close](./index.md#abi等待与并发-final-close)、
[FLOCK-TARGET-004](./invariants.md#flock-target-004---abi-validationordinary-restart-与-admissible-race-outcomes)与
[FLOCK-LIFECYCLE-001](./invariants.md#flock-lifecycle-001---terminal-retirement-后无持久-grant)。

**状态：** Neutralized / 2026-07-29 Draft review；ABI现在明确允许结果集合与共同最终状态，不再要求
identity-preserving restart或precise cancellation。
