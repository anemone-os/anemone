# Flock 目标与不变量

**状态：** R0 Accepted / Not Cut Over
**最后更新：** 2026-07-29
**父 RFC：** [RFC-20260728-flock](./index.md)
**适用修订：** R0

本文定义第一版本地 flock 相对 current effective contract 的 R0 delta、尚未 cutover 的 target
invariants，以及只服务本 RFC 的 proof obligations。当前 effective 规则仍以 `docs/src/contracts/` 为唯一
权威；本文拟 Introduce 的 `OPENED-DESC-RETIRE-001` 与 `FLOCK-*` IDs 在 `FLOCK-CUTOVER` 前都不是 current
contract。

2026-07-29 Draft review 已把 concurrent final close 从 precise cancellation 调整为 cooperative retirement。
该调整放宽的是 waiter execution、errno 胜负、restart identity 与物理 cleanup timing，不放宽 grant 单一真相、
terminal liveness、final-close grant cleanup、no-late-persistent-grant 或 wait publication/recheck 闭环。

## 规则分类

- **Correctness Invariant：** 状态唯一 owner、并发、生命周期、cleanup、内存安全与 ABI 诚实性；违反即实现
  错误，不能作为工程降级项接受。
- **Target Guarantee / Capability：** R0 承诺的功能范围、兼容覆盖与 acceptance boundary；只能经后续
  accepted revision 调整。
- **Implementation Preference：** 类型、helper、container、allocation、锁、wait primitive、模块、stage、
  write set与命令；R0 target不冻结这些内容，只有`implementation.md`可为当前Ready stage局部冻结，且不得
  反向提升为target guarantee。

## Contract Impact

`FLOCK-CUTOVER` 是 R0 接受的语义切换单元，不是 implementation stage，也不授权执行。只有 future
transaction 完成代码、验证、review 与 contract write-back 后，新 IDs 才能成为 effective。

