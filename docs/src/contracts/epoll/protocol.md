# Epoll Protocol 当前契约

**Contract ID：** `EPOLL`
**状态：** Active
**Owner：** 每个 `Epoll` instance；watch-local policy与generation由对应 `EpollWatch` 拥有
**参与领域：** fs / iomux / task opened-description / scheduler latch / signal / Linux syscall ABI
**覆盖范围：** watch publication/lifecycle、LT/ET/ONESHOT delivery、bounded scan/copyout、epoll-file pollability
**不覆盖：** target source predicate、source-private route容器、nested epoll、socket readiness、MM fork行为
**实现位置：** `anemone-kernel/src/fs/epoll/`、`anemone-kernel/src/fs/epoll/api/`
**依赖：** `IOMUX-POLL-001..003`、`OPENED-DESC-001..003`、`OPENED-DESC-LIVENESS-001`、`SCHED-LATCH-001..003`、`SIGNAL-TEMP-MASK-001..003`
**Pending Successor：** None
**最后核验：** 2026-07-31

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| target当前readiness与route entries | 具体source state / registry | source registry持non-owning `PollRoute`；epoll watch提供`PollObserver`与operation-local snapshot | readiness truth与重扫提示 |
| interest、user data、LT/ET/ONESHOT、active generation | 对应 `EpollWatch`，由instance operation串行publication | syscall adapter只传入/取回Linux UAPI值 | watch identity与delivery policy |
| ET pending recheck causality | 对应live watch generation的sticky dirty claim | source callback只能发布hint | 防止edge丢失且拒绝stale generation |
| bounded scan、fairness cursor、ONESHOT disable与copyout policy | `EpollOperation`，由instance operation mutex串行 | wait syscall持未提交batch | delivery commit / rollback |
| epoll-file wait coverage与固定route slots | `EpollWaitPublication` | iomux consumer持non-owning route | 判断能否park，不表示readiness |
| target terminal liveness | `task::files` opened-description lifecycle | watch持non-owning capability，operation取得短lease | final-close后fail closed |

## EPOLL-WATCH-001 — Watch Ownership

**规则：** 每个watch以opened-description identity与用户fd key定位；`Epoll` / `EpollWatch`唯一拥有
interest、user data、generation、LT/ET/ONESHOT policy与callback acceptance。watch不得长期强持target；
ctl与scan只取得operation-local live lease并在commit前复核terminal liveness。每个instance的sleepable
operation mutex串行ADD/MOD/DEL、teardown、scan与copyout policy；source callback不得取得该mutex。

**违反表现：** fd reuse继承旧user data、final close后旧watch复活、source取得consumer policy、强引用环、
callback与ctl互锁，或两个结构同时推进watch active/generation truth。

**验证 / Enforcement：** watch slot/generation与opened-description capability audit；focused
`lifecycle-epoll-file`、`ctl-callback-race`、dup/close/reuse与LTP ctl/errno matrix。

**最初来源：** [RFC-20260726-epoll R2](../../rfcs/epoll/index.md)。

**当前来源：** [Stage 2 Checkpoint 2D / EPOLL-CUTOVER](../../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2d-closure-and-epoll-cutover---2026-07-27)。

## EPOLL-READY-001 — Bounded Exact Scan

**规则：** epoll不维护ready queue或ready bitmap。每次operation在固定watch上界内扫描target predicate：
LT每轮重新求值；ET只消费对应live generation的sticky dirty claim；ONESHOT disable、round-robin cursor与
copyout commit/rollback由同一operation owner提交。source notification只是dirty/recheck hint，不能成为
用户可见readiness truth。copyout失败或未提交batch必须恢复ET claim与ONESHOT policy，并重新发布activity。

**违反表现：** callback payload直接交付、stale generation消费新watch、LT依赖历史queue、maxevents长期饿死
其它watch、fault后丢edge/错误disable ONESHOT，或coverage/diagnostic字段反向驱动ready结果。

**验证 / Enforcement：** operation/scan/claim/rollback常开assertions与source audit；focused
`wait-copyout-rollback`、`lt-et-oneshot`、producer/harvest races、fairness与multiple-waiters；双libc epoll matrix。

**最初来源：** [RFC-20260726-epoll R1/R2](../../rfcs/epoll/invariants.md#operation-serialized-scandirty-与-copyout)。

**当前来源：** [Stage 2 Checkpoint 2D / EPOLL-CUTOVER](../../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2d-closure-and-epoll-cutover---2026-07-27)。

## EPOLL-FILE-001 — Non-sleeping Wait Publication

**规则：** epoll file的exact readability只能由operation scan求值。active iomux register只进入独立
irqsave spinlock，在固定容量route slots中发布route，并维护 `Uncovered / Checking / EmptyCovered`
certificate。certificate只证明一次complete empty scan是否覆盖当前publication，不缓存readiness。
无法证明empty时必须返回 `SubscribedRecheck`、在guard外self-hint并阻止consumer park；guard内不得
allocation、target poll、sleep、notify或drop final reference。首版对epoll fd作为watch target返回
`EINVAL`并记录notice，不开放nested epoll。

**违反表现：** active wait取得operation mutex、coverage被当成ready bit、route未armed却park、spinlock内
sleep/allocate/callback/final drop、容量耗尽后静默睡眠，或nested epoll意外成功。

**验证 / Enforcement：** wait-publication状态/锁序与fixed-capacity audit；focused
`lifecycle-epoll-file`、`multiple-waiters`、wide-harvest race，KUnit与RV64 product-path matrix。

**最初来源：** [RFC-20260726-epoll R1/R2](../../rfcs/epoll/invariants.md#epoll-file-readiness)。

**当前来源：** [Stage 2 Checkpoint 2D / EPOLL-CUTOVER](../../devlog/transactions/2026-07-26-epoll.md#stage-2-checkpoint-2d-closure-and-epoll-cutover---2026-07-27)。
