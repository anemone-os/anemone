# VFS Local Flock 当前契约

**Contract ID：** `FLOCK`
**状态：** Active
**Owner：** inode-associated VFS flock domain
**参与领域：** VFS inode/file / task opened-description lifecycle / scheduler wait / Linux syscall ABI
**覆盖范围：** local whole-file advisory flock的grant/conflict truth、blocking recheck与terminal holder cleanup
**不覆盖：** remote/distributed flock、mandatory locking、lease、deadlock detection、公平性、POSIX/OFD record locks、通用file-lock framework
**实现位置：** `anemone-kernel/src/fs/{flock.rs,inode.rs,api/flock.rs}`、`anemone-kernel/src/task/files/opened_description.rs`
**依赖：** `OPENED-DESC-001/002/003`、`OPENED-DESC-LIVENESS-001`、`OPENED-DESC-RETIRE-001`、`SCHED-WAKE-001..004`
**Pending Successor：** None
**最后核验：** 2026-07-29

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| opened-description identity与terminal liveness | `ProcFile` lifecycle owner | flock domain持opaque non-owning capability；operation取得短lease | alias sharing、commit前liveness recheck与terminal cleanup |
| holder x inode grant relation、mode与conflict predicate | 对应inode-associated `FlockDomain` | syscall/VFS facade只传normalized operation与capability | SH/EX裁决、conversion、unlock与retirement removal |
| wait publication与predicate recheck | 对应`FlockDomain`内的serialization和`Event` | waiter持operation-local listener | 闭合check-then-sleep窗口并消费progress hint |
| signal completion与physical wake placement | wait core / scheduler | flock只提交notification并重验predicate | interruption、restart与实际task调度 |
| Linux flags、fd admission与errno translation | syscall adapter | core不读取raw flags、fd table或用户指针 | Linux `flock(2)` ABI containment |

## FLOCK-DOMAIN-001 — Inode-associated domain 是唯一 grant truth

**规则：** 每个local file identity由其inode-associated VFS flock domain唯一拥有
`OpenedDescription x FileIdentity -> Mode` grant relation、当前mode与conflict predicate。同一opened
description在同一domain至多有一种mode；dup/fork alias共享holder，独立`open()`形成不同holder，硬链接和不同
path到达同一inode domain。SH/EX conversion先删除旧mode，再竞争新mode；重复同mode是idempotent，unlock只
删除调用holder的grant。`ProcFile`、fd slot、syscall adapter、filesystem backend和wait notification不得保存
mode、candidate、waiter或conflict mirror。

**违反表现：** 以fd number、pid、path或临时inode number作为holder/file truth；不同path绕过冲突；同一holder
出现重复grant；backend或`ProcFile`保存并驱动mode；或notification/candidate bit直接决定grant success。

**验证 / Enforcement：** `FlockDomain`所有access与grant mutation source audit、常开duplicate/removal
assertions、owner-local KUnit；focused conflict/idempotence、alias/fork、hard-link、conversion与final-close
regressions，双libc `flock01/02/03/04/06`。

**最初来源：** [RFC-20260728-flock R0](../../rfcs/flock/invariants.md#flock-domain-001---grant-relation-只有一个-vfs-owner)。

**当前来源：** [FLOCK-CUTOVER](../../devlog/transactions/2026-07-29-flock.md#flock-cutover---2026-07-29)。

## FLOCK-WAIT-001 — Wait publication 与 predicate recheck 闭合进度

**规则：** blocking acquire/conversion在domain serialization下检查holder liveness与conflict，并通过
`Event` publication/recheck闭合lost-wake窗口；等待期间不持有domain guard。unlock、conversion old-mode
removal和retirement cleanup在guard外提交notification，使可能eligible的waiter获得重新检查机会。notification
只是progress hint，不转移grant、不表示success或`EBADF`，也不替代waiter自己的liveness、conflict与signal
判断。retirement即使没有找到grant也必须notification；waiter负责operation-local listener cleanup，producer
不等待waiter运行或scheduler placement。

**违反表现：** check与park之间遗漏publication；跨park持domain lock；wake直接提交grant/errno；无grant
retirement不通知已发布waiter；close等待task运行；或用第二份active/completed truth决定进度。

**验证 / Enforcement：** `FlockDomain::{lock,unlock,retire}`的listener/predicate循环、guard-out publish/drop与
`Event::listen(false, predicate)` source audit；focused blocking/multiple-shared/no-grant-terminal/signal-restart
regressions和`SCHED-WAKE-001..004` preservation review。

**最初来源：** [RFC-20260728-flock R0](../../rfcs/flock/invariants.md#flock-wait-001---wait-publicationpredicate-recheck-与-cooperative-progress)。

**当前来源：** [FLOCK-CUTOVER](../../devlog/transactions/2026-07-29-flock.md#flock-cutover---2026-07-29)。

## FLOCK-LIFECYCLE-001 — Retired holder 不遗留或重获持久 grant

**规则：** final published-reference retirement通过`OPENED-DESC-RETIRE-001`同步删除holder已有grant并提交
recheck hint。domain mutation与terminal liveness commit recheck串行化，使并发operation只能先完成普通grant
mutation并随后由retirement删除，或观察`Retired`后停止mutation；retired holder不能遗留或重新取得持久grant。
该约束不规定close、signal、ordinary restart与operation return之间的精确全序：普通结果、`EBADF`或
`EINTR`/restart由实际observation决定，close只等待grant cleanup与notification submission。

**违反表现：** final close返回后旧holder仍有grant；cleanup后in-flight operation重新插入grant；
syscall-local borrow或lease延迟semantic retirement；retirement只设置liveness却不清grant；或为固定race winner
引入第二套cancellation/lifecycle状态。

**验证 / Enforcement：** grant mutation与`OpenedDescriptionCapability` commit recheck serialization audit、
全部published-ref release caller与retirement/static-hook order audit；focused final-close、no-retired-grant、
exec/CLOEXEC与bounded concurrent-close regressions。

**最初来源：** [RFC-20260728-flock R0](../../rfcs/flock/invariants.md#flock-lifecycle-001---terminal-retirement-后无持久-grant)。

**当前来源：** [FLOCK-CUTOVER](../../devlog/transactions/2026-07-29-flock.md#flock-cutover---2026-07-29)。