| Contract ID | 变化 | 当前规则 | R0 Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| [`OPENED-DESC-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-001--published-slot-refcount-是-final-release-的唯一真相) | Preserve | published slot refcount 是 terminal retirement 唯一真相 | flock 不让 syscall borrow、waiter或VFS storage延迟/复活retirement | 全程 |
| [`OPENED-DESC-002`](../../contracts/task/opened-description-lifecycle.md#opened-desc-002--dupfork-共享-descriptionfd-table-只拥有-publication) | Preserve | dup/fork共享description，fd table只拥有publication | alias共享同一holder；fd number、path、inode不替代description identity | 全程 |
| [`OPENED-DESC-003`](../../contracts/task/opened-description-lifecycle.md#opened-desc-003--当前-final-release-callback-是创建时固定的单-hook) | Preserve | creation-time固定的单`FileDescOps::final_release` hook | static hook保持；flock使用独立的mandatory VFS handoff，不覆盖或组合该hook | 全程 |
| [`OPENED-DESC-LIVENESS-001`](../../contracts/task/opened-description-lifecycle.md#opened-desc-liveness-001--non-owning-capability-只验证-terminal-opened-description-liveness) | Preserve | opaque non-owning capability与operation-local lease只验证terminal liveness | VFS commit前重验liveness，但不取得lifecycle word、长期strong hold或restart identity | 全程 |
| [`SCHED-WAKE-001..004`](../../contracts/scheduler/wake-delivery.md) | Preserve | wait core拥有logical completion，scheduler拥有physical placement obligation | flock只提交recheck hint；不观察placement，不等待task运行，不把wake当grant或errno | 全程 |
| `OPENED-DESC-RETIRE-001` | Introduce | None（尚未生效） | `ProcFile` lifecycle owner在static hook前exactly-once进入窄VFS flock cleanup；返回只等待grant cleanup与recheck submission | `FLOCK-CUTOVER` |
| `FLOCK-DOMAIN-001` | Introduce | None（尚未生效） | inode-associated VFS flock domain唯一拥有holder × file identity grant relation、mode与conflict predicate | `FLOCK-CUTOVER` |
| `FLOCK-WAIT-001` | Introduce | None（尚未生效） | wait publication/recheck闭合lost-wake；retirement notification只是协作式progress hint | `FLOCK-CUTOVER` |
| `FLOCK-LIFECYCLE-001` | Introduce | None（尚未生效） | final retirement删除既有grant且retired holder不能遗留持久grant；不定义精确syscall-outcome全序 | `FLOCK-CUTOVER` |

`OPENED-DESC-003` 的 static hook 与 `OPENED-DESC-RETIRE-001` 的 mandatory flock handoff 是两个不同义务。
后者只闭合当前 VFS-owned flock relation，不形成 dynamic registry、backend hook、future-participant framework
或同步 waiter cancellation surface。

## Target Invariants

### OPENED-DESC-RETIRE-001 - Terminal episode 固定进入窄 VFS flock cleanup

**分类：** Correctness Invariant。

**规则：** `ProcFile` lifecycle owner单独编排terminal retirement。fd table先撤销最后slot publication并释放
private guard；首次`Live(1) -> Retired`后，owner在同一显式、可睡眠lifecycle path中exactly-once进入窄VFS
flock cleanup。handoff只接收opaque opened-description identity/retirement context与owner-approved VFS target，
不得读取lifecycle word、fd-table lock或完整`ProcFile`。

handoff不可失败，返回前删除该holder已存在的flock grant，并为目标domain中已经发布的waiter提交一次recheck
notification。没有grant时removal可以no-op，但notification不能因此遗漏。handoff不等待waiter运行、选择errno、
完成task physical placement或销毁全部operation-local资源；这些属于waiter、wait core与scheduler各自生命周期。
mandatory cleanup返回后才运行existing creation-time single`FileDescOps::final_release`。

**Owner：** `ProcFile` lifecycle owner拥有terminal episode与participant顺序；VFS flock owner拥有grant cleanup与
recheck source；waiter owner拥有operation-local wait cleanup。

**依赖：** `OPENED-DESC-001/002/003`、`SCHED-WAKE-001..004`。

**违反表现：** final close遗漏grant cleanup；无grant时遗漏recheck使已发布waiter永久睡眠；handoff等待task
实际运行；动态注册或覆盖existing hook；VFS读取`description_refs`或回取fd-table lock。

**Cutover：** `FLOCK-CUTOVER` 后写入opened-description lifecycle current contract。

### FLOCK-DOMAIN-001 - Grant relation 只有一个 VFS owner

**分类：** Correctness Invariant。

**规则：** inode-associated VFS flock domain单独拥有
`OpenedDescription × FileIdentity -> Mode` relation、同一file identity上的全部grants与conflict predicate。
semantic holder是opened description；inode identity只提供聚合裁决点。syscall、fd slot、`ProcFile`、VFS
`File`与filesystem backend不得保存mode mirror或第二份grant truth。

**Owner：** VFS flock domain。

**依赖：** `OPENED-DESC-001/002`。

**违反表现：** independent open不冲突；hard-link path形成多个domain；holder mode与conflict set漂移；fd reuse
命中旧owner。

**Cutover：** `FLOCK-CUTOVER` 后写入VFS flock current contract。

### FLOCK-TARGET-001 - 首版本地 whole-file advisory capability

**分类：** Target Guarantee / Capability。

**规则：** 第一版支持本机范围内的`LOCK_SH / LOCK_EX / LOCK_UN / LOCK_NB` whole-file advisory flock。
所有有效、非`O_PATH`的local VFS`File`进入generic capability，不按regular、directory、device、pipe、
anonymous/control provenance或读写open mode建立whitelist。remote filesystems不在本target。

**Owner：** syscall ABI adapter负责admission；VFS flock owner负责normalized operation。

**依赖：** `FLOCK-DOMAIN-001`。

**违反表现：** regular-file-only fast path；普通local file无contract地返回`EINVAL/ENOTSUP`；ordinary I/O因
advisory grant被拒绝；backend复制flock state。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

### FLOCK-TARGET-002 - Conflict 与 owner-local operation

**分类：** Target Guarantee / Capability。

**规则：** 不同owner的shared/shared兼容；exclusive与其它owner的shared/exclusive冲突。同一owner在同一file
identity上最多持一种mode；same-mode request幂等，other-mode request是conversion，unlock只删除该owner
grant且无grant时不影响其它owner。

**Owner：** VFS flock domain。

**依赖：** `FLOCK-DOMAIN-001`。

**违反表现：** 同一owner出现两份grant；unlock删除其它owner；SH/SH错误冲突；exclusive与其它grant共存。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

### FLOCK-TARGET-003 - Conversion 是明确的非原子能力

**分类：** Target Guarantee / Capability。

**规则：** SH/EX conversion先撤销旧mode，再竞争target mode；两步之间其它waiter可以获得grant，`LOCK_NB`
conflict返回后旧grant可以已经丢失。实现不得把“尽量原子”写成用户可依赖保证，也不得在conflict、signal或
retirement后无条件恢复旧mode。

**Owner：** VFS flock domain。

**依赖：** `FLOCK-DOMAIN-001`、`FLOCK-TARGET-002`。

**违反表现：** conversion保留旧grant参与冲突；失败后恢复旧mode造成强原子语义；两个mode同时存在。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

### FLOCK-TARGET-004 - ABI validation、ordinary restart 与 admissible race outcomes

**分类：** Target Guarantee / Capability。

**规则：** invalid/combined/unknown operation返回`EINVAL`；invalid fd与`O_PATH`返回`EBADF`；`LOCK_NB`
conflict返回`EAGAIN / EWOULDBLOCK`。blocking wait不restart时返回`EINTR`，允许`SA_RESTART`时使用现有
syscall replay并重新申请normalized target mode；不保存原opened-description identity，重放时普通fd lookup
可以观察signal handler后的fd-table状态。

concurrent final close没有唯一结果：普通domain outcome先完成时可返回普通结果；commit前观察`Retired`时停止
修改grant并返回`EBADF`；signal先完成wait outcome时进入普通`EINTR` / restart。RFC不要求retirement精确压过
signal或already-completed operation，也不承诺success后的grant晚于concurrent final close继续存在。

**Owner：** syscall ABI adapter负责flag/fd/error translation；signal/wait owner负责interruption与ordinary
restart；VFS flock owner只返回normalized operation observation。

**依赖：** `FLOCK-WAIT-001`、现有signal restart semantics。

**违反表现：** 未知flag静默成功；wake直接等同success；为flock新增identity-preserving restart carrier；把
某一race winner固化为唯一ABI；retired holder因success path遗留持久grant。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

### FLOCK-WAIT-001 - Wait publication、predicate recheck 与 cooperative progress

**分类：** Correctness Invariant。

**规则：** conflict predicate、waiter publication与commit前final recheck由VFS flock owner在自己的
serialization boundary下闭合。notification只说明predicate可能变化；被唤醒者重验holder liveness、conflict与
signal。unlock、conversion old-mode removal与grant cleanup使多个shared waiter newly eligible时，协议让所有
这些waiter最终获得recheck机会。

terminal retirement即使没有删除grant，也为目标domain中已发布的waiter提交recheck notification。waiter在
自己的operation lifecycle中处理wake/signal outcome并完成cleanup；retirement producer不等待waiter运行，
不决定errno，也不维护第二份active/completed truth。

**Owner：** VFS flock owner拥有predicate、wait publication与notification source；scheduler wait owner拥有
wait identity、logical completion与physical wake delivery；operation owner拥有最终recheck与local cleanup。

**依赖：** `FLOCK-DOMAIN-001`、`SCHED-WAKE-001..004`。

**违反表现：** check-then-sleep lost wake；wake转移grant；无grant retirement遗漏notification；close等待
physical placement；waiter或retirement各自维护重复completion truth。

**Cutover：** `FLOCK-CUTOVER` 后写入VFS flock current contract。

### FLOCK-LIFECYCLE-001 - Terminal retirement 后无持久 grant

**分类：** Correctness Invariant。

**规则：** `task::files`单独拥有`Live(1) -> Retired`；VFS flock domain单独拥有grant commit/removal。协议必须
保证：已经完成的grant mutation会被subsequent retirement cleanup删除；已经进入retirement cleanup后的
operation在commit前重验liveness，不得建立retired-holder grant。final close返回后该holder没有持久grant。

该规则不要求为整个syscall outcome建立精确全序。operation可以根据实际完成的domain、liveness与signal
observation返回普通结果、`EBADF`、`EINTR`或ordinary restart；close只等待grant cleanup与recheck submission，
不等待operation结束。syscall-local borrow、lease、waiter或VFS storage lifetime都不延迟semantic final close，
retired identity永久不可republish。

**Owner：** retirement state与terminal episode属于`ProcFile` lifecycle owner；grant cleanup属于VFS flock
owner；wait outcome与operation-local cleanup属于waiter/scheduler owner。

**依赖：** `OPENED-DESC-001/002`、`OPENED-DESC-LIVENESS-001`、`OPENED-DESC-RETIRE-001`、
`FLOCK-DOMAIN-001`、`FLOCK-WAIT-001`。

**违反表现：** final close后仍有retired-holder grant；cleanup完成后旧syscall插回grant；temporary strong hold
推迟semantic final close；为了精确errno让close等待waiter；VFS读取或复制`description_refs`。

**Cutover：** `FLOCK-CUTOVER` 后与`OPENED-DESC-RETIRE-001`、VFS flock contract在同一closure中生效。

### FLOCK-TARGET-005 - Alias、exec 与 independent-open semantics

**分类：** Target Guarantee / Capability。

**规则：** dup/fork aliases共享同一holder并可由任一alias conversion/unlock；single-alias close不释放仍有
published aliases的grant；flock跨exec保留，`FD_CLOEXEC`只有关闭最后alias才触发cleanup；同一进程的
independent open是不同holder并相互冲突。

**Owner：** opened-description identity/liveness属于`task::files`；grant relation属于VFS flock domain。

**依赖：** `OPENED-DESC-001/002`、`FLOCK-DOMAIN-001`、`FLOCK-LIFECYCLE-001`。

**违反表现：** pid成为owner；关闭一个dup fd即解锁；fork复制mode真相；exec无条件解锁；independent open
绕过conflict。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

### FLOCK-TARGET-006 - Advisory 与 record-lock namespace 独立

**分类：** Target Guarantee / Capability。

**规则：** 没有参与flock protocol的ordinary read/write不因grant被拒绝；第一版local flock不与POSIX或OFD
record lock共享grant、waiter或conflict state。

**Owner：** VFS flock domain只拥有local flock namespace；其它lock families不进入本RFC。

**依赖：** `FLOCK-DOMAIN-001`。

**违反表现：** ordinary I/O被强制拒绝；fcntl lock与flock意外交互；为了复用合并三类owner/cleanup。

**Cutover：** N/A；产品能力随`FLOCK-CUTOVER`生效。

## RFC-local Invariants

### FLOCK-RFC-001 - R0 acceptance 与 cutover 分离

R0 semantic revision只接受target与contract delta；首个Ready stage是
acceptance前置证据，不会因此升级成target guarantee。`FLOCK-CUTOVER`只能由future transaction在代码、验证、
review与contract write-back同一closure中执行。任何部分能力不能先冒充effective flock。

### FLOCK-RFC-002 - Ready resolution 与 Active authorization 分离

2026-07-29的独立implementation-resolution任务已从live source把Stage 0完整解析为`Ready / Not Active`，
Stage 1保持`Outline`。后续独立review接受R0，开发者再明确授权transaction bootstrap与Stage 0 Active；这些
事实分别记录，不由`Ready`自动推出。Stage 0或任一checkpoint closure也不自动解析或激活Stage 1。

### FLOCK-RFC-003 - Generic facade 不预置 remote protocol

首版只允许syscall到VFS flock owner的stable facade与窄retirement cleanup handoff。没有remote consumer时，
不得在filesystem/backend/public API预留职责未闭合的optional hook。future remote支持必须经follow-up RFC
重新定义owner、RPC、wait/cancel、recovery、errno与record-lock interaction。

### FLOCK-RFC-004 - Acceptance evidence 不制造第二份行为真相

LTP/focused oracle证明ABI-visible behavior；owner-local tests/source audit证明single grant truth、lost-wake与
no-late-persistent-grant。race test只验证admissible outcome集合和最终状态，不能把某次调度结果固化为ABI。
测试不得向production path加入fake grant、lifecycle word getter或其它行为control plane。

### FLOCK-RFC-005 - Flock retirement handoff 不是扩展 registry

fixed VFS handoff只闭合opened-description terminal episode与当前VFS-owned flock relation。它不允许dynamic
registration、per-feature callback slot、backend dispatch、`has_flock`mirror或future participant自然加入。
其它relation需要独立contract review，不能复用本ID扩大同步teardown surface。

## 状态所有权

| 状态或事实 | 唯一 owner |
| --- | --- |
| fd number、slot publication、`FD_CLOEXEC` | `FilesState / FileDesc` |
| opened-description identity、published ref与terminal retirement | `ProcFile` lifecycle owner |
| terminal episode、窄VFS flock handoff与static hook顺序 | `ProcFile` lifecycle owner |
| VFS file identity与inode lifetime | VFS inode owner |
| holder × file identity grant relation、mode与conflict predicate | inode-associated VFS flock domain |
| wait publication、predicate recheck、grant commit/removal与notification source | VFS flock protocol owner |
| wait identity、signal outcome与physical wake delivery | scheduler / wait owner |
| operation-local recheck、errno completion与local wait cleanup | flock syscall operation owner |
| Linux operation parsing与fd admission | syscall ABI adapter |

不存在“task/VFS共同拥有”或“ProcFile/inode各保存一份mode”的合法状态。notification capability不转移grant、
wait identity或cleanup ownership。

## 身份与能力模型

- opened-description identity只由`task::files`产生并比较；fd number、path、inode、pid、task、raw pointer与
  `Weak::upgrade()`成功都不能替代。
- file identity由VFS inode owner提供；flock domain只使用owner-approved identity，不建立临时`(dev, ino)`
  replica。
- VFS可持opaque opened-description capability/context，用于same-identity comparison、operation-local access与
  commit-time liveness validation；不得取得完整`ProcFile`、`FileDesc`、fd-table lock或lifecycle word。
- operation-local lease只保证storage在当前operation内可访问，不保证semantic liveness，不能被grant长期保存，
  也不形成identity-preserving restart carrier。
- retirement context只服务exactly-once flock cleanup handoff；它不允许取得live lease、重新publication identity、
  注册callback或逃逸成长期feature state。

## 线性化点

### Acquire / same-mode request

success truth只能来自同一flock-domain serialization中的final conflict recheck、holder liveness observation与
grant commit。前置snapshot、notification、wake或candidate都不是success truth。

### Conversion

old-mode removal与target-mode competition是两个externally observable steps。old removal一旦成立，后续
conflict、signal、retirement或其它ordinary failure不恢复旧grant。

### Unlock

unlock在线性化为domain删除同一holder relation；没有relation时是owner-local no-op，不扫描其它holder。

### Retirement 与 grant convergence

`Live(1) -> Retired`由`ProcFile` lifecycle owner线性化。窄VFS handoff随后进入grant owner完成cleanup与
recheck submission。operation若已完成grant mutation，cleanup删除结果；cleanup若先进入domain，后续commit
重验liveness并拒绝建立grant。这里不定义syscall return、signal completion或waiter execution的全序。

### Cooperative wait

waiter publication与predicate recheck闭合lost-wake；retirement notification只提交一次recheck机会。waiter在
自己的wait identity上竞争event、signal或其它completion并最终重验，不把notification当成行为真相。

## 锁序与生命周期规则

- fd-table mutation先unpublish，释放fd-table private guard后才进入external flock cleanup。
- terminal owner先提交`Retired`，再进入VFS flock handoff；grant cleanup与recheck submission后才运行static
  `FileDescOps::final_release`。
- VFS flock path不回取fd-table private lock，不修改publication lifecycle。
- domain serialization不跨task park持有；wait publication必须能在serialization释放后安全park并在wake后重验。
- scheduler wake/placement不回调flock domain完成grant commit，close也不观察placement result。
- waiter在operation return前完成自己的local wait cleanup；retirement path不等待这一动作。
- `Drop`与assertion不是semantic grant cleanup owner。

Stage 0当前选择的锁、allocation/destruction位置、`Event`与module placement见`implementation.md`；它们是
可由live evidence修正的implementation preference，不属于当前target。Stage 1的具体选择仍留给后续
resolution gate。

## 禁止退化项

- 把wake、queue entry、candidate bit或diagnostic identity当作grant/readiness truth。
- 在`ProcFile`或`File`保存mode mirror，再用另一registry汇总conflict。
- 用fd number、path、pid、task或temporary inode number替代holder/file identity。
- 让syscall或filesystem backend直接增删grant，绕过VFS flock owner。
- 用syscall-local strong borrow延迟semantic final close，或用`Weak::upgrade()`判断owner live。
- final close后允许retired holder遗留grant，或cleanup后让旧operation重新插入grant。
- 要求close等待waiter运行、return、physical placement或operation-local resource destruction。
- 为flock新增跨signal保存opened-description identity的restart carrier，或精确规定retirement必须压过signal。
- 只设置`Retired`却不给已发布waiterrecheck机会，同时仍声称blocked operation能协作式收敛。
- 覆盖现有`FileDescOps::final_release`，或增加dynamic observer registry、callback slot、`has_flock`mirror。
- 借本RFC迁移`ProcFile`owner或把flock handoff扩张为通用backend/feature lifecycle framework。
- 为`ENOLCK`、future record locks或remote backend预先扭曲自然数据结构。
- 把公平性、FIFO、无惊群、cancel latency或deadlock detection写成首版correctness invariant。
- 把Stage 0 Ready中冻结的stage、API、锁、container、write set或测试命令当成target guarantee，或让Stage 0
  closure自动解析/激活Stage 1。

## R0 acceptance 与 Stage 0 activation 记录

2026-07-29 的 R0 review 与 activation preflight 已确认：

1. `index.md`、本文、tracking与`docs/src/rfcs.md`对cooperative semantics保持一致；
2. KETER-FLOCK-004已将旧precise-cancellation target的修复折回canonical target；
3. APOLLYON-FLOCK-003继续由recheck notification闭合，但其依据不再要求精确`EBADF`或同步waiter teardown；
4. KETER-FLOCK-001/002的历史决定有明确supersession，且owner、handoff、ABI admissible outcomes无歧义；
5. implementation preferences未被提升为target，`implementation.md`的Stage 0达到完整`Ready`，Stage 1保持
   依赖、受保护边界与resolution trigger完整的`Outline`；
6. R0 acceptance、transaction bootstrap 与 Stage 0 Active authorization 分别记录在
   [transaction](../../devlog/transactions/2026-07-29-flock.md)，Checkpoint 0S、0A 的独立授权与closure没有自动
   授权后续checkpoint；
7. `OPENED-DESC-RETIRE-001`与全部`FLOCK-*` IDs保持Not Effective，0A的private substrate不提供partial flock
   capability。

第一个可执行stage已按独立授权进入Active且关闭至Checkpoint 0A；Checkpoint 0B仍未激活。最终implementation
closure仍需逐项记录每个ID的Effective / Not Cut Over结果，并区分agent-run、developer-run与Not Run evidence。
